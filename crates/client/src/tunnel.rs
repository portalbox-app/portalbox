use tokio::net::TcpStream;

use crate::utils::get_tls_connector;

const SSH_TLS_PORT: u16 = 22857;

pub async fn connect(host: &str) -> anyhow::Result<()> {
    let tls_connector = get_tls_connector()?;

    let host_port = format!("{host}-ssh.portalbox.app:{SSH_TLS_PORT}");

    let mut socket_addrs = tokio::net::lookup_host(host_port).await?;
    let first = socket_addrs
        .next()
        .ok_or(anyhow::anyhow!("Failed to resolve ip"))?;

    let tcp_stream = TcpStream::connect(&first).await?;
    let _ = tcp_stream.set_nodelay(true);

    let domain = format!("{host}-ssh.portalbox.app");

    let tls_stream = tls_connector
        .connect(domain.as_str().try_into()?, tcp_stream)
        .await?;

    let (mut read, mut write) = tokio::io::split(tls_stream);

    let mut std_in = tokio::io::stdin();
    let mut std_out = tokio::io::stdout();

    let in_to_write = tokio::io::copy(&mut std_in, &mut write);
    let read_to_out = tokio::io::copy(&mut read, &mut std_out);

    tokio::select! {
        _ = in_to_write => {
            Ok(())
        },
        _ = read_to_out => {
            Ok(())
        }
    }
}
