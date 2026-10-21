//! Application key configuration and the stable Fieldnotes wire namespace.
use crate::SecurityError;
use forklaunch_core::encryption::{EnvelopeError, EnvelopeKey as Keyring, EnvelopeProfile};
use uuid::Uuid;
use zeroize::Zeroizing;
pub struct EnvelopeKey(Keyring);
fn profile() -> EnvelopeProfile {
    EnvelopeProfile::new("fieldnotes").expect("static namespace")
}
fn mapped(e: EnvelopeError) -> SecurityError {
    match e {
        EnvelopeError::InvalidInput => SecurityError::InvalidInput,
        EnvelopeError::Encryption => SecurityError::Encryption,
    }
}
impl EnvelopeKey {
    pub fn new(key: [u8; 32]) -> Self {
        Self(Keyring::new(profile(), key))
    }
    pub fn with_rotation(
        id: String,
        key: [u8; 32],
        previous: Vec<(String, [u8; 32])>,
        legacy: Option<[u8; 32]>,
    ) -> Result<Self, SecurityError> {
        Keyring::with_rotation(profile(), id, key, previous, legacy)
            .map(Self)
            .map_err(mapped)
    }
    pub fn seal(
        &self,
        tenant: Uuid,
        resource: Uuid,
        purpose: &str,
        plaintext: &[u8],
    ) -> Result<String, SecurityError> {
        self.0
            .seal(tenant, resource, purpose, plaintext)
            .map_err(mapped)
    }
    pub fn open(
        &self,
        tenant: Uuid,
        resource: Uuid,
        purpose: &str,
        envelope: &str,
    ) -> Result<Zeroizing<Vec<u8>>, SecurityError> {
        self.0
            .open(tenant, resource, purpose, envelope)
            .map_err(mapped)
    }
    /// Framework-owned key loading and bounded rotation configuration.
    pub fn from_env() -> Result<Self, SecurityError> {
        Keyring::from_env(profile()).map(Self).map_err(mapped)
    }
    pub fn as_keyring(&self) -> &Keyring {
        &self.0
    }
}
