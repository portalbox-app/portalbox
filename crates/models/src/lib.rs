pub mod consts;
pub mod protocol;
pub mod utils;

use std::path::PathBuf;

pub use crate::utils::serialize_secret_string;

use secrecy::SecretString;
use semver::Version;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct GrantTokenResp {
    #[serde(serialize_with = "serialize_secret_string")]
    pub access_token: SecretString,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(serialize_with = "serialize_secret_string")]
    pub refresh_token: SecretString,
    pub user: User,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub aud: String,
    pub role: String,
    pub email: String,
    pub email_confirmed_at: String,
    pub phone: String,
    pub confirmation_sent_at: String,
    pub confirmed_at: String,
    pub last_sign_in_at: String,
    pub app_metadata: AppMetadata,
    pub user_metadata: serde_json::Value,
    pub identities: Vec<Identity>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppMetadata {
    pub provider: String,
    pub providers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Identity {
    pub id: String,
    pub user_id: String,
    pub identity_data: IdentityData,
    pub provider: String,
    pub last_sign_in_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdentityData {
    pub sub: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserData {
    pub user_id: Uuid,
    pub hostname: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignIn {
    pub email: String,
    #[serde(serialize_with = "serialize_secret_string")]
    pub password: SecretString,
    #[serde(default, rename = "remember-me")]
    pub remember_me: bool,
    pub base_sub_domain: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignInAccessCode {
    pub base_sub_domain: String,
    pub access_code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignInResult {
    #[serde(serialize_with = "serialize_secret_string")]
    pub client_access_token: SecretString,
    pub base_sub_domain: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SigninGuestResult {
    #[serde(serialize_with = "serialize_secret_string")]
    pub client_access_token: SecretString,
    pub base_sub_domain: String,
    #[serde(serialize_with = "serialize_secret_string")]
    pub access_code: SecretString,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Contact {
    #[serde(default, rename = "first-name")]
    pub first_name: String,
    #[serde(default, rename = "last-name")]
    pub last_name: String,
    pub email: String,
    pub phone: Option<String>,
    pub subject: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceRequest {
    pub base_sub_domain: String,
    #[serde(serialize_with = "serialize_secret_string")]
    pub client_access_token: SecretString,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceApproval {
    pub base_sub_domain: String,
    pub hostname: String,
    #[serde(serialize_with = "serialize_secret_string")]
    pub service_access_token: SecretString,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppsRequest {
    pub os_arch: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppsResult {
    pub vscode: AppInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppInfo {
    pub os_arch: String,
    pub latest_version: Version,
    pub download_link: String,
}

impl AppInfo {
    pub fn vscode_dir<P: Into<PathBuf>>(&self, apps_dir: P) -> PathBuf {
        let apps_dir = apps_dir.into();

        let version = self.latest_version.to_string();
        let os_arch = self.os_arch.as_str();
        let dir_name = format!("portalbox-vscode-{version}-{os_arch}");

        apps_dir.join(dir_name)
    }

    pub fn vscode_cmd<P: Into<PathBuf>>(&self, apps_dir: P) -> PathBuf {
        let mut dir = self.vscode_dir(apps_dir);
        cfg_if::cfg_if! {
            if #[cfg(target_os = "windows")] {
                dir.push("bin/portalbox-vscode.cmd")
            } else {
                dir.push("bin/portalbox-vscode")
            }
        };
        dir
    }

    pub fn server_data_dir<P: Into<PathBuf>>(&self, apps_data_dir: P) -> PathBuf {
        self.apps_data_subdir(apps_data_dir, "vscode-server-data")
    }

    pub fn user_data_dir<P: Into<PathBuf>>(&self, apps_data_dir: P) -> PathBuf {
        self.apps_data_subdir(apps_data_dir, "vscode-user-data")
    }

    pub fn extensions_dir<P: Into<PathBuf>>(&self, apps_data_dir: P) -> PathBuf {
        self.apps_data_subdir(apps_data_dir, "vscode-extensions")
    }

    pub fn output_file<P: Into<PathBuf>>(&self, apps_data_dir: P) -> PathBuf {
        let apps_data_dir = apps_data_dir.into();
        apps_data_dir.join("vscode.log")
    }

    fn apps_data_subdir<P: Into<PathBuf>>(&self, apps_data_dir: P, subdir: &str) -> PathBuf {
        let apps_data_dir = apps_data_dir.into();
        let ret = apps_data_dir.join(subdir);

        let _ = std::fs::create_dir_all(&ret);

        ret
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientVersionRequest {
    pub current_version: Version,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientVersionResponse {
    pub latest_version: Version,
}

#[cfg(test)]
mod tests {
    use crate::utils::get_os_arch;

    #[test]
    fn get_out_platform_arch() {
        let val = get_os_arch();
        dbg!(val);
    }
}
