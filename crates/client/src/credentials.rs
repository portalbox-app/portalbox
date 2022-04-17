use std::collections::HashMap;

use models::secrets::SecretToken;
use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct CredManager {
    pub credentials: HashMap<String, Credential>,
}

impl CredManager {
    pub async fn save(&self, config: &Config) -> Result<(), anyhow::Error> {
        let contents = toml::to_string_pretty(self)?;

        let filepath = config.credentials_file_path();
        let _ = tokio::fs::write(filepath, contents).await?;
        Ok(())
    }

    pub async fn load(config: &Config) -> Result<CredManager, anyhow::Error> {
        // Load previously saved session
        let filepath = config.credentials_file_path();
        let file_content = tokio::fs::read_to_string(filepath).await?;

        let session = toml::from_str(&file_content)?;

        Ok(session)
    }

    pub async fn delete(config: &Config) -> Result<(), anyhow::Error> {
        let filepath = config.credentials_file_path();
        let _ = tokio::fs::remove_file(filepath).await?;
        Ok(())
    }

    pub fn empty() -> Self {
        Self {
            credentials: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Credential {
    pub email: String,
    pub client_access_token: SecretToken,
    pub base_hostname: String,
}

impl Credential {
    pub fn new(email: String, client_access_token: SecretToken, base_hostname: String) -> Self {
        Self {
            email,
            client_access_token,
            base_hostname,
        }
    }
}
