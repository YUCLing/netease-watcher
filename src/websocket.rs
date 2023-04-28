use std::sync::Arc;

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse
};
use tokio::sync::Mutex;

use crate::Music;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<crate::State>,
) -> impl IntoResponse {
    println!("New websocket connection");
    ws.on_upgrade(move |socket| handle_socket(socket, state.0, state.1))
}

async fn handle_socket(mut socket: WebSocket, current_time: Arc<Mutex<f64>>, music: Arc<Mutex<Option<Music>>>) {
    let send_task = tokio::spawn(async move {
        let mut last_music = None;
        let mut last_val = -1.0;
        loop {
            let val = current_time.lock().await.clone();
            if val != last_val {
                if socket.send(Message::Text(serde_json::json!({
                    "type": "timechange",
                    "value": val
                }).to_string())).await.is_err() {
                    break;
                }
                last_val = val;
            }

            if let Ok(val) = music.try_lock() {
                let val = val.clone();
                if val != last_music {
                    if socket.send(Message::Text(serde_json::json!({
                        "type": "musicchange",
                        "value": val
                    }).to_string())).await.is_err() {
                        break;
                    }
                    last_music = val;
                }
            }
        }
    });

    let _ = send_task.await;

    println!("Websocket disconnected.");
}