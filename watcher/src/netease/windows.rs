use std::{
    ffi::c_void,
    path::Path,
    time::{Duration, Instant},
};

use notify::EventKind;
use rusqlite::Connection;
use tokio::sync::{oneshot, watch};
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::{HMODULE, MAX_PATH},
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            ProcessStatus::{
                EnumProcessModulesEx, EnumProcesses, GetModuleBaseNameW, GetProcessImageFileNameW,
                LIST_MODULES_ALL,
            },
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
        UI::{
            Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG},
            WindowsAndMessaging::{SetWindowsHookExW, UnhookWindowsHookEx, WH_CBT},
        },
    },
};

use crate::{
    netease::{
        create_file_watcher, update_music, windows::process::get_process_thread_ids,
        FIND_RETRY_SECS,
    },
    Music,
};

mod process;
mod util;

const HOOK_COOLDOWN: u64 = 3;

pub struct NeteaseWatcherWindows {
    pub(super) time: (watch::Sender<f64>, watch::Receiver<f64>),
    pub(super) music: (watch::Sender<Option<Music>>, watch::Receiver<Option<Music>>),
    pub(super) scheduled_find_time: (
        watch::Sender<Option<Instant>>,
        watch::Receiver<Option<Instant>>,
    ),
    pub(super) watch_thread: Option<(oneshot::Sender<()>, std::thread::JoinHandle<()>)>,
    webdb_file: String,
}

