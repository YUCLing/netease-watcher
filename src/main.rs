use axum::{Router, routing::get};
use serde::Serialize;
use tokio::sync::watch::{self, Receiver};

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
pub struct State(Receiver<f64>, Receiver<Option<Music>>);

#[tokio::main]
async fn main() {
    println!("Netease Cloud Music Status Monitor v{}", env!("CARGO_PKG_VERSION"));
    println!("by YUCLing");
    println!("= cheers! =");

    let (time_tx, time_rx) = watch::channel(-1.0);
    let (music_tx, music_rx) = watch::channel(None);

    let mut port: i32 = 3574;
    if let Ok(p) = std::env::var("PORT") {
        if let Ok(p) = p.parse() {
            port = p;
        }
    }

    netease::current_time_monitor(time_tx);
    netease::music_monitor(music_tx);

    {
        let app = Router::new()
            .route("/ws", get(websocket::ws_handler))
            .with_state(State(time_rx, music_rx));

        println!("Starting websocket server at port {}", port);
        axum::Server::bind(&format!("0.0.0.0:{}", port).parse().unwrap())
            .serve(app.into_make_service())
            .await
            .unwrap();
    }
}
