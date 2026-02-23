use std::ffi::c_void;

use lightningscanner::Scanner;
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

use crate::util::{
    extract_addr_from_instruction, is_64_bit_dll, MOVSD_PATTERN_32, MOVSD_PATTERN_64,
};

pub fn find_movsd_instructions(process: HANDLE, module_base: usize) -> Option<usize> {
    let mut scanner = None;

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
            if scanner.is_none() {
                let mut buffer = vec![0u8; mbi.RegionSize];
                let mut bytes_read = 0;
                if ReadProcessMemory(
                    process,
                    address as _,
                    buffer.as_mut_ptr().cast(),
                    mbi.RegionSize,
                    Some(&mut bytes_read),
                )
                .is_err()
                {
                    continue;
                }
                let Ok(is_64_bit) = is_64_bit_dll(&buffer) else {
                    continue;
                };
                scanner = Some(Scanner::new(if is_64_bit {
                    MOVSD_PATTERN_64
                } else {
                    MOVSD_PATTERN_32
                }));
            }
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
                    let result = scanner.as_ref().unwrap().find(None, buf_ptr, bytes_read);
                    let addr = result.get_addr() as usize;
                    if addr != 0 {
                        let relative_addr = addr - buf_ptr as usize; // we are doing scanning on our copy of memory, so get the relative offset instead.
                        let instruction_addr = address + relative_addr;
                        let offset = extract_addr_from_instruction(&buffer, relative_addr);

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
