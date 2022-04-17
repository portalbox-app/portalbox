use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

#[derive(Debug)]
pub struct ProxyConnectionHelloFixed {
    pub version: u16,
    pub inner_auth_token: Uuid,
    pub host_len: u16,
}

pub enum ProxyConnectionAckMessage {
    Ok,
    Failed,
}

pub async fn write_hello_message<S: AsyncWrite + Unpin>(
    inner_auth_token: SecretString,
    host_name: &str,
    stream: &mut S,
) -> Result<(), anyhow::Error> {
    let auth_token = inner_auth_token.expose_secret().as_bytes();

    let version = 1u16;
    let version_bytes = version.to_be_bytes();

    let host_name_bytes = host_name.as_bytes();
    let host_len = host_name_bytes.len() as u16;
    let host_len_bytes = host_len.to_be_bytes();

    // Write hello message
    stream.write_all(&version_bytes).await?;
    stream.write_all(auth_token).await?;
    stream.write_all(&host_len_bytes).await?;

    stream.write_all(host_name_bytes).await?;
    stream.flush().await?;

    Ok(())
}

pub async fn read_fixed_portion_hello_message<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<ProxyConnectionHelloFixed, anyhow::Error> {
    let mut buf = vec![0u8; 20];

    stream.read_exact(&mut buf).await?;

    let version = u16::from_be_bytes(buf[..2].try_into()?);
    let auth_token = uuid::Uuid::from_slice(&buf[2..18])?;
    let host_len = u16::from_be_bytes(buf[18..].try_into()?);

    if host_len > 1024 {
        return Err(anyhow::anyhow!("Invalid host length"));
    }

    let ret = ProxyConnectionHelloFixed {
        version,
        inner_auth_token: auth_token,
        host_len,
    };

    Ok(ret)
}

pub async fn read_ack_message<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<ProxyConnectionAckMessage, anyhow::Error> {
    let mut buf = [0u8; 2];

    stream.read_exact(&mut buf).await?;

    let code = i16::from_be_bytes(buf[..].try_into()?);

    let ret = match code {
        0 => ProxyConnectionAckMessage::Ok,
        val if val < 0 => ProxyConnectionAckMessage::Failed,
        _ => return Err(anyhow::anyhow!("Unknown ack code")),
    };

    Ok(ret)
}

pub async fn write_ack_message<S: AsyncWrite + Unpin>(
    stream: &mut S,
    message: ProxyConnectionAckMessage,
) -> Result<(), anyhow::Error> {
    let code = match message {
        ProxyConnectionAckMessage::Ok => 0i16,
        ProxyConnectionAckMessage::Failed => -1i16,
    };

    let code_bytes = code.to_be_bytes();

    stream.write_all(&code_bytes).await?;
    stream.flush().await?;

    Ok(())
}
