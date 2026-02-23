use std::{
    path::Path,
    time::{Duration, Instant},
};

use notify::EventKind;
use rusqlite::Connection;
use tokio::sync::{
    oneshot,
    watch::{self, Receiver, Sender},
};

use crate::{
    netease::{
        create_file_watcher, unix::util::determine_is_64_bit, update_music, FIND_RETRY_SECS,
    },
    Music,
};

mod mem;
mod util;

pub struct NeteaseWatcherUnix {
    pub(super) time: (Sender<f64>, Receiver<f64>),
    pub(super) music: (Sender<Option<Music>>, Receiver<Option<Music>>),
    pub(super) scheduled_find_time: (Sender<Option<Instant>>, Receiver<Option<Instant>>),
    pub(super) watch_thread: Option<(oneshot::Sender<()>, std::thread::JoinHandle<()>)>,
}

impl NeteaseWatcherUnix {
    pub fn new() -> Self {
        let (time_tx, time_rx) = watch::channel(-1.0);
        let (music_tx, music_rx) = watch::channel(None);
        let (scheduled_find_time_tx, scheduled_find_time_rx) = watch::channel(Some(Instant::now()));
        NeteaseWatcherUnix {
            time: (time_tx, time_rx),
            music: (music_tx, music_rx),
            scheduled_find_time: (scheduled_find_time_tx, scheduled_find_time_rx),
            watch_thread: None,
        }
    }

    pub fn start(&mut self) {
        let (stop_signal, mut stop_rx) = oneshot::channel();
        let time_tx = self.time.0.clone();
        let music_tx = self.music.0.clone();
        let scheduled_find_time_tx = self.scheduled_find_time.0.clone();
        scheduled_find_time_tx.send(Some(Instant::now())).unwrap();
        let join_handle = std::thread::spawn(move || 'watcher_loop: loop {
            if stop_rx.try_recv().is_ok() {
                break 'watcher_loop;
            }
            'find_process: {
                let Ok(processes) = procfs::process::all_processes() else {
                    break 'find_process;
                };
                for process in processes {
                    let Ok(process) = process else {
                        continue;
                    };
                    let Ok(cmdline) = process.cmdline() else {
                        continue;
                    };
                    let Some(executable) = cmdline.first() else {
                        continue;
                    };
                    if !executable.ends_with("cloudmusic.exe")
                        || cmdline.iter().any(|x| x.contains("--type"))
                    {
                        continue;
                    }
                    let Ok(maps) = process.maps() else {
                        log::warn!(
                            "Unable to read memory maps of the process {}, skipping.",
                            process.pid
                        );
                        continue;
                    };
                    let mut in_cloudmusic_map = false;
                    let mut is_64_bit = false;
                    'maps: for map in maps {
                        use procfs::process::{MMPermissions, MMapPath};

                        match &map.pathname {
                            MMapPath::Path(p) => {
                                let filename = p.file_name().unwrap_or_default();
                                if filename == "cloudmusic.dll" {
                                    in_cloudmusic_map = true;
                                    if !in_cloudmusic_map {
                                        // header map, we can determine the bitness of the process from it.
                                        if let Err(_) = determine_is_64_bit(process.pid, &map)
                                            .map(|is_64| is_64_bit = is_64)
                                        {
                                            log::warn!("Unable to determine if the process {} is 64-bit, might be unsupported.", process.pid);
                                        }
                                    }
                                } else {
                                    in_cloudmusic_map = false;
                                }
                            }
                            // the following maps of the same module might be anonymous, so we don't set in_cloudmusic_map to false immediately.
                            MMapPath::Anonymous => {}
                            _ => {
                                in_cloudmusic_map = false;
                            }
                        }

                        if !in_cloudmusic_map {
                            continue;
                        }

                        if !map.perms.contains(MMPermissions::EXECUTE) {
                            // instructions must be in an executable map, so skip if it's not.
                            continue;
                        }

                        let Some(addr) =
                            util::find_movsd_instructions(process.pid, &map, is_64_bit)
                        else {
                            continue;
                        };

                        log::info!(
                            "Found Netease Cloud Music process: {} (pid {})",
                            executable,
                            process.pid
                        );

                        scheduled_find_time_tx.send(None).unwrap(); // set None to indicate that we have found the process and won't try to find again until it exits.

                        let Some((pfx, user)) = process.environ().ok().and_then(|x| {
                            use std::ffi::OsStr;

                            let Some(pfx) = x.get(OsStr::new("WINEPREFIX")) else {
                                return None;
                            };

                            let Some(user) = x.get(OsStr::new("USER")) else {
                                return None;
                            };

                            Some((
                                pfx.to_string_lossy().to_string(),
                                user.to_string_lossy().to_string(),
                            ))
                        }) else {
                            continue;
                        };

                        let netease_webdb_file = format!(
                                "{}/drive_c/users/{}/AppData/Local/NetEase/CloudMusic/Library/webdb.dat",
                                pfx, user
                            );

                        let Ok(conn) = Connection::open(&netease_webdb_file) else {
                            log::error!("Failed to open the database file.");
                            continue;
                        };

                        // initial update
                        update_music(&conn, &music_tx);

                        let Ok((_watcher, notify_rx)) =
                            create_file_watcher(Path::new(&netease_webdb_file))
                        else {
                            log::error!("Failed to create file watcher.");
                            continue;
                        };

                        // TODO: how do we setup CBTProc hook from outside of Wine?
                        // run a helper program in the wine to hook?

                        let mut last_val = -1.;
                        loop {
                            if stop_rx.try_recv().is_ok() {
                                break 'watcher_loop;
                            }

                            let val = util::read_double_from_addr(process.pid, addr);
                            if val < 0. {
                                // unable to read properly
                                continue 'maps; // keep trying other maps
                            }
                            if val != last_val {
                                if time_tx.send(val).is_ok() {
                                    last_val = val;
                                }
                            }

                            if let Ok(Ok(e)) = notify_rx.try_recv() {
                                if matches!(e.kind, EventKind::Modify(_)) {
                                    update_music(&conn, &music_tx);
                                }
                            }

                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            }
            scheduled_find_time_tx
                .send(Some(Instant::now() + Duration::from_secs(FIND_RETRY_SECS)))
                .unwrap();
            std::thread::sleep(Duration::from_secs(FIND_RETRY_SECS));
        });
        self.watch_thread = Some((stop_signal, join_handle));
    }
}
