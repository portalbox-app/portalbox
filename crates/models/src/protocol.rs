use std::str::FromStr;

use num_enum::{IntoPrimitive, TryFromPrimitive};
use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const AUTH_TOKEN_LENGTH: usize = 80;

#[derive(Debug)]
pub struct ProxyConnectionHelloFixed {
    pub version: u16,
    pub inner_auth_token: SecretString,
    pub host_len: u16,
}

#[derive(Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
pub enum ProxyConnectionMessage {
    AuthOk = 0x1111u16,
    AuthFailed = 0x2222u16,
    Ping = 0x3333,
    Pong = 0x4444,
    Data = 0x5555,
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
    const BUF_LEN: usize = 2 + AUTH_TOKEN_LENGTH + 2;

    let mut buf = vec![0u8; BUF_LEN];

    stream.read_exact(&mut buf).await?;

    let version = u16::from_be_bytes(buf[..2].try_into()?);
    let auth_token_bytes = &buf[2..2 + AUTH_TOKEN_LENGTH];
    let auth_token_str = std::str::from_utf8(auth_token_bytes)?;
    let host_len = u16::from_be_bytes(buf[2 + AUTH_TOKEN_LENGTH..].try_into()?);

    if host_len > 1024 {
        return Err(anyhow::anyhow!("Invalid host length"));
    }

    let inner_auth_token = SecretString::from_str(auth_token_str)?;

    let ret = ProxyConnectionHelloFixed {
        version,
        inner_auth_token,
        host_len,
    };

    Ok(ret)
}

pub async fn read_proxy_message<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<ProxyConnectionMessage, anyhow::Error> {
    let mut buf = [0u8; 2];

    stream.read_exact(&mut buf).await?;

    let code = u16::from_be_bytes(buf[..].try_into()?);

    let msg = ProxyConnectionMessage::try_from(code)?;

    Ok(msg)
}

pub async fn write_proxy_message<S: AsyncWrite + Unpin>(
    stream: &mut S,
    message: ProxyConnectionMessage,
) -> Result<(), anyhow::Error> {
    let code: u16 = message.into();

    let code_bytes = code.to_be_bytes();

    stream.write_all(&code_bytes).await?;
    stream.flush().await?;

    Ok(())
}
