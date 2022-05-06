use std::str::FromStr;

use num_enum::{IntoPrimitive, TryFromPrimitive};
use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const AUTH_TOKEN_LENGTH: usize = 80;

#[derive(Debug)]
pub struct ProxyConnectionHello {
    pub version: u16,
    pub connection_token: SecretString,
}

#[derive(Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
pub enum ProxyConnectionMessage {
    AuthOk = 0x1111u16,
    AuthFailed = 0x2222u16,
    Ping = 0x3333,
    Pong = 0x4444,
    DataHome = 0x5555,
    DataVscode = 0x5556,
    DataSsh = 0x5557,
}

pub async fn read_hello_message<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<ProxyConnectionHello, anyhow::Error> {
    const BUF_LEN: usize = 2 + AUTH_TOKEN_LENGTH;

    let mut buf = vec![0u8; BUF_LEN];

    stream.read_exact(&mut buf).await?;

    let version = u16::from_be_bytes(buf[..2].try_into()?);
    let auth_token_bytes = &buf[2..2 + AUTH_TOKEN_LENGTH];
    let auth_token_str = std::str::from_utf8(auth_token_bytes)?;

    let connection_token = SecretString::from_str(auth_token_str)?;

    let ret = ProxyConnectionHello {
        version,
        connection_token,
    };

    Ok(ret)
}

pub async fn write_hello_message<S: AsyncWrite + Unpin>(
    connection_token: SecretString,
    stream: &mut S,
) -> Result<(), anyhow::Error> {
    let auth_token = connection_token.expose_secret().as_bytes();

    let version = 1u16;
    let version_bytes = version.to_be_bytes();

    // Write hello message
    stream.write_all(&version_bytes).await?;
    stream.write_all(auth_token).await?;

    stream.flush().await?;

    Ok(())
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
