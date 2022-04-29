use crate::{
    cli::{Cli, Commands},
    client_instance::ClientInstance,
    config::Config,
    credentials::CredManager,
};
use axum::{error_handling::HandleError, extract::Extension, http::StatusCode, Router};
use clap::StructOpt;
use credentials::Credential;
use dotenv::dotenv;
use models::AppsResult;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{
    net::{SocketAddr, ToSocketAddrs},
    time::Duration,
};
use tera::Tera;
use tokio::signal;
use tokio::sync::Mutex;
use tower_http::{services::ServeDir, trace::TraceLayer};

mod api;
mod cli;
mod client_instance;
mod config;
mod credentials;
mod downloader;
mod error;
mod proxy_client;
mod reset;
mod telemetry;
mod version;
mod website;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().ok();
    let args = Cli::parse();
    let config_file = args.config_file;

    let config = match Config::new(config_file) {
        Ok(val) => val,
        Err(e) => {
            return Err(anyhow::anyhow!("Invalid config file {}", e));
        }
    };

    telemetry::init_subscriber(&config);

    config.ensure_all_dirs().await?;

    match args.command {
        Commands::Start => start(config).await,
        Commands::Config => config.show().await,
        Commands::Reset(reset) => {
            let ret = reset::reset(reset, config).await;
            ret
        }
    }
}

async fn start(config: Config) -> Result<(), anyhow::Error> {
    let config_1 = config.clone();

    tracing::info!("Starting...");
    tracing::debug!(?config, runtime_dir = ?config.runtime_dir());

    let apps = match init_apps(&config).await {
        Ok(val) => val,
        Err(e) => {
            tracing::error!(?e, "Error initializing");
            return Err(e.into());
        }
    };

    tracing::debug!(?apps);

    let vscode_full_cmd = apps.vscode.vscode_cmd(&config.apps_dir());
    let vscode_log_file = apps.vscode.output_file(&config.apps_data_dir());

    if !vscode_full_cmd.exists() {
        tracing::error!(?vscode_full_cmd, "Can't find vscode");
        return Err(anyhow::anyhow!("Can't find vscode"));
    }

    let config_2 = config.clone();
    tracing::info!("VSCode starting...");
    let vscode_handle = duct::cmd!(
        vscode_full_cmd,
        "--host",
        "0.0.0.0",
        "--port",
        config.vscode_port.to_string(),
        "--server-data-dir",
        apps.vscode.server_data_dir(&config_2.apps_data_dir()),
        "--user-data-dir",
        apps.vscode.user_data_dir(&config_2.apps_data_dir()),
        "--extensions-dir",
        apps.vscode.extensions_dir(&config_2.apps_data_dir()),
        "--without-connection-token"
    )
    .stderr_to_stdout()
    .stdout_path(vscode_log_file)
    .start()?;

    let serve_dir_service = {
        let wwwroot_dir = if let Ok(runtime_dir) = &config.runtime_dir() {
            runtime_dir.join("wwwroot")
        } else {
            "wwwroot".into()
        };

        ServeDir::new(wwwroot_dir)
    };

    let tera = {
        let templates_dir = if let Ok(runtime_dir) = &config.runtime_dir() {
            runtime_dir.join("website/templates")
        } else {
            "website/templates".into()
        };
        let dir_glob = format!("{}/**/*.html", templates_dir.display());
        Tera::new(&dir_glob).unwrap()
    };
    let (connect_service_request_sender, connect_service_request_receiver) =
        tokio::sync::mpsc::channel(10);

    let env = Environment {
        config: config.clone(),
        tera,
        signed_in_base_sub_domain: Arc::new(Mutex::new(None)),
        connect_service_request_sender,
    };

    let credentials = match CredManager::load(&config).await {
        Ok(val) => {
            tracing::info!("Credentials loaded");
            val
        }
        Err(_e) => {
            tracing::info!("No existing credentials");
            CredManager::empty()
        }
    };

    if let Some(credential) = credentials.credentials.get(config.server_url().as_str()) {
        tracing::info!(server_url = ?config.server_url(), "Signing in...");
        if let Err(e) = website::start_all_service(credential.clone(), &env).await {
            tracing::error!(?e, "Error signing in");
        }
    }

    let app = Router::new()
        .merge(website::routes())
        .nest("/api", api::routes())
        .fallback(HandleError::new(serve_dir_service, handle_serve_dir_error))
        .layer(TraceLayer::new_for_http())
        .layer(Extension(env));

    let addr = SocketAddr::from(([0, 0, 0, 0], config_1.local_home_service_port));
    tracing::info!(?addr, "Listening");
    let server_fut = async move {
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    };

    let proxy_client_fut = async move {
        let server_proxy_url = config_1.server_proxy_url();
        tracing::debug!(?server_proxy_url, "server details");
        let proxy_server: Vec<_> = server_proxy_url
            .to_socket_addrs()
            .expect("Unable to resolve domain")
            .collect();

        let ret = proxy_client::start(proxy_server[0], connect_service_request_receiver).await;
        if let Err(e) = ret {
            tracing::error!(?e, "proxy server error");
        }
    };

    tracing::debug!("Checking for update...");
    let _ = version::check(&config).await;

    tokio::select! {
        _ = server_fut => {
            tracing::info!("server_fut ended");
        }
        _ = proxy_client_fut => {
            tracing::info!("proxy client ended");
        }
        _ = signal::ctrl_c() => {
            tracing::info!("Ctrl-C received, terminating...");
        }
    }

    let vscode_killed = vscode_handle.kill();
    if let Err(e) = vscode_killed {
        tracing::error!(?e, "Failed to kill the vscode process");
    }
    tracing::info!("Terminated");
    Ok(())
}

