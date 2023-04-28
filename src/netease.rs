use std::{sync::Arc, mem::size_of, path::Path, ffi::c_void, fs};

use serde_json::Value;
use windows::{Win32::{System::{ProcessStatus::{EnumProcesses, GetModuleBaseNameW, GetProcessImageFileNameW, EnumProcessModulesEx, LIST_MODULES_ALL}, Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ, WaitForSingleObject, INFINITE}, Diagnostics::Debug::ReadProcessMemory}, Foundation::{HMODULE, CloseHandle, MAX_PATH, GetLastError, HANDLE, WAIT_OBJECT_0}, Storage::FileSystem::{FindFirstChangeNotificationW, FILE_NOTIFY_CHANGE_LAST_WRITE, FindCloseChangeNotification, FindNextChangeNotification}, UI::Shell::{SHGetKnownFolderPath, FOLDERID_LocalAppData, KNOWN_FOLDER_FLAG}}, core::HSTRING};

use tokio::sync::Mutex;

use crate::Music;

pub fn current_time_monitor(current_time: Arc<Mutex<f64>>) {
    tokio::spawn(async move {
        loop {
            unsafe {
                let mut process_ids: Vec<u32> = Vec::with_capacity(8192);
                let mut cb_needed: u32 = 0;

                let ret = EnumProcesses(process_ids.as_mut_ptr(), process_ids.capacity().try_into().unwrap(), &mut cb_needed);
                if ret.into() {
                    let count = cb_needed as usize / size_of::<u32>();
                    process_ids.set_len(count);
                    for i in 0..count {
                        let pid = process_ids.get(i);
                        if let Some(pid) = pid {
                            let proc = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, *pid);
                            if let Ok(proc) = proc {
                                let mut file_name = vec![0; MAX_PATH.try_into().unwrap()];
                                let len = GetProcessImageFileNameW(proc, &mut file_name);

                                if len == 0 {
                                    println!("Failed to find process name for {} ({:?})", pid, GetLastError());
                                } else {
                                    file_name.set_len(len.try_into().unwrap());

                                    let file_name = String::from_utf16_lossy(&file_name);
                                    if let Some(name) = Path::new(&file_name).file_name() {
                                        if name == "cloudmusic.exe" {
                                            let mut process_modules = Vec::with_capacity(4096);
                                            let mut cb_needed: u32 = 0;

                                            let ret = EnumProcessModulesEx(proc, process_modules.as_mut_ptr(), process_modules.capacity().try_into().unwrap(), &mut cb_needed, LIST_MODULES_ALL);
                                            if ret.into() {
                                                let count = cb_needed as usize / size_of::<HMODULE>();
                                                process_modules.set_len(count);
                                                for i in 0..count {
                                                    let hmod = process_modules.get(i).unwrap();

                                                    let mut base_name = vec![0; MAX_PATH.try_into().unwrap()];
                                                    let len = GetModuleBaseNameW(proc, *hmod, &mut base_name);
                                                    if len == 0 {
                                                        println!("Failed to find module name for {} ({:?})", file_name, GetLastError());
                                                    } else {
                                                        let base_name = String::from_utf16_lossy(&base_name).trim().to_lowercase();

                                                        if base_name.find("cloudmusic.dll").is_some() {
                                                            loop {
                                                                let mut buf: [u8; 8] = [0; 8];
                                                                let addr = hmod.0 + 0xA74570;
                                                                let ret = ReadProcessMemory(proc, addr as *mut c_void, buf.as_mut_ptr() as *mut c_void, 8, None);
                                                                if ret.into() {
                                                                    let val = f64::from_le_bytes(buf);
                                                                    let mut num = current_time.lock().await;
                                                                    *num = val;
                                                                } else {
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                CloseHandle(proc);
                            }
                        }
                    }
                }
            }
        }
    });
}

fn update_music(file_path: &str, music: &Arc<Mutex<Option<Music>>>) {
    match fs::read_to_string(file_path) {
        Ok(content) => {
            let json = serde_json::from_str::<Value>(&content);
            let new_val;
            if let Ok(opt) = json.and_then(|x| {
                Ok(x.get(0).and_then(|x| {
                    x.get("track").and_then(|x| {
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
                        let id = x.get("id").unwrap().as_i64().unwrap();
                        Some(Music {
                            album: album_name,
                            thumbnail,
                            artists: artists_vec,
                            id,
                            duration,
                            name
                        })
                    })
                }))
            }) {
                new_val = opt;
            } else {
                new_val = None;
            }
            let mut m = music.blocking_lock();
            if new_val != *m {
                println!("Music changed to {}", if let Some(music) = new_val.clone() {
                    format!("{} - {}", music.name, music.artists.join(", "))
                } else {
                    "*no music*".to_string()
                });
                *m = new_val;
            }
        },
        Err(x) => {
            if x.raw_os_error() != Some(32) {
                eprintln!("Unable to read music history file. ({})", x);
            }
        }
    }
}

pub fn music_monitor(music: Arc<Mutex<Option<Music>>>) {
    let app_data_path;
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KNOWN_FOLDER_FLAG(0), None).expect("Unable to fetch AppData path.");
        app_data_path = path.to_string().expect("Unable to call Windows API.");
    }
    let netease_file_dir = Path::new(&app_data_path).join("NetEase\\CloudMusic\\webdata\\file").to_str().expect("Unable to get path of history file.").to_string();
    let netease_history_file = format!("{}{}", netease_file_dir, "\\history");
    tokio::task::spawn_blocking(move || {
        update_music(&netease_history_file, &music);

        unsafe {
            let handle = FindFirstChangeNotificationW(&HSTRING::from(&netease_file_dir), false, FILE_NOTIFY_CHANGE_LAST_WRITE).expect("Unable to create file change handle.");
            loop {
                let ret = WaitForSingleObject(HANDLE(handle.0), INFINITE);
                match ret {
                    WAIT_OBJECT_0 => {
                        update_music(&netease_history_file, &music);
                        FindNextChangeNotification(handle);
                    },
                    _ => break
                }
            }
            FindCloseChangeNotification(handle);
        }
    });
}