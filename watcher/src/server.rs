use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse, Json,
};
use tokio::sync::watch::Receiver;

use crate::Music;

pub async fn http_handler(
    State(state): State<crate::State>,
) -> impl IntoResponse {
    let crate::State(time_rx, music_rx) = state.clone();
    let current_time = *time_rx.borrow();
    let current_music = music_rx.borrow().clone();
    Json(serde_json::json!({
        "time": current_time,
        "music": current_music
    }))
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<crate::State>,
) -> impl IntoResponse {
    log::info!("New WebSocket connection.");
    let crate::State(time_rx, music_rx) = state.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, time_rx, music_rx))
}

async fn handle_socket(
    mut socket: WebSocket,
    mut current_time: Receiver<f64>,
    mut music: Receiver<Option<Music>>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let tx_clone = tx.clone();
    let current_time_task = tokio::spawn(async move {
        loop {
            if current_time.changed().await.is_err() {
                break;
            }
            if let Err(_err) = tx_clone.send(Message::Text(
                serde_json::json!({
                    "type": "timechange",
                    "value": *current_time.borrow()
                })
                .to_string()
                .into(),
            )) {
                break;
            }
        }
    });

    let music_change_task = tokio::spawn(async move {
        loop {
            if music.changed().await.is_err() {
                break;
            }
            if let Err(_err) = tx.send(Message::Text(
                serde_json::json!({
                    "type": "musicchange",
                    "value": *music.borrow()
                })
                .to_string()
                .into(),
            )) {
                break;
            }
        }
    });

    while let Some(msg) = rx.recv().await {
        if let Err(_err) = socket.send(msg).await {
            rx.close();
            current_time_task.abort();
            music_change_task.abort();
            break;
        }
    }

    log::info!("WebSocket disconnected.");
}
