//! WebSocket handler 相关实现

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::StreamExt;
use futures_util::SinkExt;
use std::sync::Arc;
use tokio::sync::broadcast;
use futures_util::stream::SplitSink;
use tracing::{info, warn};

pub type Broadcaster = broadcast::Sender<String>;

/// WebSocket 连接入口
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(broadcaster): State<Arc<Broadcaster>>,
) -> impl IntoResponse {
    info!("WebSocket connection request received");
    ws.on_upgrade(move |socket| handle_socket(socket, broadcaster))
}

pub async fn handle_socket(socket: WebSocket, broadcaster: Arc<Broadcaster>) {
    info!("WebSocket connection established");
    let (mut ws_sender, mut ws_receiver): (SplitSink<WebSocket, Message>, _) = socket.split();
    let mut rx = broadcaster.subscribe();

    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(key) => {
                    if ws_sender.send(Message::Text(key.into())).await.is_err() {
                        warn!("WebSocket send failed, client disconnected?");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket receiver lagged behind by {} messages.", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    // The broadcaster has been dropped, no more messages to receive
                    break;
                }
            }
        }
        info!("WebSocket send task finished.");
    });

    let recv_task = tokio::spawn(async move {
        while let Some(msg_result) = ws_receiver.next().await {
            match msg_result {
                Ok(msg) => match msg {
                    Message::Text(t) => info!("Received text from WebSocket client: {}", t),
                    Message::Binary(_) => info!("Received binary data from WebSocket client."),
                    Message::Ping(_) => info!("Received WebSocket ping."),
                    Message::Pong(_) => info!("Received WebSocket pong."),
                    Message::Close(_) => {
                        info!("WebSocket client initiated close.");
                        break;
                    }
                },
                Err(e) => {
                    warn!("Error receiving WebSocket message: {}", e);
                    break;
                }
            }
        }
        info!("WebSocket receive task finished.");
    });

    tokio::select! {
        _ = send_task => { /* Send task finished */ }
        _ = recv_task => { /* Receive task finished */ }
    }
    info!("WebSocket connection closed.");
}
