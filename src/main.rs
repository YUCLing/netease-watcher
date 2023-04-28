use std::sync::Arc;

use axum::{Router, routing::get};
use serde::Serialize;
use tokio::sync::Mutex;

mod netease;
mod websocket;

#[derive(Clone, Serialize, PartialEq, Debug)]
pub struct Music {
    id: i64,
    thumbnail: String,
    album: String,
    artists: Vec<String>,
    duration: i64,
    name: String
}

#[derive(Clone)]
pub struct State(Arc<Mutex<f64>>, Arc<Mutex<Option<Music>>>);

#[tokio::main]
async fn main() {
    let current_time = Arc::new(Mutex::new(-1.0));
    let music: Arc<Mutex<Option<Music>>> = Arc::new(Mutex::new(None));

    {
        let current_time_ref = Arc::clone(&current_time);
        netease::current_time_monitor(current_time_ref);
    }

    {
        let music_ref = Arc::clone(&music);
        netease::music_monitor(music_ref);
    }

    {
        let current_time_ref = Arc::clone(&current_time);
        let music_ref = Arc::clone(&music);
        let app = Router::new()
            .route("/ws", get(websocket::ws_handler))
            .with_state(State(current_time_ref, music_ref));

        axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
            .serve(app.into_make_service())
            .await
            .unwrap();
    }
}
