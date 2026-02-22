use std::sync::Arc;

use axum::{routing::get, Router};
use logging::{setup_logger, setup_panic_logger_hook};
use serde::Serialize;
use tokio::sync::watch::{self, Receiver};

use crate::netease::NeteaseWatcher;

mod logging;
mod netease;
#[cfg(windows)]
mod process;
mod server;
#[cfg(feature = "tui")]
mod tui;
mod util;

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

    let host = std::env::var("HOST").unwrap_or("127.0.0.1".to_string());

    let port: i32 = std::env::var("PORT")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(3574);

    let endpoint = format!("{}:{}", host, port);

    let mut watcher = NeteaseWatcher::new();

    watcher.start();

    #[cfg(feature = "tui")]
    {
        let mut time_rx = watcher.time();
        let mut music_rx = watcher.music();
        let mut next_find_time_rx = watcher.next_find_time();
        // tui updater task, notify tui to redraw when music or time changes
        tokio::spawn(async move {
            let mut last_time = *time_rx.borrow();
            loop {
                tokio::select! {
                    _ = time_rx.changed() => {
                        let time = *time_rx.borrow();
                        if time.round() != last_time {
                            // lower the freq of update to avoid too much redraw
                            last_time = time.round();
                            *crate::tui::TUI_MUSIC_TIME.lock().unwrap() = time;
                            crate::tui::TUI_NOTIFY.notify_one();
                        }
                    }
                    _ = music_rx.changed() => {
                        *crate::tui::TUI_MUSIC.lock().unwrap() = music_rx.borrow().clone();
                        crate::tui::TUI_NOTIFY.notify_one();
                    }
                    _ = next_find_time_rx.changed() => {
                        *crate::tui::TUI_NEXT_FIND_TIME.lock().unwrap() = next_find_time_rx.borrow().clone();
                        crate::tui::TUI_NOTIFY.notify_one();
                    }
                }
            }
        });
    }

    {
        let app = Router::new()
            .route("/ws", get(server::ws_handler))
            .fallback(get(server::http_handler))
            .with_state(State(watcher.time(), watcher.music()));

        log::info!("Starting HTTP server at {}", endpoint);
        let listener = tokio::net::TcpListener::bind(&endpoint).await.unwrap();

        #[cfg(feature = "tui")]
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        #[cfg(not(feature = "tui"))]
        axum::serve(listener, app).await.unwrap();
    }

    #[cfg(feature = "tui")]
    tui::run(endpoint).await;

    watcher.stop().await.unwrap();
}
