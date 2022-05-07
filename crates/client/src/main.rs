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
use std::{net::SocketAddr, time::Duration};
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
mod tls_client;
mod utils;
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

    if let Some(command) = args.command {
        match command {
            Commands::Start => start(config).await,
            Commands::Tls { host } => tls_client::connect(&host).await,
            Commands::Config => config.show().await,
            Commands::Reset(reset) => {
                let ret = reset::reset(reset, config).await;
                ret
            }
            Commands::Version => {
                let git_sha = &env!("VERGEN_GIT_SHA")[..7];
                println!("portalbox {} ({})", version::VERSION, git_sha);
                Ok(())
            }
        }
    } else {
        start(config).await
    }
}

async fn start(config: Config) -> Result<(), anyhow::Error> {
    let config = Arc::new(config);
    let config_1 = config.clone();
    let config_2 = config.clone();
    let config_3 = config.clone();

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

    tracing::debug!("VSCode starting...");
    let vscode_handle = duct::cmd!(
        vscode_full_cmd,
        "--host",
        "0.0.0.0",
        "--port",
        config.vscode_port.to_string(),
        "--server-data-dir",
        apps.vscode.server_data_dir(&config_1.apps_data_dir()),
        "--user-data-dir",
        apps.vscode.user_data_dir(&config_1.apps_data_dir()),
        "--extensions-dir",
        apps.vscode.extensions_dir(&config_1.apps_data_dir()),
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
    let (proxy_request_sender, proxy_request_receiver) = tokio::sync::mpsc::channel(10);

    let env = Environment {
        config,
        tera,
        existing_credential: Arc::new(Mutex::new(None)),
        proxy_request_sender,
    };

    let credentials = match CredManager::load(&env.config).await {
        Ok(val) => {
            tracing::info!("Credentials loaded... signing in");
            val
        }
        Err(_e) => {
            tracing::info!("No existing credentials");
            CredManager::empty()
        }
    };

    if let Some(credential) = credentials
        .credentials
        .get(env.config.server_url().as_str())
    {
        tracing::debug!(server_url = ?env.config.server_url(), "Signing in...");
        if let Err(e) = website::start_proxy_service(credential.clone(), &env).await {
            tracing::error!(?e, "Error signing in");
        }
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], env.config.local_home_service_port));
    tracing::info!(
        "Dasboard available at http://localhost:{}",
        env.config.local_home_service_port
    );
    let app = Router::new()
        .merge(website::routes())
        .nest("/api", api::routes())
        .fallback(HandleError::new(serve_dir_service, handle_serve_dir_error))
        .layer(TraceLayer::new_for_http())
        .layer(Extension(env));

    let server_fut = async move {
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    };

    let proxy_client_fut = {
        let server_proxy_url = config_1.server_proxy_url();
        tracing::debug!(?server_proxy_url, "proxy_client_fut");
        let mut sock_addrs = tokio::net::lookup_host(server_proxy_url).await?;
        let first = sock_addrs
            .next()
            .ok_or(anyhow::anyhow!("Failed to resolve proxy server"))?;

        async move {
            let ret = proxy_client::start_deamon(config_1, first, proxy_request_receiver).await;
            if let Err(e) = ret {
                tracing::error!(?e, "proxy server error");
            }
        }
    };

    let server_news_fut = async move {
        tracing::debug!("Pre fetch server news");
        let _ = website::fetch_server_news(&config_2).await;
    };

    let version_check_fut = async move {
        tracing::debug!("Checking for update...");
        let _ = version::check(&config_3).await;
    };

    tokio::task::spawn(server_news_fut);
    tokio::task::spawn(version_check_fut);

    tokio::select! {
        _ = server_fut => {
            tracing::debug!("server_fut ended");
        }
        _ = proxy_client_fut => {
            tracing::debug!("proxy client ended");
        }
        _ = signal::ctrl_c() => {
            tracing::debug!("Ctrl-C received, terminating...");
        }
    }

    let vscode_killed = vscode_handle.kill();
    if let Err(e) = vscode_killed {
        tracing::error!(?e, "Failed to kill the vscode process");
    }
    tracing::debug!("Terminated");
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
    let os_arch = models::utils::get_os_arch();
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
    config: Arc<Config>,
    tera: Tera,
    existing_credential: Arc<Mutex<Option<Credential>>>,
    proxy_request_sender: tokio::sync::mpsc::Sender<ProxyRequest>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProxyRequest {
    #[serde(serialize_with = "models::serialize_secret_string")]
    pub portalbox_inner_token: SecretString,
    pub base_sub_domain: String,
    pub hostname: String,
}
