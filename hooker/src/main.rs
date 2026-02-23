//! Helper Wine program for CBT hooking.

use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, MAX_PATH},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            ProcessStatus::GetModuleFileNameExW,
            Threading::{
                INFINITE, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SYNCHRONIZE,
                PROCESS_VM_READ, WaitForSingleObject,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, SetWindowsHookExW, UnhookWindowsHookEx, WH_CBT,
        },
    },
    core::{BOOL, Error, HSTRING, PCWSTR},
};

// TODO: dedupe this?
pub fn get_process_thread_ids(process_id: u32) -> Result<Vec<u32>, Error> {
    unsafe {
        // Create a snapshot of all currently running threads
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)?;

        let mut thread_entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };

        let mut thread_ids = Vec::new();

        // Iterate through all threads in the snapshot
        if Thread32First(snapshot, &mut thread_entry).is_ok() {
            loop {
                // Check if thread belongs to our target process
                if thread_entry.th32OwnerProcessID == process_id {
                    thread_ids.push(thread_entry.th32ThreadID);
                }

                // Prepare for next iteration
                thread_entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;

                // Move to next thread entry
                if Thread32Next(snapshot, &mut thread_entry).is_err() {
                    break;
                }
            }
        }

        Ok(thread_ids)
    }
}

type SyncState = (windows::Win32::Foundation::HANDLE, u32);

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let mut pid = 0;
    unsafe {
        if GetWindowThreadProcessId(hwnd, Some(&mut pid)) == 0 {
            return BOOL(1);
        }
    }

    let Ok(handle) = (unsafe {
        OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
    }) else {
        return BOOL(1);
    };

    let mut file_name_buf = [0u16; MAX_PATH as usize];
    let len = unsafe { GetModuleFileNameExW(Some(handle), None, &mut file_name_buf) };
    if len == 0 {
        let _ = unsafe { CloseHandle(handle) };
        return BOOL(1);
    }

    let file_name = String::from_utf16_lossy(&file_name_buf[..len as usize]);
    if !file_name.ends_with("cloudmusic.exe") {
        let _ = unsafe { CloseHandle(handle) };
        return BOOL(1);
    }

    unsafe {
        *Box::from_raw(lparam.0 as *mut Option<SyncState>) = Some((handle, pid));
    }

    BOOL(0)
}

fn main() {
    let state: Box<Option<SyncState>> = Box::new(None);
    let state_ptr = Box::into_raw(state);
    let _ = unsafe { EnumWindows(Some(enum_windows_proc), LPARAM(state_ptr as isize)) };
    let state = unsafe { *Box::from_raw(state_ptr) };
    if state.is_none() {
        eprintln!("Failed to find Cloud Music window");
        return;
    }

    let (handle, pid) = state.unwrap();

    let mut hook = Vec::new();
    'hook_loop: loop {
        'hook: {
            unsafe {
                if !hook.is_empty() {
                    break 'hook;
                }
                let Ok(threads) = get_process_thread_ids(pid) else {
                    break 'hook;
                };
                let hook_lib_name = HSTRING::from("wndhok.dll");
                let Ok(lib) = LoadLibraryW(PCWSTR(hook_lib_name.as_ptr())) else {
                    break 'hook;
                };
                let Some(proc) =
                    GetProcAddress(lib, windows::core::PCSTR(c"CBTProc".as_ptr().cast()))
                else {
                    break 'hook;
                };
                let proc = std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(
                        i32,
                        windows::Win32::Foundation::WPARAM,
                        windows::Win32::Foundation::LPARAM,
                    )
                        -> windows::Win32::Foundation::LRESULT,
                >(proc);
                for thread in threads {
                    if let Ok(hhook) =
                        SetWindowsHookExW(WH_CBT, Some(proc), Some(lib.into()), thread)
                    {
                        hook.push(hhook);
                    }
                }
                if !hook.is_empty() {
                    break 'hook_loop;
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    unsafe { WaitForSingleObject(handle, INFINITE) };

    for h in hook {
        unsafe { UnhookWindowsHookEx(h).unwrap() };
    }
}
