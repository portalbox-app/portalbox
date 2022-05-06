use std::time::Duration;

use crate::{
    config::Config,
    credentials::{CredManager, Credential, GuestCredential, UserCredential},
    error::ServerError,
    ConnectServiceRequest, Environment,
};
use axum::{
    extract::{Extension, Form, Host},
    response::{Html, Redirect},
    routing::{get, post},
    Router,
};
use cached::{CachedAsync, TimedCache};
use models::{Contact, SignIn, SignInResult, SigninGuestResult};
use pulldown_cmark::{html, Parser};
use secrecy::SecretString;
use serde::Serialize;
use serde_json::json;
use sysinfo::{System, SystemExt};
use tera::Context;
use tokio::{fs::File, io::AsyncReadExt};

const FETCH_SERVER_NEWS_TIMEOUT: Duration = Duration::from_secs(3);

pub fn routes() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/signin", get(handle_signin))
        .route("/signin", post(handle_post_signin))
        .route("/signin-guest", get(handle_signin_guest))
        .route("/signin-guest", post(handle_post_signin_guest))
        .route("/terminal", get(handle_terminal))
        .route("/privacy", get(handle_privacy))
        .route("/terms", get(handle_terms))
        .route("/contact", get(handle_contact))
        .route("/contact", post(handle_post_contact))
        .route("/services/new", get(handle_new_service))
        .route("/services/new", post(handle_post_new_service))
        .route("/about", get(handle_about))
}

