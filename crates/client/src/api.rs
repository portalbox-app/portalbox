use std::io::{Read, Write};

use crate::Environment;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Extension, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use portable_pty::{native_pty_system, CommandBuilder, PtyPair, PtySize};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

const PORTALBOX_TERM_CMD_PREFIX: &str = "__portalbox_term_cmd";

pub fn routes() -> Router {
    Router::new().route("/term-ws", get(handle_term_ws))
}

async fn handle_term_ws(
    Extension(_env): Extension<Environment>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(socket: WebSocket) {
    tracing::debug!("handle_socket");
    // Create a new pty
    let pair = {
        let pty_system = native_pty_system();
        pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap()
    };

    // Spawn a shell into the pty
    let shell = std::env::var("SHELL").unwrap();
    let cmd = CommandBuilder::new(shell);
    let _child = pair.slave.spawn_command(cmd).unwrap();

    let mut pty_reader = pair.master.try_clone_reader().unwrap();
    let pty_writer = pair.master.try_clone_writer().unwrap();

    let (pty_read_sender, pty_read_receiver) = unbounded_channel();

    std::thread::spawn(move || {
        // TODO: end the thread once the ws connection ends
        let mut buffer = [0; 4 * 1024];
        loop {
            let n = match pty_reader.read(&mut buffer) {
                Ok(val) => val,
                Err(e) => {
                    tracing::error!(?e, "Error reading from pty");
                    break;
                }
            };
            if n == 0 {
                break;
            }

            let data = buffer[..n].to_vec();
            let send = pty_read_sender.send(data);
            if let Err(e) = send {
                tracing::error!(?e, "Pty sending error, ending");
            }
        }
        tracing::debug!("pty_read thread ended");
    });

    let (ws_outgoing, ws_incoming) = socket.split();

    let (ws_msg_sender, ws_msg_receiver) = unbounded_channel();

    let (portalbox_cmd_sender, portalbox_cmd_receiver) = unbounded_channel();

    tracing::debug!("handle_socket - split");

    tokio::select! {
        _ = handle_websocket_incoming(
            ws_incoming,
            pty_writer,
            portalbox_cmd_sender,
            ws_msg_sender.clone(),
        ) => {
            tracing::info!("handle_websocket_incoming completed");
        }
        _ = handle_pty_incoming(pty_read_receiver, ws_msg_sender) => {
            tracing::info!("handle_pty_incoming completed");
        }
        _ = handle_ws_msg_send(ws_msg_receiver, ws_outgoing) => {
            tracing::info!("handle_ws_msg_send completed");
        }
        _ = handle_portalbox_cmds(portalbox_cmd_receiver, pair) => {
            tracing::info!("handle_portalbox_cmds completed");
        }
    };

    tracing::debug!("handle_socket - done");
}

async fn handle_websocket_incoming(
    mut incoming: SplitStream<WebSocket>,
    mut pty_writer: Box<dyn Write + Send>,
    portalbox_cmd_sender: UnboundedSender<String>,
    ws_msg_sender: UnboundedSender<Message>,
) -> Result<(), anyhow::Error> {
    while let Some(Ok(msg)) = incoming.next().await {
        match msg {
            Message::Text(text) => {
                if text.starts_with(PORTALBOX_TERM_CMD_PREFIX) {
                    let _ = portalbox_cmd_sender.send(text);
                } else {
                    pty_writer.write_all(text.as_bytes())?;
                }
            }
            Message::Binary(data) => {
                pty_writer.write_all(&data)?;
            }
            Message::Ping(data) => {
                let _ = ws_msg_sender.send(Message::Pong(data));
            }
            Message::Pong(data) => {
                tracing::debug!(?data, "got pong data");
            }
            Message::Close(data) => {
                tracing::debug!(?data, "got close message");
            }
        };
    }

    Ok(())
}

async fn handle_pty_incoming(
    mut pty_read_receiver: UnboundedReceiver<Vec<u8>>,
    ws_msg_sender: UnboundedSender<Message>,
) -> Result<(), anyhow::Error> {
    while let Some(data) = pty_read_receiver.recv().await {
        let msg = Message::Binary(data);
        let _sent = ws_msg_sender.send(msg)?;
    }

    Ok(())
}

async fn handle_ws_msg_send(
    mut ws_msg_receiver: UnboundedReceiver<Message>,
    mut ws_outgoing: SplitSink<WebSocket, Message>,
) -> Result<(), anyhow::Error> {
    while let Some(msg) = ws_msg_receiver.recv().await {
        ws_outgoing.send(msg).await?;
    }

    Ok(())
}

async fn handle_portalbox_cmds(
    mut portalbox_cmd_receiver: UnboundedReceiver<String>,
    pair: PtyPair,
) {
    while let Some(cmd) = portalbox_cmd_receiver.recv().await {
        let cmd = parse_portalbox_cmd(&cmd);

        match cmd {
            Ok(cmd) => {
                tracing::debug!(?cmd, "Got portalbox cmd");
                match cmd {
                    PortalBoxCmd::Resize { cols, rows } => {
                        let ret = pair.master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });

                        if let Err(e) = ret {
                            tracing::error!(?e, "Error resizing terminal");
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(?e, "Failed to process command");
            }
        }
    }
}

fn parse_portalbox_cmd(cmd: &str) -> Result<PortalBoxCmd, anyhow::Error> {
    let size = cmd.trim_start_matches("__portalbox_term_cmd_resize:");

    let mut sizes = size.split('x');
    let cols = sizes
        .next()
        .ok_or(anyhow::anyhow!("Parsing failed - no cols"))?;
    let rows = sizes
        .next()
        .ok_or(anyhow::anyhow!("Parsing failed - no rows"))?;

    let cols = cols.parse::<u16>()?;
    let rows = rows.parse::<u16>()?;

    let ret = PortalBoxCmd::Resize { cols, rows };
    Ok(ret)
}

#[derive(Debug)]
enum PortalBoxCmd {
    Resize { cols: u16, rows: u16 },
}
