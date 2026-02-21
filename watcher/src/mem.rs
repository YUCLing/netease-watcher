//! Memory utilities for Unix-like systems (Linux, macOS, etc.).

use std::{
    fs::OpenOptions,
    io::{Read, Seek},
};

use nix::unistd::Pid;

pub fn read_process_memory(pid: i32, addr: usize, len: usize) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "linux")]
    'process_vm_readv: {
        use nix::{
            sys::uio::{process_vm_readv, RemoteIoVec},
            unistd::Pid,
        };

        let mut buf = vec![0u8; len];
        let slice = std::io::IoSliceMut::new(&mut buf);

        let Ok(read_len) = process_vm_readv(
            Pid::from_raw(pid),
            &mut [slice],
            &[RemoteIoVec { base: addr, len }],
        ) else {
            break 'process_vm_readv;
        };

        if read_len == 0 {
            break 'process_vm_readv;
        }

        return Ok(buf);
    }

    'proc_mem: {
        if len < 3 {
            // For small reads, use ptrace to read word by word.
            break 'proc_mem;
        }

        let Ok(buf) = OpenOptions::new()
            .read(true)
            .open(format!("/proc/{}/mem", pid))
            .and_then(|mut file| {
                file.seek(std::io::SeekFrom::Start(addr as u64))?;
                let mut buf = vec![0u8; len];
                file.read_exact(&mut buf)?;
                Ok(buf)
            })
        else {
            break 'proc_mem;
        };

        return Ok(buf);
    }

    let mut left_bytes = len;
    let mut buf = Vec::with_capacity(len);

    while left_bytes > 0 {
        use nix::sys::ptrace::read;

        let word = match read(Pid::from_raw(pid), (addr + (len - left_bytes)) as *mut _) {
            Ok(word) => word,
            Err(_) => break,
        };

        buf.push((word & 0xFF) as u8);
        left_bytes -= 1;
    }

    Err(format!("Unsupported platform"))
}
