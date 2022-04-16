use crate::config::Config;
use semver::Version;

pub static VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn check(config: &Config) -> Result<(), anyhow::Error> {
    let current_version = Version::parse(VERSION)?;
    let latest = get_latest_version(current_version.clone(), config).await?;

    if latest > current_version {
        tracing::info!(
            "Update available, current version = {}, latest version = {}",
            VERSION,
            latest
        );
        Ok(())
    } else {
        tracing::info!("Already running the latest version {}", current_version);
        Ok(())
    }
}

async fn get_latest_version(
    current_version: Version,
    config: &Config,
) -> Result<Version, anyhow::Error> {
    let url = config.server_url_with_path("api/client-version");

    let request_form = models::ClientVersionRequest { current_version };

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .json(&request_form)
        .send()
        .await?
        .json::<models::ClientVersionResponse>()
        .await?;

    let latest_version = response.latest_version;

    Ok(latest_version)
}
