use secrecy::{CloneableSecret, DebugSecret, Secret, SerializableSecret, Zeroize};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Token(Uuid);

impl Token {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
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
