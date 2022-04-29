use std::collections::HashMap;

use secrecy::SecretString;
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
#[serde(tag = "type")]
pub enum Credential {
    User(UserCredential),
    Anonymous(AnonymousCredential),
}

impl Credential {
    pub fn new_user(cred: UserCredential) -> Self {
        Self::User(cred)
    }

    pub fn new_anonymous(cred: AnonymousCredential) -> Self {
        Self::Anonymous(cred)
    }

    pub fn client_access_token(&self) -> &SecretString {
        match self {
            Credential::User(val) => &val.client_access_token,
            Credential::Anonymous(val) => &val.client_access_token,
        }
    }

    pub fn base_sub_domain(&self) -> &String {
        match self {
            Credential::User(val) => &val.base_sub_domain,
            Credential::Anonymous(val) => &val.base_sub_domain,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserCredential {
    pub email: String,
    #[serde(serialize_with = "models::serialize_secret_string")]
    pub client_access_token: SecretString,
    pub base_sub_domain: String,
}

impl UserCredential {
    pub fn new(email: String, client_access_token: SecretString, base_sub_domain: String) -> Self {
        Self {
            email,
            client_access_token,
            base_sub_domain,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AnonymousCredential {
    pub base_sub_domain: String,
    #[serde(serialize_with = "models::serialize_secret_string")]
    pub client_access_token: SecretString,
    #[serde(serialize_with = "models::serialize_secret_string")]
    pub access_code: SecretString,
}

impl AnonymousCredential {
    pub fn new(
        base_sub_domain: String,
        client_access_token: SecretString,
        access_code: SecretString,
    ) -> Self {
        Self {
            base_sub_domain,
            client_access_token,
            access_code,
        }
    }
}
