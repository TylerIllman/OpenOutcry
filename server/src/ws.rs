//! The WebSocket endpoint.
//!
//! One task per connection. Each connection subscribes to the session's
//! broadcast channel for shared events, and holds a private unbounded channel
//! for things addressed only to it — snapshots and rejections. Those are the two
//! kinds of traffic in the protocol, and keeping them separate is what stops a
//! rejection leaking to the whole room.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::protocol::{ClientCommand, ServerEvent};
use crate::state::{AppState, Envelope, Msg, Who};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsQuery {
    pub code: String,
    pub host_token: Option<String>,
    pub player_token: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    State(app): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, q, app))
}

async fn handle_socket(socket: WebSocket, q: WsQuery, app: AppState) {
    let Some(handle) = app.get(&q.code).await else {
        close_with(socket, "no such session").await;
        return;
    };

    // Identify the connection. A host token beats a player token; anything else
    // is refused rather than silently downgraded to a spectator.
    let who = if q.host_token.as_deref() == Some(handle.host_token.as_str()) {
        Who::Host
    } else if let Some(token) = q.player_token.clone() {
        match handle.resolve(token).await {
            Some(p) => Who::Player { id: p.id, name: p.name },
            None => {
                close_with(socket, "unknown player token").await;
                return;
            }
        }
    } else {
        close_with(socket, "missing token").await;
        return;
    };

    handle.touch();
    let (mut sink, mut stream) = socket.split();
    let mut broadcast_rx = handle.events.subscribe();
    let (priv_tx, mut priv_rx) = mpsc::unbounded_channel::<ServerEvent>();

    // Every connection opens with a snapshot, so a late joiner and a reconnect
    // take exactly the same path.
    let _ = handle
        .tx
        .send(Msg::Cmd(Envelope {
            who: who.clone(),
            cmd: ClientCommand::Resync { from_seq: 0 },
            reply: priv_tx.clone(),
        }))
        .await;

    loop {
        tokio::select! {
            Some(ev) = priv_rx.recv() => {
                if send(&mut sink, &ev).await.is_err() { break; }
            }
            ev = broadcast_rx.recv() => {
                match ev {
                    Ok(ev) => { if send(&mut sink, &ev).await.is_err() { break; } }
                    // Lagged means this connection fell behind the broadcast
                    // buffer. The client will spot the seq gap and resync.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                let Message::Text(text) = msg else { continue };
                match serde_json::from_str::<ClientCommand>(&text) {
                    Ok(cmd) => {
                        handle.touch();
                        let _ = handle.tx.send(Msg::Cmd(Envelope {
                            who: who.clone(),
                            cmd,
                            reply: priv_tx.clone(),
                        })).await;
                    }
                    Err(e) => {
                        let _ = priv_tx.send(ServerEvent::Error {
                            message: format!("bad command: {e}"),
                        });
                    }
                }
            }
            else => break,
        }
    }
}

async fn send<S>(sink: &mut S, ev: &ServerEvent) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    let json = serde_json::to_string(ev).map_err(|_| ())?;
    sink.send(Message::Text(json.into())).await.map_err(|_| ())
}

async fn close_with(mut socket: WebSocket, reason: &str) {
    let ev = ServerEvent::Error { message: reason.to_string() };
    if let Ok(json) = serde_json::to_string(&ev) {
        let _ = socket.send(Message::Text(json.into())).await;
    }
    let _ = socket.close().await;
}
