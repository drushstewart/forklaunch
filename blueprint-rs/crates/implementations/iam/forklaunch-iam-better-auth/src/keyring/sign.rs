//! Configured trusted signing helper with persistent key side effects.
mod claims;
mod duration;
mod schema;
use super::{
    AuthStore, BlueprintError, BlueprintResult, DateTime, KeyRing, Utc, Value, json, mint_session,
};
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
        self.sign_selected(body, None, None, now).await
    }
    /// Signs using an explicitly provisioned key ID, optionally requiring its algorithm.
    /// Missing, expired, mismatched or public-only keys never trigger replacement minting.
    /// # Errors
    /// Returns sanitized input, storage or key errors. IDs must be nonempty and at most 512 bytes.
    pub async fn sign_jwt_with_key_id(
        &self,
        body: Option<&Value>,
        key_id: &str,
        expected_algorithm: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        if key_id.is_empty()
            || key_id.len() > 512
            || expected_algorithm.is_some_and(|algorithm| algorithm.len() > 16)
        {
            return Err(error("Invalid signing key constraint"));
        }
        self.sign_selected(body, Some((key_id, expected_algorithm)), None, now)
            .await
    }
    /// Selects the newest live key matching a trusted algorithm constraint.
    /// Missing keys mint only the configured primary or an advertised extra profile.
    /// A trusted creation callback owns the returned key and may substitute its
    /// algorithm; this selects a generation profile, not a post-callback guarantee.
    /// # Errors
    /// Rejects unsupported constraints, unadvertised minting and invalid selected keys.
    pub async fn sign_jwt_with_algorithm(
        &self,
        body: Option<&Value>,
        algorithm: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        super::KeyPairConfig::new(algorithm, 2048)?;
        self.sign_selected(body, None, Some(algorithm), now).await
    }
    async fn sign_selected(
        &self,
        body: Option<&Value>,
        pin: Option<(&str, Option<&str>)>,
        algorithm_pin: Option<&str>,
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
        // The upstream server helper shallow-merges overrideOptions: providing
        // jwks replaces the configured object rather than deep-merging it.
        let overrides_jwks = options.get("jwks").is_some();
        let default_algorithm = if overrides_jwks {
            "EdDSA"
        } else {
            self.key_config.algorithm()
        };
        let default_bits = if overrides_jwks {
            2048
        } else {
            self.key_config.rsa_bits()
        };
        if algorithm_pin.is_some() && options["jwks"].get("keyPairConfigs").is_some() {
            return Err(error("Use trusted additional key profiles"));
        }
        let configured_primary = options["jwks"]["keyPairConfig"]["alg"]
            .as_str()
            .unwrap_or(default_algorithm);
        let primary = algorithm_pin.unwrap_or(configured_primary);
        let extra_profile = if overrides_jwks {
            None
        } else {
            self.key_config.additional_profile(primary)
        };
        let default_bits = extra_profile.map_or(default_bits, super::KeyPairConfig::rsa_bits);
        let configured_expiration = if overrides_jwks {
            None
        } else {
            self.key_config.expiration(now)?
        };
        let mut tx = self.repository(true, now);
        let selected = if let Some((id, expected_algorithm)) = pin {
            let row = tx
                .by_id(id)
                .await?
                .ok_or_else(|| error("Requested signing key unavailable"))?;
            let effective_algorithm = row.effective_algorithm(primary)?;
            if row
                .expires_at()?
                .is_some_and(|expiry| expiry.timestamp_millis() < now.timestamp_millis())
                || expected_algorithm.is_some_and(|algorithm| effective_algorithm != algorithm)
            {
                return Err(error("Requested signing key unavailable"));
            }
            Some(row)
        } else if let Some(algorithm) = algorithm_pin {
            tx.by_algorithm(algorithm, configured_primary).await?
        } else {
            tx.select(primary).await?
        };
        if selected.is_none()
            && algorithm_pin.is_some()
            && primary != configured_primary
            && extra_profile.is_none()
        {
            return Err(error(
                "Requested algorithm is not provisioned or advertised",
            ));
        }
        let key = if let Some(row) = selected {
            row.signing_key_with_algorithm(primary)?
        } else {
            let (bits, expires_at) =
                mint_options(options, primary, default_bits, configured_expiration, now)?;
            mint_session(&mut tx, now, primary, bits, expires_at).await?
        };
        // Better Auth persists a newly minted key even if later claim setters fail.
        tx.commit().await?;
        let payload = claims::build(payload, options, expiry, &self.base_url, clock)?;
        Ok(json!({"token":key.sign_payload(&payload)?}))
    }
}
fn mint_options(
    options: &Value,
    primary: &str,
    default_bits: usize,
    configured_expiration: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> BlueprintResult<(usize, Option<DateTime<Utc>>)> {
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
            None => default_bits,
            Some(value) => value.as_u64().filter(|bits| (2048..=4096).contains(bits)).and_then(|bits| usize::try_from(bits).ok()).ok_or_else(|| error("Invalid or unsupported modulusLength option provided, 2048 bits or larger keys must be used"))?,
        }
    } else {
        2048
    };
    let expires_at = if let Some(interval) = options["jwks"]["rotationInterval"]
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
        Some(expires)
    } else {
        configured_expiration
    };
    Ok((bits, expires_at))
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

// Same local JOSE claim setters, without repository selection or side effects.
pub(super) fn resolved_payload(
    body: Option<&Value>,
    now: DateTime<Utc>,
    base: &str,
) -> BlueprintResult<Value> {
    schema::validate(body)?;
    let body = body.ok_or_else(|| error("Invalid signing input"))?;
    let clock = json!(now.timestamp())
        .as_f64()
        .ok_or_else(|| error("Invalid signing time"))?;
    let expiry = claims::expiry(&body["payload"], &body["overrideOptions"], clock)?;
    claims::build(
        &body["payload"],
        &body["overrideOptions"],
        expiry,
        base,
        clock,
    )
}
