#[cfg(windows)]
use std::ffi::c_void;

use lightningscanner::Scanner;
#[cfg(unix)]
use procfs::process::MemoryMap;
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

// TODO: support both 32-bit and 64-bit processes for Windows and Linux? Currently only 64-bit on Windows and 32-bit on Linux is supported.
#[allow(dead_code)]
const MOVSD_PATTERN_64: &str = "f2 0f 11 3d ?? ?? ?? ?? f2 0f 11 35"; // MOVSD [offset], XMM7 & MOVSD [offset], XMM6
#[allow(dead_code)]
const MOVSD_PATTERN_32: &str = "f2 0f 11 0d ?? ?? ?? ?? 68"; // MOVSD [offset], XMM1 & PUSH [offset]

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

#[cfg(unix)]
/// DIFFERS FROM WINDOWS:
/// This returns the address used by instruction directly. This is already the final address for Linux.
pub fn find_movsd_instructions(pid: i32, map: &MemoryMap) -> Option<usize> {
    use crate::mem;

    let len = (map.address.1 - map.address.0) as usize;

    let Ok(buf) = mem::read_process_memory(pid, map.address.0 as usize, len) else {
        return None;
    };

    let scanner = Scanner::new(MOVSD_PATTERN_32);

    unsafe {
        let buf_ptr = buf.as_ptr();
        let result = scanner.find(None, buf_ptr, len);
        let addr = result.get_addr() as usize;
        if addr != 0 {
            let relative_addr = addr - buf_ptr as usize; // we are doing scanning on our copy of memory, so get the relative offset instead.
            let offset_bytes = &buf[relative_addr + 4..relative_addr + 8];
            let offset = i32::from_le_bytes([
                offset_bytes[0],
                offset_bytes[1],
                offset_bytes[2],
                offset_bytes[3],
            ]) as isize;

            return Some(offset as usize);
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

#[cfg(unix)]
pub fn read_double_from_addr(pid: i32, addr: usize) -> f64 {
    use crate::mem;

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
