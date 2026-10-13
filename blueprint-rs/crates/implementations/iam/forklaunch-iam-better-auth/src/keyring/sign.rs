//! Configured trusted signing helper with persistent key side effects.
mod claims;
mod duration;
mod schema;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, KeyRing, Model, Utc,
    Value, json, mint_config, optional_time, required_time, restore,
};
use std::cmp::Reverse;
impl<S: AuthStore> KeyRing<S> {
    /// Signs a trusted server payload using persisted configured keys.
    /// Schema/default-duration errors precede key selection; claim errors follow it.
    /// # Errors
    /// Returns schema, configured claim/options or sanitized storage/key errors.
    pub async fn sign_jwt(
        &self,
        body: Option<&Value>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        schema::validate(body)?;
        let body = body.ok_or_else(|| error("Invalid signing input"))?;
        let payload = &body["payload"];
        let options = &body["overrideOptions"];
        let clock = json!(now.timestamp())
            .as_f64()
            .ok_or_else(|| error("Invalid signing time"))?;
        let expiry = claims::expiry(payload, options, clock)?;
        let mut tx = self.store.begin(true).await?;
        let primary = options["jwks"]["keyPairConfig"]["alg"]
            .as_str()
            .unwrap_or("EdDSA");
        let mut candidates = Vec::new();
        for row in tx.list_page(Model::Jwks, None, 100).await?.records {
            if optional_time(&row, "expiresAt")?.is_none_or(|expiry| expiry > now) {
                candidates.push((required_time(&row, "createdAt")?, row));
            }
        }
        candidates.sort_by_key(|(created, _)| Reverse(*created));
        let selected = candidates
            .iter()
            .find(|(_, row)| row["alg"] == primary || row["alg"].is_null())
            .or_else(|| candidates.first());
        let key = if let Some((_, row)) = selected {
            restore(row)?
        } else {
            let curve = match primary {
                "EdDSA" | "Ed25519" => "Ed25519",
                "ES256" => "P-256",
                "ES384" => "P-384",
                "ES512" => "P-521",
                "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512" => "",
                _ => return Err(error("Unsupported signing-key algorithm")),
            };
            if !curve.is_empty()
                && options["jwks"]["keyPairConfig"]
                    .get("crv")
                    .is_some_and(|value| value != curve)
            {
                return Err(error(format!(
                    "Invalid or unsupported crv option provided, the only supported value for {primary} is {curve}"
                )));
            }
            let bits = if curve.is_empty() {
                match options["jwks"]["keyPairConfig"].get("modulusLength").filter(|value| !value.is_null()) {
                    None => 2048,
                    Some(value) => value.as_u64().filter(|bits| (2048..=4096).contains(bits)).and_then(|bits| usize::try_from(bits).ok()).ok_or_else(|| error("Invalid or unsupported modulusLength option provided, 2048 bits or larger keys must be used"))?,
                }
            } else {
                2048
            };
            let key = mint_config(&mut tx, now, primary, bits).await?;
            if let Some(interval) = options["jwks"]["rotationInterval"]
                .as_f64()
                .filter(|interval| *interval != 0.0)
            {
                let clock_ms = json!(now.timestamp_millis())
                    .as_f64()
                    .ok_or_else(|| error("Invalid signing time"))?;
                let millis = clock_ms + interval * 1_000.0;
                let millis = format!("{:.0}", millis.trunc())
                    .parse::<i64>()
                    .map_err(|_| error("Invalid key expiration"))?;
                let expires = DateTime::<Utc>::from_timestamp_millis(millis)
                    .ok_or_else(|| error("Invalid key expiration"))?;
                let mut row = tx
                    .get(
                        Model::Jwks,
                        key.public_jwk()["kid"]
                            .as_str()
                            .ok_or_else(|| error("Missing signing key"))?,
                    )
                    .await?
                    .ok_or_else(|| error("Missing signing key"))?;
                row["expiresAt"] = json!(super::timestamp(expires));
                tx.replace(Model::Jwks, row).await?;
            }
            key
        };
        // Better Auth persists a newly minted key even if later claim setters fail.
        tx.commit().await?;
        let payload = claims::build(payload, options, expiry, &self.base_url, clock)?;
        Ok(json!({"token":key.sign_payload(&payload)?}))
    }
}
fn error(message: impl Into<String>) -> BlueprintError {
    BlueprintError::new("better_auth_jwt_sign_input", message)
}

pub(super) fn custom_payload(
    body: Option<&Value>,
    now: DateTime<Utc>,
    base: &str,
) -> BlueprintResult<Value> {
    schema::validate(body)?;
    let body = body.ok_or_else(|| error("Invalid signing input"))?;
    let payload = &body["payload"];
    let options = &body["overrideOptions"];
    let clock = json!(now.timestamp())
        .as_f64()
        .ok_or_else(|| error("Invalid signing time"))?;
    let expiry = claims::expiry(payload, options, clock)?;
    let mut result = payload.clone();
    if let Some(record) = result.as_object_mut() {
        record.shift_remove("__proto__");
    }
    result["exp"] = match expiry {
        claims::Expiry::Claim(value) => value,
        claims::Expiry::InvalidNumber => {
            return Err(error("Custom signing requires finite JSON claims"));
        }
    };
    for (claim, option) in [("iss", "issuer"), ("aud", "audience")] {
        result[claim] = payload
            .get(claim)
            .filter(|v| !v.is_null())
            .or_else(|| options["jwt"].get(option).filter(|v| !v.is_null()))
            .cloned()
            .unwrap_or_else(|| json!(base));
    }
    Ok(result)
}
