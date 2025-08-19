use std::{ffi::c_void, mem::size_of, path::Path, time::{Duration, SystemTime}};

use rusqlite::Connection;
use serde_json::Value;
use windows::{
    core::{HSTRING, PCSTR, PCWSTR},
    Win32::{
        Foundation::{HMODULE, MAX_PATH},
        Storage::FileSystem::{
            CreateFileW, ReadDirectoryChangesW, FILE_ACTION_MODIFIED, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
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

use tokio::sync::watch::Sender;

use crate::{process::get_process_thread_ids, util, Music};

pub const FIND_RETRY_SECS: u64 = 5;
pub const HOOK_COOLDOWN: u64 = 3;

pub fn current_time_monitor(current_time: Sender<f64>) {
    std::thread::spawn(move || unsafe {
        loop {
            'find_netease: {
                let mut process_ids = [0; 8192];
                let mut cb_needed: u32 = 0;

                let Ok(_) = EnumProcesses(
                    process_ids.as_mut_ptr(),
                    process_ids.len() as u32,
                    &mut cb_needed,
                ) else {
                    break 'find_netease;
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
                            windows::core::Error::from_win32().message()
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
                            let err = windows::core::Error::from_win32();
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
                        std::mem::drop(file_name);

                        // found addr of the play progress
                        #[cfg(feature = "tui")]
                        {
                            *crate::tui::TUI_FOUND_CM.lock().unwrap() = true;
                        }

                        let mut hook = Vec::new();
                        let mut last_hook_attempt = SystemTime::UNIX_EPOCH;
                        let mut last_val = -1.;
                        loop {
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
                                if !hook.is_empty() ||
                                    last_hook_attempt.elapsed().unwrap().as_secs() < HOOK_COOLDOWN
                                {
                                    break 'hook;
                                }
                                last_hook_attempt = SystemTime::now();
                                let Ok(threads) = get_process_thread_ids(*pid) else {
                                    break 'hook;
                                };
                                let hook_lib_name = HSTRING::from("wndhok.dll");
                                let Ok(lib) = LoadLibraryW(PCWSTR(hook_lib_name.as_ptr())) else {
                                    break 'hook;
                                };
                                let Some(proc) = GetProcAddress(lib, PCSTR(c"CBTProc".as_ptr().cast())) else {
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
                                    log::info!(
                                        "Successfully hooked into Netease Cloud Music."
                                    );
                                }
                            }
                            if val != last_val {
                                #[cfg(feature = "tui")]
                                {
                                    let mut lck = crate::tui::TUI_MUSIC_TIME.lock().unwrap();
                                    if lck.round() != val.round() { // reducing the freq of update
                                        *lck = val;
                                        crate::tui::TUI_NOTIFY.notify_one();
                                    }
                                }
                                if current_time.send(val).is_ok() {
                                    last_val = val;
                                }
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            }
            #[cfg(feature = "tui")]
            {
                *crate::tui::TUI_FOUND_CM.lock().unwrap() = false;
                *crate::tui::TUI_LAST_FIND_TIME.lock().unwrap() = std::time::SystemTime::now();
            }
            #[cfg(not(feature = "tui"))]
            log::info!("Unable to find/open Netease Cloud Music process. Next try in {} secs.", FIND_RETRY_SECS);
            // no netease found, wait
            std::thread::sleep(Duration::from_secs(FIND_RETRY_SECS));
        }
    });
}

fn update_music(conn: &Connection, music: &Sender<Option<Music>>) {
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
        #[cfg(feature = "tui")]
        {
            *crate::tui::TUI_MUSIC.lock().unwrap() = new_val.clone();
            crate::tui::TUI_NOTIFY.notify_one();
        }
        let _ = music.send(new_val);
    }
}

pub fn music_monitor(music: Sender<Option<Music>>) {
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
    let conn = Connection::open(&netease_webdb_file).expect("Failed to open the database.");
    std::thread::spawn(move || {
        update_music(&conn, &music);
        unsafe {
            let dir = CreateFileW(
                PCWSTR(HSTRING::from(&netease_library_dir).as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
            .expect("Unable to obtain library dir.");
            std::mem::drop(netease_library_dir);
            let mut buffer = [0; 1024];
            let mut bytes_returned = 0;

            loop {
                let Ok(_) = ReadDirectoryChangesW(
                    dir,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    false,
                    FILE_NOTIFY_CHANGE_LAST_WRITE,
                    Some(&mut bytes_returned),
                    None,
                    None,
                ) else {
                    continue;
                };

                if bytes_returned == 0 {
                    // buffer is too small, update the music directly.
                    update_music(&conn, &music);
                    continue;
                }

                let mut buffer = &buffer[..];
                loop {
                    let next_entry_offset = buffer[0];
                    let action = buffer[1];
                    if action == FILE_ACTION_MODIFIED.0 {
                        let filename_len = buffer[2] as usize / size_of::<u16>();
                        let filename = String::from_utf16_lossy(std::slice::from_raw_parts(
                            buffer[3..3 + filename_len].as_ptr().cast(),
                            filename_len,
                        ));
                        if filename.starts_with("webdb.dat") {
                            std::thread::sleep(Duration::from_millis(50)); // todo: check necessity
                            update_music(&conn, &music);
                        }
                    }
                    if next_entry_offset == 0 {
                        break;
                    } else {
                        buffer = &buffer[next_entry_offset as usize / size_of::<u32>()..];
                    }
                }
            }
        }
    });
}
