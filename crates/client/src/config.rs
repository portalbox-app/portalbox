use std::path::PathBuf;

use config::{ConfigError, Environment, File};
use serde::Deserialize;
use url::Url;

pub(crate) const PORTALBOX_DIR: &str = ".portalbox";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server_url: Url,
    pub server_proxy_port: u16,
    pub local_home_service_port: u16,
    pub vscode_port: u16,
    // Configurable, default to local data dir/PORTALBOX_DIR
    pub home_dir: PathBuf,
    pub telemetry: bool,
    pub log: String,
}

impl Default for Config {
    fn default() -> Self {
        let default_home_dir = {
            let mut home_dir = dirs::home_dir().unwrap();
            home_dir.push(PORTALBOX_DIR);
            home_dir
        };

        Self {
            server_url: Url::parse("https://www.portalbox.app").unwrap(),
            server_proxy_port: 46637,
            local_home_service_port: 3030,
            vscode_port: 3000,
            home_dir: default_home_dir,
            telemetry: true,
            log: "".into(),
        }
    }
}

impl Config {
    pub fn new(config_file: Option<PathBuf>) -> Result<Self, ConfigError> {
        let config_file = config_file.unwrap_or_else(|| {
            let mut home_dir = dirs::home_dir().unwrap();
            home_dir.push(PORTALBOX_DIR);
            home_dir.push(CONFIG_FILE);
            home_dir
        });

        let file_source = File::from(config_file);

        let ret = ::config::Config::builder()
            .add_source(file_source.required(false))
            .add_source(Environment::with_prefix("PORTALBOX"))
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
        let mut ret = self.home_dir.clone();
        ret.push("apps");
        ret
    }

    pub fn apps_data_dir(&self) -> PathBuf {
        let mut ret = self.home_dir.clone();
        ret.push("apps-data");
        ret
    }

    pub fn credentials_file_path(&self) -> PathBuf {
        let mut ret = self.home_dir.clone();
        ret.push("credentials.toml");
        ret
    }

    pub async fn ensure_all_dirs(&self) -> Result<(), anyhow::Error> {
        let apps_dir = self.apps_dir();
        let apps_data_dir = self.apps_data_dir();

        let _ = tokio::fs::create_dir_all(apps_dir).await?;
        let _ = tokio::fs::create_dir_all(apps_data_dir).await?;

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
