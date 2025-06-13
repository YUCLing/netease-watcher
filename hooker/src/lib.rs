#![no_std]

use core::ffi::c_void;

use lazy_static::lazy_static;
use windows_sys::Win32::{
    Foundation::{BOOL, HINSTANCE, LPARAM, LRESULT, RECT, WPARAM},
    System::SystemServices::DLL_PROCESS_DETACH,
    UI::WindowsAndMessaging::{
        CallNextHookEx, GetWindowRect, IsIconic, IsWindow, IsZoomed, SendMessageW, SetWindowPos, ShowWindow, HCBT_ACTIVATE, HCBT_MINMAX, HWND_BOTTOM, HWND_TOP, SIZE_RESTORED, SWP_NOACTIVATE, SWP_NOREDRAW, SWP_NOREPOSITION, SW_FORCEMINIMIZE, SW_MAXIMIZE, SW_MINIMIZE, SW_NORMAL, SW_SHOWMINIMIZED, WM_SIZE
    },
};

mod window;

lazy_static! {
    static ref LAST_HWND: spin::Mutex<Option<usize>> = spin::Mutex::new(None);

    static ref MAXMIZED: spin::Mutex<bool> = spin::Mutex::new(false);
    static ref WND_POS: spin::Mutex<Option<WindowPos>> = spin::Mutex::new(None);
}

#[derive(Debug)]
struct WindowPos {
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
}

#[no_mangle]
extern "C" fn DllMain(_hinst: HINSTANCE, fdw_reason: u32, _lpv_reserved: c_void) -> BOOL {
    if fdw_reason == DLL_PROCESS_DETACH {
        let hwnd = LAST_HWND.lock().take();
        if let Some(hwnd) = hwnd {
            let hwnd = hwnd as *mut c_void;
            if unsafe { IsWindow(hwnd) } == 0 {
                return 1;
            }
            let wnd_pos = WND_POS.lock().take();
            if let Some(wnd_pos) = wnd_pos {
                unsafe {
                    SetWindowPos(
                        hwnd,
                        HWND_BOTTOM,
                        wnd_pos.x,
                        wnd_pos.y,
                        wnd_pos.cx,
                        wnd_pos.cy,
                        SWP_NOACTIVATE | SWP_NOREPOSITION | SWP_NOREDRAW,
                    );
                }
                if *MAXMIZED.lock() {
                    unsafe { ShowWindow(hwnd, SW_MAXIMIZE) };
                }
            }
        }
    }
    1
}

#[no_mangle]
extern "C" fn CBTProc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hcbt = ncode as u32;
    if hcbt == HCBT_MINMAX {
        let sw = (lparam & 0xffff) as i32;
        if sw == SW_NORMAL {
            return 0;
        }
        let mut wnd_pos_lck = WND_POS.lock();
        if wnd_pos_lck.is_none()
            && (sw == SW_SHOWMINIMIZED || sw == SW_MINIMIZE || sw == SW_FORCEMINIMIZE)
        {
            let hwnd = wparam as *mut c_void;
            let maximized = unsafe { IsZoomed(hwnd) } != 0;
            let mut rect = RECT {
                left: 0,
                right: 0,
                top: 0,
                bottom: 0,
            };
            if maximized {
                unsafe { ShowWindow(hwnd, SW_NORMAL) };
            }
            if unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
                let wnd_pos = WindowPos {
                    x: rect.left,
                    y: rect.top,
                    cx: rect.right - rect.left,
                    cy: rect.bottom - rect.top,
                };
                let _ = unsafe { SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, 0) };
                unsafe {
                    SendMessageW(
                        hwnd,
                        WM_SIZE,
                        SIZE_RESTORED as usize,
                        ((wnd_pos.y << 16) | wnd_pos.cx) as isize,
                    )
                };
                *MAXMIZED.lock() = maximized;
                *LAST_HWND.lock() = Some(hwnd as usize);
                *wnd_pos_lck = Some(wnd_pos);
                let _ = window::switch_focus(hwnd);
                return 1;
            }
        }
    } else if hcbt == HCBT_ACTIVATE {
        let wnd_pos = WND_POS.lock().take();
        if let Some(wnd_pos) = wnd_pos {
            LAST_HWND.lock().take();
            let hwnd = wparam as *mut c_void;
            if unsafe { IsIconic(hwnd) } != 0 {
                // to set window pos, window must not be minimized.
                unsafe { ShowWindow(hwnd, SW_NORMAL) };
            }
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    HWND_TOP,
                    wnd_pos.x,
                    wnd_pos.y,
                    wnd_pos.cx,
                    wnd_pos.cy,
                    SWP_NOACTIVATE,
                )
            };
            if *MAXMIZED.lock() {
                unsafe { ShowWindow(hwnd, SW_MAXIMIZE) };
            }
        }
    }
    unsafe { CallNextHookEx(core::ptr::null_mut(), ncode, wparam, lparam) }
}
