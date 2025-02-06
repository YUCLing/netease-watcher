use std::{sync::Arc, time::Duration};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use tokio::{
    runtime::Handle,
    sync::{watch::Receiver, Mutex},
};

use crate::Music;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<crate::State>,
) -> impl IntoResponse {
    println!("New websocket connection");
    let time_rx = state.0.clone();
    let music_rx = state.1.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, time_rx, music_rx))
}

async fn handle_socket(
    mut socket: WebSocket,
    mut current_time: Receiver<f64>,
    mut music: Receiver<Option<Music>>,
) {
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
            std::thread::sleep(Duration::from_millis(100));
        }
    });

    let msg_queue_cln = Arc::clone(&msg_queue);
    let mut current_time_task = tokio::task::spawn_blocking(move || loop {
        if Handle::current()
            .block_on(async { current_time.changed().await })
            .is_err()
        {
            break;
        }
        msg_queue_cln.blocking_lock().push(Message::Text(
            serde_json::json!({
                "type": "timechange",
                "value": *current_time.borrow()
            })
            .to_string()
            .into(),
        ));
    });

    let msg_queue_cln = Arc::clone(&msg_queue);
    let mut music_change_task = tokio::task::spawn_blocking(move || loop {
        if Handle::current()
            .block_on(async { music.changed().await })
            .is_err()
        {
            break;
        }
        msg_queue_cln.blocking_lock().push(Message::Text(
            serde_json::json!({
                "type": "musicchange",
                "value": *music.borrow()
            })
            .to_string()
            .into(),
        ));
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
