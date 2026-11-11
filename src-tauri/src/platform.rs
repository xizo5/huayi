//! Windows 窗口操作。
//!
//! 为什么不用 Tauri 自带的 `win.show()` / `win.hide()`：
//!
//! 1. `show()` 走 SW_SHOW，会抢前台焦点 —— 划词工具绝不能打断用户正在敲的字。
//! 2. 主窗口被最小化或收进托盘后，`show()` 不还原，用户点了"去设置"看不到窗口。
//! 3. 用 SW_SHOWNOACTIVATE 显示后的窗口，Tauri 内部可见状态和实际不一致，
//!    `hide()` 会失效 —— 表现就是浮层关不掉。
//!
//! 所以这里全部走 Win32，显式和状态无关。

use tauri::WebviewWindow;

#[cfg(windows)]
fn hwnd_of(win: &WebviewWindow) -> Option<windows::Win32::Foundation::HWND> {
    win.hwnd()
        .ok()
        .map(|h| windows::Win32::Foundation::HWND(h.0 as *mut core::ffi::c_void))
}

/// 显示但不激活。浮层专用。
#[cfg(windows)]
pub fn show_no_activate(win: &WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
    if let Some(h) = hwnd_of(win) {
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        }
    } else {
        let _ = win.show();
    }
}

/// 显示并抢焦点。主窗口用，最小化状态也能还原。
#[cfg(windows)]
pub fn show_and_focus(win: &WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE};
    if let Some(h) = hwnd_of(win) {
        unsafe {
            let _ = ShowWindow(h, SW_RESTORE);
            let _ = SetForegroundWindow(h);
        }
    } else {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[cfg(windows)]
pub fn hide(win: &WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    if let Some(h) = hwnd_of(win) {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    } else {
        let _ = win.hide();
    }
}

#[cfg(windows)]
pub fn is_visible(win: &WebviewWindow) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    match hwnd_of(win) {
        Some(h) => unsafe { IsWindowVisible(h).as_bool() },
        None => false,
    }
}

// ---------------- 非 Windows 退路 ----------------

#[cfg(not(windows))]
pub fn show_no_activate(win: &WebviewWindow) {
    let _ = win.show();
}

#[cfg(not(windows))]
pub fn show_and_focus(win: &WebviewWindow) {
    let _ = win.show();
    let _ = win.set_focus();
}

#[cfg(not(windows))]
pub fn hide(win: &WebviewWindow) {
    let _ = win.hide();
}

#[cfg(not(windows))]
pub fn is_visible(win: &WebviewWindow) -> bool {
    win.is_visible().unwrap_or(false)
}

#[cfg(not(windows))]
pub fn info_box(_title: &str, _text: &str) {}

/// 原生提示框。只在"第一次点 × 收进托盘"时用一次，讲清楚窗口去哪了。
#[cfg(windows)]
pub fn info_box(title: &str, text: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
    };
    unsafe {
        let _ = MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
        );
    }
}
