use std::{
    collections::HashMap,
    hash::RandomState,
    process::{Child, Command, Stdio},
};

use lightningscanner::Scanner;
use procfs::process::{MemoryMap, Process};

use crate::{
    netease::unix::mem,
    util::{extract_addr_from_instruction, is_64_bit_dll, MOVSD_PATTERN_32, MOVSD_PATTERN_64},
};

pub fn determine_is_64_bit(pid: i32, map: &MemoryMap) -> Result<bool, ()> {
    let len = (map.address.1 - map.address.0) as usize;

    let Ok(buf) = mem::read_process_memory(pid, map.address.0 as usize, len) else {
        return Err(());
    };

    is_64_bit_dll(&buf)
}

pub fn find_movsd_instructions(pid: i32, map: &MemoryMap, is_64_bit: bool) -> Option<usize> {
    let len = (map.address.1 - map.address.0) as usize;

    let Ok(buf) = mem::read_process_memory(pid, map.address.0 as usize, len) else {
        return None;
    };

    let scanner = Scanner::new(if is_64_bit {
        MOVSD_PATTERN_64
    } else {
        MOVSD_PATTERN_32
    });

    unsafe {
        let buf_ptr = buf.as_ptr();
        let result = scanner.find(None, buf_ptr, len);
        let addr = result.get_addr() as usize;
        if addr != 0 {
            let relative_addr = addr - buf_ptr as usize; // we are doing scanning on our copy of memory, so get the relative offset instead.
            let offset = extract_addr_from_instruction(&buf, relative_addr);

            return Some(offset as usize);
        }
    }

    None
}

pub fn read_double_from_addr(pid: i32, addr: usize) -> f64 {
    let Ok(buf) = mem::read_process_memory(pid, addr, 8) else {
        return -1.;
    };

    let val = f64::from_le_bytes(buf.try_into().unwrap());
    if val == -1. {
        // the initial value is 1.0, treat it as a success.
        0.
    } else {
        val
    }
}

pub fn run_executable_in_same_wine<S: AsRef<str>>(
    proc: &Process,
    executable: S,
) -> Result<Command, String> {
    let Ok(env_vars) = proc.environ().map(|x| {
        HashMap::<String, String, RandomState>::from_iter(x.iter().filter_map(|x| {
            let Some(key) = x.0.to_str() else {
                return None;
            };
            let Some(value) = x.1.to_str() else {
                return None;
            };
            Some((key.to_string(), value.to_string()))
        }))
    }) else {
        return Err("Unable to read environment variables of the process.".to_string());
    };

    let Some(loader) = env_vars.get("WINELOADER").cloned() else {
        return Err("Unable to determine the loader used by the process.".to_string());
    };

    let mut command = Command::new(loader);
    command.arg(executable.as_ref());
    for (key, value) in env_vars {
        command.env(key, value);
    }

    Ok(command)
}
