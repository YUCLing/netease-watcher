#[cfg(windows)]
use std::ffi::c_void;

use lightningscanner::Scanner;
#[cfg(unix)]
use procfs::process::MemoryMap;
use rusqlite::Connection;
use serde_json::Value;
use tokio::sync::watch::Sender;
#[cfg(windows)]
use windows::Win32::{
    Foundation::HANDLE,
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        Memory::{
            VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ,
            PAGE_PROTECTION_FLAGS,
        },
    },
};

use crate::Music;

// 32-bit uses absolute addressing, while 64-bit uses RIP-relative addressing.
pub const MOVSD_PATTERN_64: &str = "f2 0f 11 3d ?? ?? ?? ?? f2 0f 11 35"; // MOVSD [offset], XMM7 & MOVSD [offset], XMM6
pub const MOVSD_PATTERN_32: &str = "f2 0f 11 0d ?? ?? ?? ?? 68"; // MOVSD [offset], XMM1 & PUSH [offset]

pub fn is_64_bit_dll(dll_header: &[u8]) -> Result<bool, ()> {
    // Check if the DLL is 64-bit by looking at the PE header.
    // The PE header starts with "MZ" (0x4D, 0x5A), followed by a DOS stub, and then the PE header at an offset specified in the DOS header.
    if dll_header.len() < 0x40 {
        return Err(()); // Not a valid PE file
    }

    let pe_offset = u32::from_le_bytes(dll_header[0x3C..0x40].try_into().unwrap()) as usize;
    if pe_offset + 4 > dll_header.len() {
        return Err(()); // Invalid PE offset
    }

    let pe_signature = &dll_header[pe_offset..pe_offset + 4];
    if pe_signature != b"PE\0\0" {
        return Err(()); // Not a valid PE file
    }

    let machine_type =
        u16::from_le_bytes(dll_header[pe_offset + 4..pe_offset + 6].try_into().unwrap());

    Ok(machine_type == 0x8664) // IMAGE_FILE_MACHINE_AMD64
}

pub fn extract_addr_from_instruction(buf: &[u8], relative_addr: usize) -> usize {
    let offset_bytes = &buf[relative_addr + 4..relative_addr + 8];
    let offset = i32::from_le_bytes([
        offset_bytes[0],
        offset_bytes[1],
        offset_bytes[2],
        offset_bytes[3],
    ]) as isize;

    return offset as usize;
}

pub fn update_music(conn: &Connection, music: &Sender<Option<Music>>) {
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

#[cfg(windows)]
pub fn find_movsd_instructions(process: HANDLE, module_base: usize) -> Option<usize> {
    let scanner = Scanner::new(MOVSD_PATTERN_64);

    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    let mut address = module_base;

    unsafe {
        while VirtualQueryEx(
            process,
            Some(address as _),
            &mut mbi,
            std::mem::size_of_val(&mbi),
        ) != 0
        {
            if mbi.State == MEM_COMMIT
                && (mbi.Protect & PAGE_EXECUTE_READ) != PAGE_PROTECTION_FLAGS(0)
            {
                let mut buffer = vec![0u8; mbi.RegionSize];
                let mut bytes_read = 0;
                if ReadProcessMemory(
                    process,
                    address as _,
                    buffer.as_mut_ptr().cast(),
                    mbi.RegionSize,
                    Some(&mut bytes_read),
                )
                .is_ok()
                {
                    let buf_ptr = buffer.as_ptr();
                    let result = scanner.find(None, buf_ptr, bytes_read);
                    let addr = result.get_addr() as usize;
                    if addr != 0 {
                        let relative_addr = addr - buf_ptr as usize; // we are doing scanning on our copy of memory, so get the relative offset instead.
                        let instruction_addr = address + relative_addr;
                        let offset_bytes = &buffer[relative_addr + 4..relative_addr + 8];
                        let offset = i32::from_le_bytes([
                            offset_bytes[0],
                            offset_bytes[1],
                            offset_bytes[2],
                            offset_bytes[3],
                        ]) as isize;

                        let rip = instruction_addr + 8;
                        let target_addr = rip.wrapping_add(offset as usize);

                        return Some(target_addr);
                    }
                }
            }
            address = mbi.BaseAddress as usize + mbi.RegionSize;
        }
    }
    None
}

#[cfg(windows)]
pub fn read_double_from_addr(process: HANDLE, addr: *mut c_void) -> f64 {
    let mut buf: [u8; 8] = [0; 8];
    let ret = unsafe { ReadProcessMemory(process, addr, buf.as_mut_ptr().cast(), 8, None) };
    ret.map(|_| {
        let val = f64::from_le_bytes(buf);
        if val == -1. {
            // the initial value is 1.0, treat it as a success.
            0.
        } else {
            val
        }
    })
    .unwrap_or(-1.)
}