impl NeteaseWatcherWindows {
    pub fn new() -> Self {
        let (time_tx, time_rx) = watch::channel(-1.0);
        let (music_tx, music_rx) = watch::channel(None);
        let (scheduled_find_time_tx, scheduled_find_time_rx) = watch::channel(Some(Instant::now()));
        let netease_library_dir = {
            let app_data_path = unsafe {
                let path = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KNOWN_FOLDER_FLAG(0), None)
                    .expect("Unable to fetch AppData path.");
                path.to_string().expect("Unable to call Windows API.")
            };
            Path::new(&app_data_path)
                .join("NetEase\\CloudMusic\\Library")
                .to_str()
                .expect("Unable to get path of library.")
                .to_string()
        };
        let netease_webdb_file = format!("{}{}", netease_library_dir, "\\webdb.dat");
        NeteaseWatcherWindows {
            time: (time_tx, time_rx),
            music: (music_tx, music_rx),
            scheduled_find_time: (scheduled_find_time_tx, scheduled_find_time_rx),
            watch_thread: None,
            webdb_file: netease_webdb_file,
        }
    }

    pub fn start(&mut self) {
        let (stop_signal, mut stop_rx) = oneshot::channel();
        let time_tx = self.time.0.clone();
        let music_tx = self.music.0.clone();
        let scheduled_find_time_tx = self.scheduled_find_time.0.clone();
        scheduled_find_time_tx.send(Some(Instant::now())).unwrap();
        let netease_webdb_file = self.webdb_file.clone();
        let join_handle = std::thread::spawn(move || 'watcher_loop: loop {
            if stop_rx.try_recv().is_ok() {
                break 'watcher_loop;
            }
            'find_process: {
                unsafe {
                    let mut process_ids = [0; 8192];
                    let mut cb_needed: u32 = 0;

                    let Ok(_) = EnumProcesses(
                        process_ids.as_mut_ptr(),
                        process_ids.len() as u32,
                        &mut cb_needed,
                    ) else {
                        break 'find_process;
                    };

                    let count = cb_needed as usize / size_of::<u32>();
                    'process: for pid in process_ids.iter().take(count) {
                        let Ok(proc) =
                            OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, *pid)
                        else {
                            continue;
                        };
                        let mut file_name = [0; MAX_PATH as usize];
                        let len = GetProcessImageFileNameW(proc, &mut file_name);

                        if len == 0 {
                            log::error!(
                                "Failed to find process name for {} ({})",
                                pid,
                                windows::core::Error::from_thread().message()
                            );
                            continue;
                        }

                        let file_name = String::from_utf16_lossy(&file_name[0..(len as usize)]);
                        if !Path::new(&file_name)
                            .file_name()
                            .unwrap_or_default()
                            .eq_ignore_ascii_case("cloudmusic.exe")
                        {
                            continue;
                        }

                        let mut process_modules = [HMODULE::default(); 512];
                        let mut cb_needed: u32 = 0;

                        let Ok(_) = EnumProcessModulesEx(
                            proc,
                            process_modules.as_mut_ptr(),
                            process_modules.len() as u32,
                            &mut cb_needed,
                            LIST_MODULES_ALL,
                        ) else {
                            continue;
                        };

                        let count = cb_needed as usize / size_of::<HMODULE>();
                        for hmod in process_modules.iter().take(count) {
                            let mut base_name = [0; MAX_PATH as usize];
                            let len = GetModuleBaseNameW(proc, Some(*hmod), &mut base_name);
                            if len == 0 {
                                let err = windows::core::Error::from_thread();
                                if err.code().0 == 6 {
                                    // process exited
                                    continue 'process;
                                }
                                log::error!(
                                    "Failed to find module name for {} ({})",
                                    file_name,
                                    err.message()
                                );
                                continue;
                            }

                            if !String::from_utf16_lossy(&base_name[0..(len as usize)])
                                .eq_ignore_ascii_case("cloudmusic.dll")
                            {
                                continue;
                            }

                            let Some(addr) = util::find_movsd_instructions(proc, hmod.0 as usize)
                            else {
                                continue;
                            };

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

                            std::mem::drop(file_name);

                            scheduled_find_time_tx.send(None).unwrap(); // stop trying to find process for a while, since we have already found it.

                            let mut hook = Vec::new();
                            let mut last_hook_attempt = Instant::now()
                                .checked_sub(Duration::from_secs(HOOK_COOLDOWN))
                                .unwrap();
                            let mut last_val = -1.;
                            loop {
                                if stop_rx.try_recv().is_ok() {
                                    break 'watcher_loop;
                                }

                                let val = util::read_double_from_addr(proc, addr as *mut c_void);
                                if val < 0. {
                                    // unable to read properly
                                    for hook in hook {
                                        let _ = UnhookWindowsHookEx(hook);
                                    }
                                    continue 'process; // keep trying other processes
                                }
                                'hook: {
                                    // optional, improves the detection of music changing
                                    if !hook.is_empty()
                                        || last_hook_attempt.elapsed().as_secs() < HOOK_COOLDOWN
                                    {
                                        break 'hook;
                                    }
                                    last_hook_attempt = Instant::now();
                                    let Ok(threads) = get_process_thread_ids(*pid) else {
                                        break 'hook;
                                    };
                                    let hook_lib_name = HSTRING::from("wndhok.dll");
                                    let Ok(lib) = LoadLibraryW(PCWSTR(hook_lib_name.as_ptr()))
                                    else {
                                        break 'hook;
                                    };
                                    let Some(proc) = GetProcAddress(
                                        lib,
                                        windows::core::PCSTR(c"CBTProc".as_ptr().cast()),
                                    ) else {
                                        break 'hook;
                                    };
                                    let proc = std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "system" fn(i32, windows::Win32::Foundation::WPARAM, windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT>(proc);
                                    for thread in threads {
                                        if let Ok(hhook) = SetWindowsHookExW(
                                            WH_CBT,
                                            Some(proc),
                                            Some(lib.into()),
                                            thread,
                                        ) {
                                            hook.push(hhook);
                                        }
                                    }
                                    if !hook.is_empty() {
                                        log::info!("Successfully hooked into Netease Cloud Music.");
                                    }
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
            }
            // reset states
            time_tx.send(-1.).unwrap();
            music_tx.send(None).unwrap();
            scheduled_find_time_tx
                .send(Some(Instant::now() + Duration::from_secs(FIND_RETRY_SECS)))
                .unwrap();
            std::thread::sleep(Duration::from_secs(FIND_RETRY_SECS));
        });
        self.watch_thread = Some((stop_signal, join_handle));
    }
}
