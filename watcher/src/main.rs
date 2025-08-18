use axum::{routing::get, Router};
use logging::{setup_logger, setup_panic_logger_hook};
use serde::Serialize;
use tokio::sync::watch::{self, Receiver};

mod logging;
mod netease;
mod process;
#[cfg(feature = "tui")]
mod tui;
mod util;
mod server;

#[derive(Clone, Serialize, PartialEq, Debug)]
pub struct Music {
    id: i64,
    aliases: Option<Vec<String>>,
    thumbnail: String,
    album: String,
    artists: Vec<String>,
    duration: i64,
    name: String,
}

#[derive(Clone)]
pub struct State(Receiver<f64>, Receiver<Option<Music>>);

#[tokio::main]
async fn main() {
    println!(
        "Netease Cloud Music Status Monitor v{}",
        env!("CARGO_PKG_VERSION")
    );
    println!("by YUCLing");
    println!("= cheers! =");

    setup_logger().unwrap();
    setup_panic_logger_hook();

    let (time_tx, time_rx) = watch::channel(-1.0);
    let (music_tx, music_rx) = watch::channel(None);

    let host = std::env::var("HOST").unwrap_or("127.0.0.1".to_string());

    let port: i32 = std::env::var("PORT")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(3574);

    let endpoint = format!("{}:{}", host, port);

    netease::current_time_monitor(time_tx);
    netease::music_monitor(music_tx);

    {
        let app = Router::new()
            .route("/ws", get(server::ws_handler))
            .fallback(get(server::http_handler))
            .with_state(State(time_rx, music_rx));

        log::info!("Starting HTTP server at {}", endpoint);
        let listener = tokio::net::TcpListener::bind(&endpoint)
            .await
            .unwrap();

        #[cfg(feature = "tui")]
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        #[cfg(not(feature = "tui"))]
        axum::serve(listener, app).await.unwrap();
    }

    #[cfg(feature = "tui")]
    tui::run(endpoint).await;
}
