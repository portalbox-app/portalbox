use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use backoff::{backoff::Backoff, ExponentialBackoff};
use models::{consts::MAX_READY_CONNECTIONS, protocol::ProxyConnectionMessage};
use secrecy::SecretString;
use tokio::{io::copy_bidirectional, net::TcpStream, sync::mpsc::Sender};
use tokio_rustls::{client::TlsStream, TlsConnector};
use tokio_util::sync::CancellationToken;

use crate::{config::Config, utils::get_tls_connector, ProxyRequest};

const CONN_PING_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct ProxyContext {
    proxy_address: SocketAddr,
    portalbox_inner_token: SecretString,
    base_sub_domain: String,
    hostname: String,
    tls_connector: Arc<TlsConnector>,
}

pub async fn start_deamon(
    config: Arc<Config>,
    proxy_server: SocketAddr,
    mut connect_service_request_receiver: tokio::sync::mpsc::Receiver<ProxyRequest>,
) -> Result<(), anyhow::Error> {
    let connector = get_tls_connector()?;
    let connector = Arc::new(connector);

    let start_proxy_fut = async move {
        while let Some(req) = connect_service_request_receiver.recv().await {
            let proxy_context = ProxyContext {
                proxy_address: proxy_server.clone(),
                portalbox_inner_token: req.portalbox_inner_token,
                base_sub_domain: req.base_sub_domain,
                hostname: req.hostname,
                tls_connector: connector.clone(),
            };

            tokio::task::spawn(start_proxy(proxy_context, config.clone()));
        }
    };

    let _ = start_proxy_fut.await;

    Ok(())
}

async fn start_proxy(context: ProxyContext, config: Arc<Config>) -> Result<(), anyhow::Error> {
    tracing::info!(?context.base_sub_domain, "Starting proxy...");

    let (new_stream_sender, mut new_stream_receiver) =
        tokio::sync::mpsc::channel::<()>(MAX_READY_CONNECTIONS);
    let new_stream_sender_1 = new_stream_sender.clone();

    let token = CancellationToken::new();
    let token_1 = token.clone();

    let create_connection_fut = async move {
        while let Some(_) = new_stream_receiver.recv().await {
            let proxy_context_task = context.clone();
            let new_stream_sender_task = new_stream_sender_1.clone();
            let token_task = token_1.clone();
            let config = config.clone();

            let connect_fut = async move {
                let ret = run_proxy_connection(
                    proxy_context_task,
                    config,
                    new_stream_sender_task,
                    token_task,
                )
                .await;
                if let Err(e) = ret {
                    tracing::error!(?e, "connect_proxy error");
                }
            };

            let _handle = tokio::task::spawn(connect_fut);
        }
    };

    for _i in 0..MAX_READY_CONNECTIONS {
        let _ = new_stream_sender.send(()).await;
    }
    tokio::select! {
        _ = create_connection_fut => {
            tracing::error!("Create connection future ended unexpectedly");
        }
        _ = token.cancelled() => {
            tracing::debug!("Terminating proxy...");
        }
    }

    tracing::debug!("Proxy ended");

    Ok(())
}

// After: always kick off a new connection
async fn run_proxy_connection(
    proxy_context: ProxyContext,
    config: Arc<Config>,
    new_stream_sender: Sender<()>,
    token: CancellationToken,
) -> Result<(), anyhow::Error> {
    tracing::debug!(?proxy_context.proxy_address, "run_proxy_connection");
    let mut backoff = ExponentialBackoff {
        max_interval: Duration::from_secs(4),
        max_elapsed_time: None,
        ..Default::default()
    };

    // Loop until we have a ready connection
    let mut proxy_stream = loop {
        if token.is_cancelled() {
            return Ok(());
        }

        let ret = get_ready_connection(&proxy_context, token.clone()).await;

        match ret {
            Ok(val) => break val,
            Err(e) => {
                tracing::error!(?e, "Error getting ready connection, trying again");
                if let Some(b) = backoff.next_backoff() {
                    let _ = tokio::time::sleep(b).await;
                }
            }
        }
    };

    let data_type = wailt_till_data(&mut proxy_stream).await;

    // Start/error receiving data:
    // - Signal a new connection
    // - Continue this task to end
    tracing::debug!(?data_type, "Connection active, creating a new one");
    let _ = new_stream_sender.send(()).await;

    // Return if there's any error with waiting for data.
    let data_type = data_type?;

    let dest_port = match data_type {
        ProxyConnectionMessage::DataHome => config.local_home_service_port,
        ProxyConnectionMessage::DataVscode => config.vscode_port,
        ProxyConnectionMessage::DataSsh => 22,
        _ => return Err(anyhow::anyhow!("Invalid data_type")),
    };

    let local_service_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), dest_port);

    let mut local_stream = TcpStream::connect(local_service_address).await?;

    let _ = copy_bidirectional(&mut proxy_stream, &mut local_stream).await;

    Ok(())
}

async fn get_ready_connection(
    proxy_context: &ProxyContext,
    token: CancellationToken,
) -> Result<TlsStream<TcpStream>, anyhow::Error> {
    let tcp_stream = TcpStream::connect(proxy_context.proxy_address).await?;
    let _ = tcp_stream.set_nodelay(true);

    let domain = proxy_context.hostname.as_str().try_into()?;
    let mut tls_stream = proxy_context
        .tls_connector
        .connect(domain, tcp_stream)
        .await?;

    let _ = models::protocol::write_hello_message(
        proxy_context.portalbox_inner_token.clone(),
        &mut tls_stream,
    )
    .await?;

    let ack_mess = models::protocol::read_proxy_message(&mut tls_stream).await?;

    match ack_mess {
        ProxyConnectionMessage::AuthOk => Ok(tls_stream),
        ProxyConnectionMessage::AuthFailed => {
            token.cancel();
            Err(anyhow::anyhow!("Stream failed auth"))
        }
        val @ _ => {
            tracing::error!(?val, "Got unepxtected proxy message");
            Err(anyhow::anyhow!("Unexpected proxy message"))
        }
    }
}

// - Reply to ping message
// - Error out if this task doesn't see any ping message for a pre-defined period
// - Return once got the `data` message
async fn wailt_till_data(
    stream: &mut TlsStream<TcpStream>,
) -> anyhow::Result<ProxyConnectionMessage> {
    let ret = loop {
        let mess = tokio::time::timeout(
            CONN_PING_TIMEOUT,
            models::protocol::read_proxy_message(stream),
        )
        .await??;

        match mess {
            ProxyConnectionMessage::Ping => {
                let _write =
                    models::protocol::write_proxy_message(stream, ProxyConnectionMessage::Pong)
                        .await?;
            }
            val @ (ProxyConnectionMessage::DataHome
            | ProxyConnectionMessage::DataVscode
            | ProxyConnectionMessage::DataSsh) => break val,
            val @ _ => {
                tracing::error!(?val, "Getting unexpected message");
                return Err(anyhow::anyhow!("Unexpected message"));
            }
        }
    };

    Ok(ret)
}
