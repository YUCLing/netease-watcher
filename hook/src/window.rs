use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        FindWindowW, GetWindow, IsIconic, IsWindowVisible, SetForegroundWindow, SetWindowPos,
        GW_HWNDNEXT, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    },
};

// Shell_TrayWnd\0
const SHELL_TRAYWND: [u16; 14] = [
    0x5300, 0x6800, 0x6500, 0x6c00, 0x6c00, 0x5f00, 0x5400, 0x7200, 0x6100, 0x7900, 0x5700, 0x6e00,
    0x6400, 0x0000,
];

pub fn switch_focus(current: HWND) -> Result<(), u32> {
    unsafe {
        let mut target = GetWindow(current, GW_HWNDNEXT);
        while !target.is_null() {
            if IsWindowVisible(target) != 0 && IsIconic(target) == 0 {
                break;
            }
            target = GetWindow(target, GW_HWNDNEXT);
        }

        if target.is_null() {
            target = FindWindowW(&SHELL_TRAYWND as *const u16, core::ptr::null());
        }

        if !target.is_null() {
            let _ = SetWindowPos(
                target,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE,
            );
            let _ = SetForegroundWindow(target);
        }
    }
    Ok(())
}
