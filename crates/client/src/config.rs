use std::path::PathBuf;

use config::{ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use url::Url;

pub(crate) const PORTALBOX_DIR: &str = ".portalbox";
const CONFIG_FILE: &str = "config.toml";
const ENV_VAR_PREFIX: &str = "PORTALBOX";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server_url: Url,
    pub server_proxy_port: u16,
    pub local_home_service_port: u16,
    pub vscode_port: u16,
    pub ssh_port: u16,
    pub shell_command: Option<String>,
    // Configurable, default to local data dir/PORTALBOX_DIR
    pub home_dir: PathBuf,
    pub runtime_dir: Option<PathBuf>,
    pub telemetry: bool,
    pub log: String,
}

impl Default for Config {
    fn default() -> Self {
        let default_home_dir = {
            let home_dir = dirs::home_dir().unwrap();
            home_dir.join(PORTALBOX_DIR)
        };

        Self {
            server_url: Url::parse("https://www.portalbox.app").unwrap(),
            server_proxy_port: 46637,
            local_home_service_port: 3030,
            vscode_port: 3000,
            ssh_port: 22,
            shell_command: None,
            home_dir: default_home_dir,
            runtime_dir: None,
            telemetry: true,
            log: "".into(),
        }
    }
}

impl Config {
    pub fn new(config_file: Option<PathBuf>) -> Result<Self, ConfigError> {
        let config_file = config_file.unwrap_or_else(|| {
            let home_dir = dirs::home_dir().unwrap();
            let config_file_relative = format!("{PORTALBOX_DIR}/{CONFIG_FILE}");
            home_dir.join(config_file_relative)
        });

        let file_source = File::from(config_file);

        let ret = ::config::Config::builder()
            .add_source(file_source.required(false))
            .add_source(Environment::with_prefix(ENV_VAR_PREFIX))
            .build()?;

        // You can deserialize (and thus freeze) the entire configuration as
        ret.try_deserialize()
    }

    pub fn server_proxy_url(&self) -> String {
        let host = self.server_url.host().unwrap();
        let port = self.server_proxy_port;

        format!("{host}:{port}")
    }

    pub fn server_url(&self) -> Url {
        self.server_url.clone()
    }

    pub fn server_url_with_path(&self, path: &str) -> Url {
        let mut ret = self.server_url();
        ret.set_path(path);
        ret
    }

    pub fn apps_dir(&self) -> PathBuf {
        let home_dir = self.home_dir.clone();
        home_dir.join("apps")
    }

    pub fn apps_data_dir(&self) -> PathBuf {
        let home_dir = self.home_dir.clone();
        home_dir.join("apps-data")
    }

    pub fn credentials_file_path(&self) -> PathBuf {
        let home_dir = self.home_dir.clone();
        home_dir.join("credentials.toml")
    }

    pub async fn ensure_all_dirs(&self) -> Result<(), anyhow::Error> {
        let apps_dir = self.apps_dir();
        let apps_data_dir = self.apps_data_dir();

        let _ = tokio::fs::create_dir_all(apps_dir).await?;
        let _ = tokio::fs::create_dir_all(apps_data_dir).await?;

        Ok(())
    }

    pub fn runtime_dir(&self) -> Result<PathBuf, anyhow::Error> {
        if let Some(dir) = &self.runtime_dir {
            return Ok(dir.clone());
        }

        if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let dir = PathBuf::from(dir);
            let project_dir = dir.ancestors().nth(2).unwrap();
            return Ok(project_dir.to_path_buf());
        }

        let current_exe = std::env::current_exe()?;
        let ret = current_exe
            .parent()
            .expect("Should have a parent dir")
            .to_path_buf();
        Ok(ret)
    }

    pub async fn show(&self) -> Result<(), anyhow::Error> {
        let toml_format = toml::to_string_pretty(self)?;
        println!("{}", toml_format);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_uris() {
        let config = Config::default();
        assert_eq!(config.server_url().as_str(), "https://www.portalbox.app/");
        assert_eq!(config.server_proxy_url(), "www.portalbox.app:46637");
        assert_eq!(
            config.server_url_with_path("api").as_str(),
            "https://www.portalbox.app/api"
        );

        let config = Config {
            server_url: Url::parse("http://localhost:8080").unwrap(),
            ..Default::default()
        };

        assert_eq!(config.server_url().as_str(), "http://localhost:8080/");
        assert_eq!(config.server_proxy_url(), "localhost:46637");
        assert_eq!(
            config.server_url_with_path("api/services").as_str(),
            "http://localhost:8080/api/services"
        );
    }
}
