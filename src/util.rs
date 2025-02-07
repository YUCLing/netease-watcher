use std::ffi::c_void;

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

pub fn find_movsd_instructions(process: HANDLE, module_base: usize) -> Vec<usize> {
    let pattern = &[0xF2, 0x0F, 0x11, 0x3D]; // MOVSD [offset], XMM7

    let mut found_addresses = Vec::new();
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
                    buffer.as_mut_ptr() as _,
                    mbi.RegionSize,
                    Some(&mut bytes_read),
                )
                .is_ok()
                {
                    for i in 0..(bytes_read - 8) {
                        if buffer[i..i + 4] == *pattern {
                            let instruction_addr = address + i;
                            let offset_bytes = &buffer[i + 4..i + 8];
                            let offset = i32::from_le_bytes([
                                offset_bytes[0],
                                offset_bytes[1],
                                offset_bytes[2],
                                offset_bytes[3],
                            ]) as isize;

                            let rip = instruction_addr + 8;
                            let target_addr = rip.wrapping_add(offset as usize);

                            found_addresses.push(target_addr);
                        }
                    }
                }
            }
            address = mbi.BaseAddress as usize + mbi.RegionSize;
        }
    }
    found_addresses
}

pub fn read_double_from_addr(process: HANDLE, addr: *mut c_void) -> f64 {
    let mut buf: [u8; 8] = [0; 8];
    let ret = unsafe { ReadProcessMemory(process, addr, buf.as_mut_ptr() as *mut c_void, 8, None) };
    ret.map(|_| f64::from_le_bytes(buf)).unwrap_or(-1.0)
}
