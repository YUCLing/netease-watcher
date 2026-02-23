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
