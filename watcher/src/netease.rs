use std::{
    any::Any,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

use notify::{Config, Watcher};

#[cfg(windows)]
mod windows;

use rusqlite::Connection;
use serde_json::Value;
use tokio::sync::watch;
#[cfg(windows)]
pub use windows::NeteaseWatcherWindows as NeteaseWatcher;

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub use unix::NeteaseWatcherUnix as NeteaseWatcher;

use crate::Music;

pub const FIND_RETRY_SECS: u64 = 5;

pub(self) fn create_file_watcher(
    file: &Path,
) -> notify::Result<(
    notify::RecommendedWatcher,
    mpsc::Receiver<notify::Result<notify::Event>>,
)> {
    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(tx)?;

    watcher.configure(Config::default().with_poll_interval(Duration::from_secs(1)))?;

    watcher.watch(file, notify::RecursiveMode::NonRecursive)?;

    Ok((watcher, rx))
}

pub(self) fn update_music(conn: &Connection, music: &watch::Sender<Option<Music>>) {
    let json_str: String = {
        let Ok(json_str) = conn.query_row(
            "SELECT jsonStr FROM historyTracks ORDER BY playtime DESC LIMIT 1",
            [],
            |row| row.get(0),
        ) else {
            log::error!("Unable to read the database.");
            return;
        };
        json_str
    };

    let json = serde_json::from_str::<Value>(&json_str);
    let new_val = json.ok().map(|x| {
        let album = x.get("album").unwrap();
        let album_name = album.get("name").unwrap().as_str().unwrap().to_string();
        let thumbnail = album.get("picUrl").unwrap().as_str().unwrap().to_string();
        let artists = x.get("artists").unwrap().as_array().unwrap();
        let mut artists_vec = Vec::with_capacity(artists.len());
        for i in artists {
            artists_vec.push(i.get("name").unwrap().as_str().unwrap().to_string());
        }
        let duration = x.get("duration").unwrap().as_i64().unwrap();
        let name = x.get("name").unwrap().as_str().unwrap().to_string();
        let id = x.get("id").unwrap().as_str().unwrap().parse().unwrap_or(0);
        Music {
            album: album_name,
            aliases: x
                .get("alias")
                .map(|x| {
                    x.as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_str().unwrap().to_string())
                        .collect()
                })
                .and_then(|x: Vec<String>| if x.is_empty() { None } else { Some(x) }),
            thumbnail,
            artists: artists_vec,
            id,
            duration,
            name,
        }
    });
    if new_val != *music.borrow() {
        log::info!(
            "Music changed to {}",
            if let Some(music) = new_val.as_ref() {
                format!(
                    "{}{} - {} ({})",
                    music.name,
                    if music.aliases.is_none() {
                        "".to_string()
                    } else {
                        format!(" [{}]", music.aliases.as_ref().unwrap().join("/"))
                    },
                    music.artists.join(", "),
                    music.id
                )
            } else {
                "*no music*".to_string()
            }
        );
        let _ = music.send(new_val);
    }
}

impl NeteaseWatcher {
    pub async fn stop(&mut self) -> Result<(), Box<dyn Any + Send>> {
        let Some((stop_signal, join_handle)) = self.watch_thread.take() else {
            return Ok(());
        };

        if stop_signal.send(()).is_err() {
            return Ok(());
        }

        join_handle.join()
    }

    pub fn time(&self) -> watch::Receiver<f64> {
        self.time.1.clone()
    }

    pub fn music(&self) -> watch::Receiver<Option<Music>> {
        self.music.1.clone()
    }

    pub fn next_find_time(&self) -> watch::Receiver<Option<Instant>> {
        self.scheduled_find_time.1.clone()
    }
}
