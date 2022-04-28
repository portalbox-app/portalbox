use std::{net::SocketAddr, sync::Arc, time::Duration};

use backoff::{backoff::Backoff, ExponentialBackoff};
use tokio::{
    io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc::Sender,
};
use tokio_rustls::{client::TlsStream, TlsConnector};
use tokio_util::sync::CancellationToken;

use crate::ConnectServiceRequest;

const MAX_READY_CONNECTIONS: usize = 4;
const REFRESH_CONN_INTERVAL: Duration = Duration::from_secs(60);

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

            tokio::spawn(start_service(service_context));
        }
    };

    let _ = start_service_fut.await;

    Ok(())
}

async fn start_service(context: ServiceContext) -> Result<(), anyhow::Error> {
    tracing::info!(?context.connect_service_request, "Starting service...");

    let connections = concurrent_queue::ConcurrentQueue::bounded(MAX_READY_CONNECTIONS);

    let (new_stream_sender, mut new_stream_receiver) =
        tokio::sync::mpsc::channel::<()>(MAX_READY_CONNECTIONS);
    let new_stream_sender_1 = new_stream_sender.clone();

    let (end_service_sender, mut end_service_receiver) = tokio::sync::mpsc::channel::<()>(1);

    let create_connection_fut = async move {
        while let Some(_) = new_stream_receiver.recv().await {
            let service_context_task = context.clone();
            let new_stream_sender_task = new_stream_sender_1.clone();
            let end_service_sender = end_service_sender.clone();
            let cancellation_token = CancellationToken::new();
            let cancellation_token_1 = cancellation_token.clone();

            let connect_fut = async move {
                let ret = run_proxy_connection(
                    service_context_task,
                    new_stream_sender_task,
                    end_service_sender,
                    cancellation_token,
                )
                .await;
                if let Err(e) = ret {
                    tracing::error!(?e, "connect_proxy error");
                }
            };

            let _handle = tokio::spawn(connect_fut);

            if connections.is_full() {
                let _ = connections.pop();
            }
            let _ = connections.push(cancellation_token_1.drop_guard());
        }
    };

    for _i in 0..MAX_READY_CONNECTIONS {
        let _ = new_stream_sender.send(()).await;
    }

    let refresh_connection_fut = async {
        loop {
            let _ = tokio::time::sleep(REFRESH_CONN_INTERVAL).await;
            let _ = new_stream_sender.send(()).await;
        }
    };

    tokio::select! {
        _ = create_connection_fut => {
            tracing::error!("Create connection future ended unexpectedly");
        }
        _ = refresh_connection_fut => {
            tracing::error!("refres_connection_fut ended unexpectedly");
        }
        _ = end_service_receiver.recv() => {
            tracing::info!("Terminating service...");
        }
    }

    Ok(())
}

// TODO: exponential backoff when auth failed
// After: always kick off a new connection
async fn run_proxy_connection(
    service_context: ServiceContext,
    new_stream_sender: Sender<()>,
    end_service_sender: Sender<()>,
    cancellation_token: CancellationToken,
) -> Result<(), anyhow::Error> {
    tracing::debug!(?service_context.proxy_address, "run_proxy_connection");
    let mut backoff = ExponentialBackoff {
        max_interval: Duration::from_secs(4),
        max_elapsed_time: None,
        ..Default::default()
    };

    // Loop until we have a ready connection
    let mut proxy_stream = loop {
        let ret = get_ready_connection(&service_context, end_service_sender.clone()).await;

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

    let mut init_buf = [0u8; 1024];

    // Wait for either cancellation or data to start flowing
    let read = tokio::select! {
        _ = cancellation_token.cancelled() => {
            // The token was cancelled
            tracing::debug!("connection cancelled");
            return Ok(());
        }
        ret = proxy_stream.read(&mut init_buf) => {
            ret
        }
    };

    // Start/error receiving data:
    // - Signal a new connection
    // - Continue this task to end
    tracing::debug!(?read, "Connection active, creating a new one");
    let _ = new_stream_sender.send(()).await;

    let read = read?;
    if read == 0 {
        return Err(anyhow::anyhow!("Connection closed"));
    }

    let mut local_stream = TcpStream::connect(
        service_context
            .connect_service_request
            .local_service_address,
    )
    .await?;

    let write_buf = &init_buf[..read];

    let _ = local_stream.write(write_buf).await?;

    let _ = copy_bidirectional(&mut proxy_stream, &mut local_stream).await;

    Ok(())
}

async fn get_ready_connection(
    service_context: &ServiceContext,
    end_service_sender: Sender<()>,
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

    let ack_mess = models::protocol::read_ack_message(&mut tls_stream).await?;

    match ack_mess {
        models::protocol::ProxyConnectionAckMessage::Ok => Ok(tls_stream),
        models::protocol::ProxyConnectionAckMessage::Failed => {
            let _ = end_service_sender.send(()).await;
            Err(anyhow::anyhow!("Stream failed auth"))
        }
    }
}
