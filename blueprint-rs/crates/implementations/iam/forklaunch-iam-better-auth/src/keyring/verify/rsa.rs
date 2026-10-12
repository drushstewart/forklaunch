//! RSA public verification binds the token algorithm to the imported key profile.
use super::{Value, decode_jwk};
pub(super) fn verify(jwk: &Value, algorithm: &str, content: &[u8], signature: &[u8]) -> bool {
    use ring::signature::{
        RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_2048_8192_SHA384, RSA_PKCS1_2048_8192_SHA512,
        RSA_PSS_2048_8192_SHA256, RSA_PSS_2048_8192_SHA384, RSA_PSS_2048_8192_SHA512,
        RsaPublicKeyComponents,
    };
    let verification = match algorithm {
        "RS256" => &RSA_PKCS1_2048_8192_SHA256,
        "RS384" => &RSA_PKCS1_2048_8192_SHA384,
        "RS512" => &RSA_PKCS1_2048_8192_SHA512,
        "PS256" => &RSA_PSS_2048_8192_SHA256,
        "PS384" => &RSA_PSS_2048_8192_SHA384,
        "PS512" => &RSA_PSS_2048_8192_SHA512,
        _ => return false,
    };
    if jwk["kty"] != "RSA" {
        return false;
    }
    let Some(n) = jwk["n"].as_str().and_then(decode_jwk) else {
        return false;
    };
    let Some(e) = jwk["e"].as_str().and_then(decode_jwk) else {
        return false;
    };
    RsaPublicKeyComponents { n, e }
        .verify(verification, content, signature)
        .is_ok()
}
