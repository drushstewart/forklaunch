//! RSA generation/encoding and ring's native private-key signing operations.
use super::{
    BlueprintResult, Engine, SystemRandom, URL_SAFE_NO_PAD, Value, Zeroizing, json, key_error,
};
use rsa::{
    RsaPrivateKey,
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
    traits::PublicKeyParts,
};

pub(super) struct RsaKey {
    pair: ring::signature::RsaKeyPair,
    n: Vec<u8>,
    e: Vec<u8>,
}
impl RsaKey {
    pub(super) fn generate(bits: usize) -> BlueprintResult<Zeroizing<Vec<u8>>> {
        if !(2048..=4096).contains(&bits) {
            return Err(key_error());
        }
        let key = RsaPrivateKey::new(&mut rand_core::OsRng, bits).map_err(|_| key_error())?;
        let encoded = key.to_pkcs8_der().map_err(|_| key_error())?;
        Ok(Zeroizing::new(encoded.as_bytes().to_vec()))
    }
    pub(super) fn restore(encoded: &[u8]) -> BlueprintResult<Self> {
        let key = RsaPrivateKey::from_pkcs8_der(encoded).map_err(|_| key_error())?;
        Ok(Self {
            pair: ring::signature::RsaKeyPair::from_pkcs8(encoded).map_err(|_| key_error())?,
            n: key.n().to_bytes_be(),
            e: key.e().to_bytes_be(),
        })
    }
    pub(super) fn public(&self, id: &str, algorithm: &str) -> Value {
        json!({"alg":algorithm,"kty":"RSA","n":URL_SAFE_NO_PAD.encode(&self.n),"e":URL_SAFE_NO_PAD.encode(&self.e),"kid":id})
    }
    pub(super) fn sign(&self, content: &[u8], algorithm: &str) -> BlueprintResult<Vec<u8>> {
        use ring::signature::{
            RSA_PKCS1_SHA256, RSA_PKCS1_SHA384, RSA_PKCS1_SHA512, RSA_PSS_SHA256, RSA_PSS_SHA384,
            RSA_PSS_SHA512, RsaEncoding,
        };
        let padding: &'static dyn RsaEncoding = match algorithm {
            "RS256" => &RSA_PKCS1_SHA256,
            "RS384" => &RSA_PKCS1_SHA384,
            "RS512" => &RSA_PKCS1_SHA512,
            "PS256" => &RSA_PSS_SHA256,
            "PS384" => &RSA_PSS_SHA384,
            "PS512" => &RSA_PSS_SHA512,
            _ => return Err(key_error()),
        };
        let mut signature = vec![0; self.pair.public().modulus_len()];
        self.pair
            .sign(padding, &SystemRandom::new(), content, &mut signature)
            .map_err(|_| key_error())?;
        Ok(signature)
    }
}
