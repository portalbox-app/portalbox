use std::path::Path;

use crate::config::Config;
use models::AppInfo;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ClientInstance {
    pub vscode: AppInfo,
}

impl ClientInstance {
    pub async fn infer(config: &Config) -> Result<Self, anyhow::Error> {
        let mut all_vscodes = all_vscode_installations(config.apps_dir()).await?;

        if all_vscodes.is_empty() {
            return Err(anyhow::anyhow!("No existing vscode installation"));
        }

        all_vscodes.sort_by_key(|val| val.latest_version.clone());
        let latest = all_vscodes.last().unwrap();

        let ret = ClientInstance {
            vscode: latest.to_owned(),
        };

        Ok(ret)
    }
}

async fn all_vscode_installations<P: AsRef<Path>>(
    apps_dir: P,
) -> Result<Vec<AppInfo>, anyhow::Error> {
    let all_vscode_dirs = all_vscode_dirs(apps_dir).await?;
    let all_vscodes = all_vscode_dirs
        .into_iter()
        .filter_map(|val| {
            let vscode = parse_vscode_dir(&val);
            vscode.ok()
        })
        .collect::<Vec<_>>();

    Ok(all_vscodes)
}

fn parse_vscode_dir(dir: &str) -> Result<AppInfo, anyhow::Error> {
    const PREFIX: &str = "portalbox-vscode-";
    if !dir.starts_with(PREFIX) {
        return Err(anyhow::anyhow!("Not a vscode dir"));
    }

    let version_os_arch = dir.trim_start_matches(PREFIX);

    let (version, os_arch) = version_os_arch
        .split_once('-')
        .ok_or(anyhow::anyhow!("Not vscode dir"))?;

    let version = semver::Version::parse(version)?;

    let ret = AppInfo {
        latest_version: version,
        os_arch: os_arch.into(),
        download_link: "".into(),
    };
    Ok(ret)
}

async fn all_vscode_dirs<P: AsRef<Path>>(apps_dir: P) -> Result<Vec<String>, anyhow::Error> {
    let mut all_vscode_dirs = vec![];
    let mut entries = tokio::fs::read_dir(apps_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        if let Ok(file_type) = entry.file_type().await {
            if file_type.is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                if dir_name.starts_with("portalbox-vscode") {
                    all_vscode_dirs.push(dir_name);
                }
            }
        } else {
            continue;
        }
    }

    Ok(all_vscode_dirs)
}