async fn handle_index(
    Host(host): Host,
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    tracing::debug!(?host, "handle_index");

    let vscode_url = if host.ends_with("-home.portalbox.app") {
        let sub = host.trim_end_matches("-home.portalbox.app");
        format!("//{sub}-vscode.portalbox.app")
    } else {
        let vscode_port = env.config.vscode_port;
        let host_port = host.rsplit_once(':');

        if let Some((host, _port)) = host_port {
            format!("//{host}:{vscode_port}")
        } else {
            format!("//{host}:{vscode_port}")
        }
    };

    let server_news = fetch_server_news(&env.config).await;

    tracing::debug!(?vscode_url, "handle_index - got vscode_url");

    let vscode = LocalService {
        name: "Visual Studio Code".to_string(),
        url: vscode_url,
        icon_url: "/vscode_icon.png".to_string(),
    };
    let terminal = LocalService {
        name: "Terminal".to_string(),
        url: "/terminal".to_string(),
        icon_url: "/terminal_icon.png".to_string(),
    };
    let services = vec![vscode, terminal];

    let credential = {
        let guard = env.existing_credential.lock().await;
        guard.clone()
    };

    let signed_in_home_url = credential
        .as_ref()
        .map(|val| format!("http://{}-home.portalbox.app", val.base_sub_domain()));

    let render = {
        let mut context = Context::new();
        context.insert("services", &services);
        context.insert("signed_in_home_url", &signed_in_home_url);
        context.insert("credential", &credential);
        context.insert("server_news", &server_news);
        context.insert("active_item", "dashboard");
        env.tera.render("index.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_signin(
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    let credential = {
        let guard = env.existing_credential.lock().await;
        guard.clone()
    };

    if credential.is_some() {
        let render = {
            let mut context = Context::new();
            context.insert("active_item", "signin");
            env.tera.render("signed_in.html", &context)?
        };
        Ok(Html(render))
    } else {
        let render = {
            let mut context = Context::new();
            context.insert("active_item", "signin");
            env.tera.render("signin.html", &context)?
        };
        Ok(Html(render))
    }
}

async fn handle_post_signin(
    Extension(env): Extension<Environment>,
    Form(form): Form<SignIn>,
) -> Result<Redirect, ServerError> {
    tracing::debug!(?form, "handle signin");

    let url = env.config.server_url_with_path("api/signin");

    let client = reqwest::Client::new();

    let res = client
        .post(url)
        .json(&form)
        .send()
        .await?
        .json::<SignInResult>()
        .await?;

    tracing::debug!(?res, "logged in - starting home service");

    let credential = {
        let cred = UserCredential::new(form.email, res.client_access_token, res.base_sub_domain);
        Credential::new_user(cred)
    };

    // Request to create service on the server
    let _ = start_all_service(credential.clone(), &env).await;

    if form.remember_me {
        let mut cred_manager = CredManager::load(&env.config).await.unwrap_or_default();
        cred_manager
            .credentials
            .insert(env.config.server_url().into(), credential);

        let _ = cred_manager.save(&env.config).await;
    }

    Ok(Redirect::to("/"))
}

async fn handle_signin_guest(
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    let render = {
        let mut context = Context::new();
        context.insert("active_item", "signin");
        env.tera.render("signin_guest.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_post_signin_guest(
    Extension(env): Extension<Environment>,
) -> Result<Redirect, ServerError> {
    tracing::debug!("handle_post_signin_guest");

    let url = env.config.server_url_with_path("api/signin-guest");

    let client = reqwest::Client::new();

    let res = client
        .post(url)
        .send()
        .await?
        .json::<SigninGuestResult>()
        .await?;

    let credential = {
        let cred = GuestCredential::new(
            res.base_sub_domain,
            res.client_access_token,
            res.access_code,
        );
        Credential::new_guest(cred)
    };

    // Request to create service on the server
    let _ = start_all_service(credential.clone(), &env).await;

    let mut cred_manager = CredManager::load(&env.config).await.unwrap_or_default();
    cred_manager
        .credentials
        .insert(env.config.server_url().into(), credential);

    let _ = cred_manager.save(&env.config).await;

    Ok(Redirect::to("/"))
}

pub async fn start_all_service(
    credential: Credential,
    env: &Environment,
) -> Result<(), anyhow::Error> {
    let _ = request_and_start_service(
        &env,
        credential.base_sub_domain(),
        credential.client_access_token().clone(),
    )
    .await?;

    let mut cred_guard = env.existing_credential.lock().await;
    *cred_guard = Some(credential);

    Ok(())
}

async fn request_and_start_service(
    env: &Environment,
    base_sub_domain: &str,
    client_access_token: SecretString,
) -> Result<(), anyhow::Error> {
    tracing::debug!(?base_sub_domain, "Requesting service");

    let url = env.config.server_url_with_path("api/services");

    let service_form = models::ServiceRequest {
        base_sub_domain: base_sub_domain.to_string(),
        client_access_token,
    };

    let client = reqwest::Client::new();
    let service = client
        .post(url)
        .json(&service_form)
        .send()
        .await?
        .json::<models::ServiceApproval>()
        .await?;

    tracing::debug!(?service.base_sub_domain, "Service approved");

    let req = ConnectServiceRequest {
        portalbox_inner_token: service.service_access_token,
        base_sub_domain: service.base_sub_domain,
    };

    let _ = env
        .connect_service_request_sender
        .send(req)
        .await
        .map_err(|_e| anyhow::anyhow!("Send error"))?;

    Ok(())
}

async fn handle_terminal(
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    let render = {
        let context = Context::new();
        env.tera.render("terminal.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_terms(Extension(env): Extension<Environment>) -> Result<Html<String>, ServerError> {
    let content = get_markdown_content("terms", env.clone()).await?;

    render_content_page(content, env)
}

async fn handle_contact(
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    let render = {
        let mut context = Context::new();
        context.insert("active_item", "contact");
        env.tera.render("contact.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_post_contact(
    Extension(env): Extension<Environment>,
    Form(form): Form<Contact>,
) -> Result<Html<String>, ServerError> {
    let url = env.config.server_url_with_path("api/contact");
    let client = reqwest::Client::new();
    let response = client.post(url).json(&form).send().await?;

    response.error_for_status()?;

    let render = {
        let context = Context::new();

        env.tera.render("contact_post.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_new_service(
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    let render = {
        let context = Context::new();
        env.tera.render("new_service.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_post_new_service(
    Extension(env): Extension<Environment>,
    Form(_form): Form<Contact>,
) -> Result<Html<String>, ServerError> {
    let render = {
        let context = Context::new();

        env.tera.render("new_service_post.html", &context)?
    };
    Ok(Html(render))
}

#[tracing::instrument(skip(env))]
async fn handle_about(Extension(env): Extension<Environment>) -> Result<Html<String>, ServerError> {
    let version = crate::version::VERSION;
    let system = System::new_all();

    let system_info = SystemInfo::from_system(&system);
    let mem_info = MemInfo::from_system(&system);

    let battery_info = {
        let manager =
            anyhow::Context::context(battery::Manager::new(), "Can't get battery manager")?;
        let mut batteries = anyhow::Context::context(manager.batteries(), "Can't get batteries")?;
        let battery = batteries.next();

        match battery {
            Some(Ok(battery)) => {
                json!(
                {
                    "state": format!("{:?}", battery.state()),
                    "percentage": format!("{:?}", battery.state_of_charge()),
                })
            }
            Some(Err(e)) => {
                json!(
                {
                    "state": format!("Error getting battery state: {:?}", e),
                    "percentage": "unknown",
                })
            }
            None => {
                json!(
                {
                    "state": "No battery detected",
                    "percentage": "unknown",
                })
            }
        }
    };

    let render = {
        let mut context = Context::new();
        context.insert("version", version);
        context.insert("system_info", &system_info);
        context.insert("mem_info", &mem_info);
        context.insert("battery_info", &battery_info);
        context.insert("active_item", "about");

        env.tera.render("about.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_privacy(
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    let content = get_markdown_content("privacy", env.clone()).await?;

    render_content_page(content, env)
}

#[tracing::instrument(skip(_env))]
async fn get_markdown_content(
    md_file: &str,
    _env: Environment,
) -> Result<ContentPage, anyhow::Error> {
    let content_md = {
        let path = format!("templates/markdowns/{}.md", md_file);
        let mut file = File::open(path).await?;

        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;
        contents
    };

    let mut content_html = String::with_capacity(content_md.len() * 3 / 2);
    {
        let parser = Parser::new(&content_md);
        html::push_html(&mut content_html, parser);
    }

    let ret = ContentPage {
        title: md_file.to_ascii_lowercase(),
        content_html,
    };
    Ok(ret)
}

fn render_content_page(
    content_page: ContentPage,
    env: Environment,
) -> Result<Html<String>, ServerError> {
    let render = {
        let mut context = Context::new();
        context.insert("title", &content_page.title);
        context.insert("content_html", &content_page.content_html);

        env.tera.render("content_page.html", &context)?
    };

    Ok(Html(render))
}

pub(crate) async fn fetch_server_news(config: &Config) -> String {
    lazy_static::lazy_static! {
        static ref CACHE: tokio::sync::Mutex<TimedCache<String, String>> = {
            let ret = TimedCache::with_lifespan(60 * 60);
            tokio::sync::Mutex::new(ret)
        };
    }

    let mut cache = CACHE.lock().await;

    let ret = cache
        .get_or_set_with("server_news".into(), || async move {
            let ret = fetch_server_news_impl(&config).await.unwrap_or_default();
            ret
        })
        .await
        .clone();

    ret
}

async fn fetch_server_news_impl(config: &Config) -> anyhow::Result<String> {
    tracing::debug!("fetch_server_news_impl");

    let url = config.server_url_with_path("api/server_news");
    let client = reqwest::Client::new();
    // let response = client.get(url).send().await?;

    let resp = tokio::time::timeout(FETCH_SERVER_NEWS_TIMEOUT, client.get(url).send()).await??;

    let resp = resp.error_for_status()?;
    let ret = resp.text().await?;

    Ok(ret)
}

struct ContentPage {
    title: String,
    content_html: String,
}

#[derive(Debug, Clone, Serialize)]
struct LocalService {
    name: String,
    url: String,
    icon_url: String,
}

#[derive(Debug, Clone, Serialize)]
struct MemInfo {
    total_mem: String,
    used_mem: String,
    free_mem: String,
    total_swap: String,
    used_swap: String,
}

impl MemInfo {
    fn from_system(system: &System) -> Self {
        use byte_unit::{Byte, ByteUnit};

        let total_mem = Byte::from_unit(system.total_memory() as f64, ByteUnit::KB)
            .unwrap()
            .get_appropriate_unit(true)
            .to_string();
        let used_mem = Byte::from_unit(system.used_memory() as f64, ByteUnit::KB)
            .unwrap()
            .get_appropriate_unit(true)
            .to_string();
        let free_mem = Byte::from_unit(system.free_memory() as f64, ByteUnit::KB)
            .unwrap()
            .get_appropriate_unit(true)
            .to_string();
        let total_swap = Byte::from_unit(system.total_swap() as f64, ByteUnit::KB)
            .unwrap()
            .get_appropriate_unit(true)
            .to_string();
        let used_swap = Byte::from_unit(system.used_swap() as f64, ByteUnit::KB)
            .unwrap()
            .get_appropriate_unit(true)
            .to_string();

        Self {
            total_mem,
            used_mem,
            free_mem,
            total_swap,
            used_swap,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct SystemInfo {
    name: String,
    kernel_version: String,
    os_version: String,
    host_name: String,
}

impl SystemInfo {
    fn from_system(system: &System) -> Self {
        let name = system.name().unwrap_or("Unknown".into());
        let kernel_version = system.kernel_version().unwrap_or("Unknown".into());
        let os_version = system.os_version().unwrap_or("Unknown".into());
        let host_name = system.host_name().unwrap_or("Unknown".into());

        Self {
            name,
            kernel_version,
            os_version,
            host_name,
        }
    }
}
