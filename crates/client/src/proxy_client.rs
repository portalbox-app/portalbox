use std::{net::SocketAddr, sync::Arc, time::Duration};

use backoff::{backoff::Backoff, ExponentialBackoff};
use models::{consts::MAX_READY_CONNECTIONS, protocol::ProxyConnectionMessage};
use tokio::{io::copy_bidirectional, net::TcpStream, sync::mpsc::Sender};
use tokio_rustls::{client::TlsStream, TlsConnector};
use tokio_util::sync::CancellationToken;

use crate::ConnectServiceRequest;

const CONN_PING_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct ServiceContext {
    proxy_address: SocketAddr,
    connect_service_request: ConnectServiceRequest,
    tls_connector: Arc<TlsConnector>,
}

pub async fn start(
    proxy_server: SocketAddr,
    mut connect_service_request_receiver: tokio::sync::mpsc::Receiver<ConnectServiceRequest>,
) -> Result<(), anyhow::Error> {
    let start_service_fut = async move {
        let mut root_cert_store = tokio_rustls::rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs().expect("could not load platform certs")
        {
            root_cert_store
                .add(&tokio_rustls::rustls::Certificate(cert.0))
                .unwrap();
        }

        let config = tokio_rustls::rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let connector = Arc::new(connector);

        while let Some(req) = connect_service_request_receiver.recv().await {
            let service_context = ServiceContext {
                proxy_address: proxy_server.clone(),
                connect_service_request: req,
                tls_connector: connector.clone(),
            };

            tokio::task::spawn(start_service(service_context));
        }
    };

    let _ = start_service_fut.await;

    Ok(())
}

async fn start_service(context: ServiceContext) -> Result<(), anyhow::Error> {
    tracing::info!(?context.connect_service_request, "Starting service...");

    let (new_stream_sender, mut new_stream_receiver) =
        tokio::sync::mpsc::channel::<()>(MAX_READY_CONNECTIONS);
    let new_stream_sender_1 = new_stream_sender.clone();

    let token = CancellationToken::new();
    let token_1 = token.clone();

    let create_connection_fut = async move {
        while let Some(_) = new_stream_receiver.recv().await {
            let service_context_task = context.clone();
            let new_stream_sender_task = new_stream_sender_1.clone();
            let token_task = token_1.clone();

            let connect_fut = async move {
                let ret =
                    run_proxy_connection(service_context_task, new_stream_sender_task, token_task)
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
            tracing::debug!("Terminating service...");
        }
    }

    tracing::debug!("Service ended");

    Ok(())
}

// After: always kick off a new connection
async fn run_proxy_connection(
    service_context: ServiceContext,
    new_stream_sender: Sender<()>,
    token: CancellationToken,
) -> Result<(), anyhow::Error> {
    tracing::debug!(?service_context.proxy_address, "run_proxy_connection");
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

        let ret = get_ready_connection(&service_context, token.clone()).await;

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

    let data_flowing = wailt_till_data(&mut proxy_stream).await;

    // Start/error receiving data:
    // - Signal a new connection
    // - Continue this task to end
    tracing::debug!(?data_flowing, "Connection active, creating a new one");
    let _ = new_stream_sender.send(()).await;

    // Return if there's any error with waiting for data.
    let _data_flowing = data_flowing?;

    let mut local_stream = TcpStream::connect(
        service_context
            .connect_service_request
            .local_service_address,
    )
    .await?;

    let _ = copy_bidirectional(&mut proxy_stream, &mut local_stream).await;

    Ok(())
}

async fn get_ready_connection(
    service_context: &ServiceContext,
    token: CancellationToken,
) -> Result<TlsStream<TcpStream>, anyhow::Error> {
    let tcp_stream = TcpStream::connect(service_context.proxy_address).await?;
    let _ = tcp_stream.set_nodelay(true);

    let domain = service_context.connect_service_request.hostname.as_str();

    let mut tls_stream = service_context
        .tls_connector
        .connect(domain.try_into()?, tcp_stream)
        .await?;

    let _ = models::protocol::write_hello_message(
        service_context
            .connect_service_request
            .portalbox_inner_token
            .clone(),
        &service_context.connect_service_request.hostname,
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
async fn wailt_till_data(stream: &mut TlsStream<TcpStream>) -> anyhow::Result<()> {
    loop {
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
            ProxyConnectionMessage::Data => break,
            val @ _ => {
                tracing::error!(?val, "Getting unexpected message");
                return Err(anyhow::anyhow!("Unexpected message"));
            }
        }
    }

    Ok(())
}
