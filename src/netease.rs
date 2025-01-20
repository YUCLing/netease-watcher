use std::{mem::size_of, path::Path, ffi::c_void, time::Duration};

use serde_json::Value;
use sqlite::OpenFlags;
use windows::{Win32::{System::{ProcessStatus::{EnumProcesses, GetModuleBaseNameW, GetProcessImageFileNameW, EnumProcessModulesEx, LIST_MODULES_ALL}, Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ, WaitForSingleObject, INFINITE}, Diagnostics::Debug::ReadProcessMemory}, Foundation::{HMODULE, CloseHandle, MAX_PATH, GetLastError, HANDLE, WAIT_OBJECT_0}, Storage::FileSystem::{FindFirstChangeNotificationW, FILE_NOTIFY_CHANGE_LAST_WRITE, FindCloseChangeNotification, FindNextChangeNotification}, UI::Shell::{SHGetKnownFolderPath, FOLDERID_LocalAppData, KNOWN_FOLDER_FLAG}}, core::HSTRING};

use tokio::sync::watch::Sender;

use crate::Music;

pub fn current_time_monitor(current_time: Sender<f64>) {
    std::thread::spawn(move || {
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
                                    let file_name = String::from_utf16_lossy(&file_name[0..(len as usize)]);
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
                                                        let base_name = String::from_utf16_lossy(&base_name[0..(len as usize)]).to_lowercase();

                                                        if base_name == "cloudmusic.dll" {
                                                            let mut buf: [u8; 8] = [0; 8];
                                                            // offset of previous version of 2.10.10: 0xA74570
                                                            // offset: 0xA77580
                                                            // offset before 3.0.0: 0xA7A580
                                                            // offset of 3.0.0: 0x19187D8
                                                            // offset 202859: 0x196AE10
                                                            // offset 203271: 0x19ECF30
                                                            // offset 203419: 0x1A01F40
                                                            let addr = hmod.0 + 0x1A01F40;
                                                            let mut last_val = -1.0;
                                                            loop {
                                                                let ret = ReadProcessMemory(proc, addr as *mut c_void, buf.as_mut_ptr() as *mut c_void, 8, None);
                                                                if ret.into() {
                                                                    let val = f64::from_le_bytes(buf);
                                                                    if val != last_val {
                                                                        if current_time.send(val).is_ok() {
                                                                            last_val = val;
                                                                        }
                                                                        std::thread::sleep(Duration::from_millis(100));
                                                                    }
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

            println!("Unable to find/open Netease Cloud Music process");
            // no netease found, wait
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

fn update_music(file_path: &str, music: &Sender<Option<Music>>) {
    let conn = sqlite::Connection::open_with_flags(file_path, OpenFlags::new().with_read_only());
    match conn {
        Ok(conn) => {
            let stmt = conn.prepare("SELECT jsonStr FROM historyTracks ORDER BY playtime DESC LIMIT 1");
            match stmt {
                Ok(mut stmt) => {
                    if let Ok(state) = stmt.next() {
                        match state {
                            sqlite::State::Row => {
                                match stmt.read::<String, _>(0) {
                                    Ok(x) => {
                                        let json = serde_json::from_str::<Value>(&x);
                                        let new_val;
                                        if let Ok(opt) = json.and_then(|x| {
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
                                            Ok(Some(Music {
                                                album: album_name,
                                                thumbnail,
                                                artists: artists_vec,
                                                id,
                                                duration,
                                                name
                                            }))
                                        }) {
                                            new_val = opt;
                                        } else {
                                            new_val = None;
                                        }
                                        if new_val != *music.borrow() {
                                            println!("Music changed to {}", if let Some(music) = new_val.clone() {
                                                format!("{} - {} ({})", music.name, music.artists.join(", "), music.id)
                                            } else {
                                                "*no music*".to_string()
                                            });
                                            let _ = music.send(new_val);
                                        }
                                    },
                                    Err(err) => eprintln!("Unable to read data: {}", err)
                                }
                            },
                            _ => {println!("No music has been played.")}
                        }
                    }
                },
                Err(err) => {
                    if let Some(code) = err.code {
                        if code == 5 { // db is locked
                            std::thread::sleep(Duration::from_millis(100));
                            update_music(file_path, music); // try again
                            return;
                        }
                    }
                    eprintln!("Unable to read from database");
                }
            }
        },
        Err(_) => eprintln!("Unable to open database file")
    }
}

pub fn music_monitor(music: Sender<Option<Music>>) {
    let app_data_path;
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KNOWN_FOLDER_FLAG(0), None).expect("Unable to fetch AppData path.");
        app_data_path = path.to_string().expect("Unable to call Windows API.");
    }
    let netease_library_dir = Path::new(&app_data_path).join("NetEase\\CloudMusic\\Library").to_str().expect("Unable to get path of history file.").to_string();
    let netease_webdb_file = format!("{}{}", netease_library_dir, "\\webdb.dat");
    std::thread::spawn(move || {
        update_music(&netease_webdb_file, &music);

        unsafe {
            let handle = FindFirstChangeNotificationW(&HSTRING::from(&netease_library_dir), false, FILE_NOTIFY_CHANGE_LAST_WRITE).expect("Unable to create file change handle.");
            loop {
                let ret = WaitForSingleObject(HANDLE(handle.0), INFINITE);
                match ret {
                    WAIT_OBJECT_0 => {
                        std::thread::sleep(Duration::from_millis(500)); // wait to prevent the lock from cloudmusic
                        update_music(&netease_webdb_file, &music);
                        FindNextChangeNotification(handle);
                    },
                    _ => break
                }
            }
            FindCloseChangeNotification(handle);
        }
    });
}