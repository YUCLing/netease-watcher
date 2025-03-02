use smallvec::SmallVec;
use windows_sys::Win32::{
    Foundation::{GetLastError, BOOL, HWND, LPARAM},
    UI::WindowsAndMessaging::{EnumWindows, IsIconic, IsWindowVisible, SetForegroundWindow},
};

// 枚举窗口的回调函数
unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // 从 LPARAM 中获取 Vec<HWND> 的指针
    let handles = &mut *(lparam as *mut SmallVec<[HWND; 512]>);

    // 检查窗口是否可见且未最小化
    if IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) != 0 {
        handles.push(hwnd);
    }
    1
}

// 切换焦点到下一个窗口
pub fn switch_focus(current: HWND) -> Result<(), u32> {
    unsafe {
        // 存储所有符合条件的窗口句柄
        let mut handles = SmallVec::<[HWND; 512]>::new();

        // 枚举所有顶层窗口，并将 Vec<HWND> 的指针作为 LPARAM 传递
        if EnumWindows(Some(enum_callback), &mut handles as *mut _ as isize) == 0 {
            return Err(GetLastError());
        }

        if handles.is_empty() {
            return Ok(());
        }

        // 查找当前窗口在列表中的位置
        let current_pos = handles
            .iter()
            .position(|&h| h == current)
            .unwrap_or(usize::MAX);

        // 计算下一个有效索引（支持循环切换）
        let target_index = if current_pos == usize::MAX || current_pos + 1 >= handles.len() {
            0 // 未找到当前窗口或已是最后一个时切到第一个
        } else {
            current_pos + 1
        };

        // 设置焦点到目标窗口
        let _ = SetForegroundWindow(handles[target_index]);
    }
    Ok(())
}
