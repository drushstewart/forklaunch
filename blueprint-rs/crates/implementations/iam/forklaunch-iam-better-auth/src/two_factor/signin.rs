use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, Engine, Model, TwoFactorChallenge,
    TwoFactorService, Utc, Value, crypto, error, expiry, json, text, time, verification_row,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::hmac;

pub(crate) enum SigninGate {
    Trusted(String),
    Challenge(TwoFactorChallenge),
}
impl<S: AuthStore> TwoFactorService<S> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn gate<T: AuthTransaction>(
        &self,
        tx: &mut T,
        user: &Value,
        trust: Option<&str>,
        persistent: bool,
        ip: &str,
        agent: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<SigninGate> {
        if let Some((signature, identifier)) = trust.and_then(|value| value.split_once('!'))
            && identifier.starts_with("trust-device-")
            && identifier.len() <= 128
            && let Ok(signature) = URL_SAFE_NO_PAD.decode(signature)
            && hmac::verify(
                &hmac::Key::new(hmac::HMAC_SHA256, &self.secret),
                format!("{}!{identifier}", text(user, "id")?).as_bytes(),
                &signature,
            )
            .is_ok()
            && let Some(row) = verification_row(tx, identifier).await?
            && row["value"] == user["id"]
            && time(&row, "expiresAt")? > now
        {
            tx.delete(Model::Verification, text(&row, "id")?).await?;
            return Ok(SigninGate::Trusted(self.create_trust(tx, user, now).await?));
        }
        let row = tx
            .find_unique(Model::TwoFactor, "userId", text(user, "id")?)
            .await?;
        let mut methods = Vec::new();
        if !self.lifecycle_options.disable_totp
            && row.as_ref().is_some_and(|row| row["verified"] != false)
        {
            methods.push("totp");
        }
        if self.otp_delivery.is_some() {
            methods.push("otp");
        }
        if methods.is_empty() && !self.lifecycle_options.disable_totp {
            return Err(error("TOTP_NOT_ENABLED", "TOTP is not enabled"));
        }
        let identifier = format!("2fa-{}", crypto::opaque(20)?);
        tx.insert(Model::Verification,json!({"id":uuid::Uuid::new_v4().to_string(),"identifier":identifier,"value":user["id"],"expiresAt":expiry(now,self.lifetime_options.challenge_seconds())?,"attempts":0,"persistent":persistent,"ipAddress":ip,"userAgent":agent})).await?;
        Ok(SigninGate::Challenge(TwoFactorChallenge {
            identifier,
            persistent,
            methods,
        }))
    }
    pub(super) async fn create_trust<T: AuthTransaction>(
        &self,
        tx: &mut T,
        user: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<String> {
        let identifier = format!("trust-device-{}", crypto::opaque(32)?);
        let signature = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, &self.secret),
            format!("{}!{identifier}", text(user, "id")?).as_bytes(),
        );
        tx.insert(Model::Verification,json!({"id":uuid::Uuid::new_v4().to_string(),"identifier":identifier,"value":user["id"],"expiresAt":expiry(now,self.lifetime_options.trust_seconds())?})).await?;
        Ok(format!(
            "{}!{identifier}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
}
