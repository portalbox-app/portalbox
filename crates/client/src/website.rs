use std::net::SocketAddr;

use crate::{
    credentials::{CredManager, Credential},
    error::ServerError,
    ConnectServiceRequest, Environment,
};
use axum::{
    extract::{Extension, Form, Host},
    response::{Html, Redirect},
    routing::{get, post},
    Router,
};
use models::{Contact, SignIn, SignInResult};
use pulldown_cmark::{html, Parser};
use secrecy::SecretString;
use serde::Serialize;
use sysinfo::{System, SystemExt};
use tera::Context;
use tokio::{fs::File, io::AsyncReadExt};

pub fn routes() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/signin", get(handle_signin))
        .route("/signin", post(handle_post_signin))
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

    let signed_in_home_url = {
        let guard = env.signed_in_base_hostname.lock().await;
        if let Some(base_hostname) = &*guard {
            Some(format!("http://{}-home.portalbox.app", base_hostname))
        } else {
            None
        }
    };
    let render = {
        let mut context = Context::new();
        context.insert("services", &services);
        context.insert("signed_in_home_url", &signed_in_home_url);
        env.tera.render("index.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_signin(
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    let render = {
        let context = Context::new();
        env.tera.render("signin.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_post_signin(
    Extension(env): Extension<Environment>,
    Form(form): Form<SignIn>,
) -> Result<Redirect, ServerError> {
    tracing::info!(?form, "handle signin");

    let url = env.config.server_url_with_path("api/signin");

    let client = reqwest::Client::new();

    let res = client
        .post(url)
        .json(&form)
        .send()
        .await?
        .json::<SignInResult>()
        .await?;

    tracing::info!(?res, "logged in - starting home service");

    let credential = Credential::new(form.email, res.client_access_token, res.base_hostname);

    // Request to create service on the server
    let _ = start_all_service(&credential, &env).await;

    if form.remember_me {
        let mut cred_manager = CredManager::load(&env.config).await.unwrap_or_default();
        cred_manager
            .credentials
            .insert(env.config.server_url().into(), credential);

        let _ = cred_manager.save(&env.config).await;
    }

    Ok(Redirect::to("/"))
}

pub async fn start_all_service(
    credential: &Credential,
    env: &Environment,
) -> Result<(), anyhow::Error> {
    let _home = request_and_start_service(
        &env,
        &credential.base_hostname,
        "home",
        credential.client_access_token.clone(),
        ([127, 0, 0, 1], env.config.local_home_service_port).into(),
    )
    .await?;

    let _vscode = request_and_start_service(
        &env,
        &credential.base_hostname,
        "vscode",
        credential.client_access_token.clone(),
        ([127, 0, 0, 1], env.config.vscode_port).into(),
    )
    .await?;

    let mut signed_in_base_hostname = env.signed_in_base_hostname.lock().await;
    *signed_in_base_hostname = Some(credential.base_hostname.clone());

    Ok(())
}

async fn request_and_start_service(
    env: &Environment,
    base_hostname: &str,
    service_name: &str,
    client_access_token: SecretString,
    local_service_address: SocketAddr,
) -> Result<(), anyhow::Error> {
    tracing::info!(?base_hostname, ?service_name, "Requesting service");

    let url = env.config.server_url_with_path("api/services");

    let service_form = models::ServiceRequest {
        base_hostname: base_hostname.to_string(),
        service_name: service_name.to_string(),
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

    tracing::info!(?service.hostname, "Service approved");

    let req = ConnectServiceRequest {
        portalbox_inner_token: service.service_access_token,
        hostname: service.hostname,
        local_service_name: service.service_name,
        local_service_address,
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
        let context = Context::new();
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

    let render = {
        let mut context = Context::new();
        context.insert("version", version);
        context.insert("system_info", &system_info);
        context.insert("mem_info", &mem_info);

        env.tera.render("about.html", &context)?
    };
    Ok(Html(render))
}

async fn handle_privacy(
    Extension(env): Extension<Environment>,
) -> Result<Html<String>, ServerError> {
    tracing::info!("serving privacy page");
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