async fn handle_serve_dir_error(err: std::io::Error) -> (StatusCode, String) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Something went wrong: {}", err),
    )
}

async fn init_apps(config: &Config) -> Result<AppsResult, anyhow::Error> {
    let client_instance = ClientInstance::infer(config).await;

    match client_instance {
        Ok(val) => {
            let current_vscode_version = val.vscode.latest_version.clone();
            let config_1 = config.clone();
            let update_fut = async move {
                let apps_result =
                    fetch_or_update_apps(&config_1, Some(current_vscode_version)).await;
                if let Err(e) = apps_result {
                    tracing::error!(?e, "Error getting apps result");
                }
            };
            tokio::task::spawn(update_fut);

            let ret = AppsResult {
                vscode: val.vscode.clone(),
            };
            Ok(ret)
        }
        Err(e) => {
            tracing::error!(?e, "Error loading client instance");
            let init_apps = match fetch_or_update_apps(&config, None).await {
                Ok(val) => val,
                Err(e) => {
                    tracing::error!(?e, "Can't fetch vscode from server");
                    return Err(anyhow::anyhow!("Can't fetch vscode from server"));
                }
            };

            Ok(init_apps)
        }
    }
}

async fn fetch_or_update_apps(
    config: &Config,
    current_vscode_version: Option<semver::Version>,
) -> Result<AppsResult, anyhow::Error> {
    let os_arch = models::get_os_arch();
    let url = config.server_url_with_path("api/apps");
    tracing::debug!(%url, ?os_arch, "Getting apps");

    let apps_request = models::AppsRequest { os_arch };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let apps_result = client
        .get(url)
        .json(&apps_request)
        .send()
        .await?
        .json::<models::AppsResult>()
        .await?;
    tracing::debug!(?apps_result, "Got app_results");

    if let Some(current_vscode_version) = current_vscode_version {
        if current_vscode_version == apps_result.vscode.latest_version {
            // Already have the latest version
            tracing::info!("Already have latest version");
            return Ok(apps_result);
        }
    }

    let vs_code_full_dir = apps_result.vscode.vscode_dir(&config.apps_dir());
    if vs_code_full_dir.exists() {
        tracing::debug!("Already exists, skip downloading");
        return Ok(apps_result);
    }

    tracing::info!("Downloading vscode");

    let tar_gz_path = {
        let home_dir = config.home_dir.clone();
        home_dir.join("vscode-latest.tar.gz")
    };
    let _ = downloader::download_file(&apps_result.vscode.download_link, &tar_gz_path).await?;

    let path = tar_gz_path;

    let tar_gz = std::fs::File::open(path)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);

    let extracting_msg = format!(
        "Extracing vscode {}",
        apps_result.vscode.latest_version.to_string()
    );
    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.set_style(indicatif::ProgressStyle::default_spinner().tick_strings(&[
        "[    ]", "[=   ]", "[==  ]", "[=== ]", "[ ===]", "[  ==]", "[   =]", "[    ]", "[   =]",
        "[  ==]", "[ ===]", "[=== ]", "[==  ]", "[=   ]", "[====]",
    ]));

    spinner.set_message(extracting_msg);
    spinner.enable_steady_tick(120);
    archive.unpack(&config.apps_dir())?;

    let extracted_msg = format!(
        "Extracted vscode {}",
        apps_result.vscode.latest_version.to_string()
    );
    spinner.finish_with_message(extracted_msg);

    Ok(apps_result)
}

#[derive(Clone)]
pub struct Environment {
    config: Config,
    tera: Tera,
    signed_in_base_sub_domain: Arc<Mutex<Option<Credential>>>,
    connect_service_request_sender: tokio::sync::mpsc::Sender<ConnectServiceRequest>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnectServiceRequest {
    #[serde(serialize_with = "models::serialize_secret_string")]
    pub portalbox_inner_token: SecretString,
    pub hostname: String,
    pub local_service_name: String,
    pub local_service_address: SocketAddr,
}
