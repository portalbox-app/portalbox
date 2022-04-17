use secrecy::{CloneableSecret, DebugSecret, Secret, SerializableSecret, Zeroize};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Token(Uuid);

impl Token {
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Zeroize for Token {
    fn zeroize(&mut self) {
        self.0 = Uuid::nil();
    }
}

/// Permits cloning
impl CloneableSecret for Token {}

/// Provides a `Debug` impl (by default `[[REDACTED]]`)
impl DebugSecret for Token {}

impl SerializableSecret for Token {}

/// Use this alias when storing secret values
pub type SecretToken = Secret<Token>;
