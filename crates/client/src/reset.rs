use std::path::Path;

use crate::{cli::Reset, config::Config, credentials::CredManager};

pub async fn reset(reset: Reset, config: Config) -> Result<(), anyhow::Error> {
    tracing::info!(?reset, home_dir = ?config.home_dir, "reseting");

    match reset.command {
        crate::cli::ResetCommands::Credentials => {
            let _ = CredManager::delete(&config).await?;
        }
        crate::cli::ResetCommands::Apps => {
            let _ = clean_apps(&config.apps_dir()).await?;
        }
        crate::cli::ResetCommands::AppsData => {
            let _ = clean_apps_data(&config.apps_data_dir()).await?;
        }
        crate::cli::ResetCommands::All => {
            let _ = clean_apps(&config.apps_dir()).await?;
            let _ = clean_apps_data(&config.apps_data_dir()).await?;
        }
    }

    Ok(())
}

pub async fn clean_apps(apps_dir: &Path) -> Result<(), anyhow::Error> {
    if apps_dir.exists() {
        let _ = tokio::fs::remove_dir_all(apps_dir).await?;
    }

    tracing::info!(?apps_dir, "Apps cleared");
    Ok(())
}

pub async fn clean_apps_data(apps_data_dir: &Path) -> Result<(), anyhow::Error> {
    if apps_data_dir.exists() {
        let _ = tokio::fs::remove_dir_all(apps_data_dir).await?;
    }
    tracing::info!(?apps_data_dir, "Apps data cleared");
    Ok(())
}
