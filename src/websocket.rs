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
    let msg_queue: Arc<Mutex<Vec<Message>>> = Arc::new(Mutex::new(vec![]));

    let msg_queue_cln = Arc::clone(&msg_queue);
    let send_task = tokio::spawn(async move {
        'outer: loop {
            if let Ok(mut queue) = msg_queue_cln.try_lock() {
                while let Some(msg) = queue.pop() {
                    if socket.send(msg).await.is_err() {
                        break 'outer;
                    }
                }
            }
        }
    });

    let msg_queue_cln = Arc::clone(&msg_queue);
    let mut current_time_task = tokio::task::spawn_blocking(move || {
        let mut last_val = -1.0;
        loop {
            let val = current_time.blocking_lock().clone();
            if val != last_val {
                msg_queue_cln.blocking_lock().push(Message::Text(serde_json::json!({
                    "type": "timechange",
                    "value": val
                }).to_string()));
                last_val = val;
            }
        }
    });

    let msg_queue_cln = Arc::clone(&msg_queue);
    let mut music_change_task = tokio::task::spawn_blocking(move || {
        let mut last_val = None;
        loop {
            let val = music.blocking_lock().clone();
            if val != last_val {
                msg_queue_cln.blocking_lock().push(Message::Text(serde_json::json!({
                    "type": "musicchange",
                    "value": val
                }).to_string()));
                last_val = val;
            }
        }
    });

    tokio::select! {
        _ = send_task => {
            current_time_task.abort();
            music_change_task.abort();
        },
        _ = (&mut current_time_task) => {

        },
        _ = (&mut music_change_task) => {

        }
    };
    
    println!("Websocket disconnected.");
}