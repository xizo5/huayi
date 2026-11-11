//! 浮层窗口的定位与尺寸管理。
//!
//! 尺寸用逻辑像素（和 CSS 对齐，高 DPI 下不会显示不全），
//! 位置用物理像素（和鼠标钩子给的系统坐标对齐）。

use crate::selection::Anchor;
use std::sync::Mutex;
use tauri::{Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow};

pub const BTN_W: f64 = 132.0;
pub const BTN_H: f64 = 40.0;
/// 卡片宽度，和前端 CSS 里的 .card width 保持一致
pub const CARD_W: f64 = 400.0;
/// 骨架屏阶段的占位高度，翻译结果回来后前端会重新报真实高度
const LOADING_H: f64 = 168.0;

static LAST_ANCHOR: Mutex<Option<Anchor>> = Mutex::new(None);

#[derive(Clone, serde::Serialize)]
struct PickPayload {
    text: String,
    x: f64,
    y: f64,
    /// true = 取完词直接翻译，不显示中间的「翻译」按钮
    auto: bool,
}

/// 取词成功：把浮层摆好并通知前端。
///
/// 两种进入方式：
/// - 快捷键（via = "hotkey"）：直接按卡片尺寸展开，前端立刻开始翻译，中间不点按钮；
/// - 拖选（via = "drag"）：只弹出一个小胶囊，等用户决定要不要翻。
pub fn show_pending(app: &tauri::AppHandle, text: &str, anchor: Anchor) {
    if let Ok(mut guard) = LAST_ANCHOR.lock() {
        *guard = Some(anchor);
    }

    let Some(win) = app.get_webview_window("popup") else {
        eprintln!("[huayi] 找不到 popup 窗口");
        return;
    };

    // 先收起再定位，避免在旧位置闪一下
    crate::platform::hide(&win);

    let auto = anchor.via == "hotkey";

    if auto {
        let (x, y) = compute(&win, anchor, CARD_W, LOADING_H, Mode::Card);
        let _ = win.set_size(LogicalSize::new(CARD_W, LOADING_H));
        let _ = win.set_position(PhysicalPosition::new(x, y));
    } else {
        let (x, y) = compute(&win, anchor, BTN_W, BTN_H, Mode::Button);
        let _ = win.set_size(LogicalSize::new(BTN_W, BTN_H));
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }

    crate::platform::show_no_activate(&win);

    let _ = win.emit(
        "pick",
        PickPayload {
            text: text.to_string(),
            x: anchor.x,
            y: anchor.y,
            auto,
        },
    );
}

/// 用户点到浮层外面就收起来。
/// 注意不能用 `win.is_visible()` 判断 —— 我们用 SW_SHOWNOACTIVATE 显示的，
/// Tauri 内部状态不认，会出现"结果显示着但认为隐藏"的错乱。
pub fn hide_if_clicked_outside(app: &tauri::AppHandle, x: i32, y: i32) {
    let Some(win) = app.get_webview_window("popup") else {
        return;
    };
    if !crate::platform::is_visible(&win) {
        return;
    }
    if let (Ok(p), Ok(s)) = (win.outer_position(), win.outer_size()) {
        let inside = x >= p.x
            && x < p.x + s.width as i32
            && y >= p.y
            && y < p.y + s.height as i32;
        if !inside {
            crate::platform::hide(&win);
        }
    }
}

/// 浮层收起（供命令和内部复用）
pub fn close(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("popup") {
        crate::platform::hide(&win);
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    /// 只放一个小胶囊按钮
    Button,
    /// 放翻译卡片
    Card,
}

fn compute(win: &WebviewWindow, anchor: Anchor, w: f64, h: f64, mode: Mode) -> (i32, i32) {
    let scale = win.scale_factor().unwrap_or(1.0);
    let pw = w * scale;
    let ph = h * scale;
    let (sx, sy, sw, sh) = screen_bounds(win);

    let (mut x, mut y) = match mode {
        Mode::Button => (anchor.x - pw / 2.0, anchor.y - ph - 10.0),
        Mode::Card => (anchor.x - 16.0, anchor.y + 14.0),
    };

    // 下方放不下就翻到锚点上方
    if mode == Mode::Card && y + ph > sy + sh - 8.0 {
        let above = anchor.y - ph - 24.0;
        if above >= sy + 8.0 {
            y = above;
        }
    }

    x = x.clamp(sx + 8.0, (sx + sw - pw - 8.0).max(sx + 8.0));
    y = y.clamp(sy + 8.0, (sy + sh - ph - 8.0).max(sy + 8.0));

    (x as i32, y as i32)
}

fn screen_bounds(win: &WebviewWindow) -> (f64, f64, f64, f64) {
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());

    match monitor {
        Some(m) => {
            let p = m.position();
            let s = m.size();
            (p.x as f64, p.y as f64, s.width as f64, s.height as f64)
        }
        None => (0.0, 0.0, 1920.0, 1080.0),
    }
}

// ---------------- 命令 ----------------

/// 前端把内容渲染完、量出真实高度后再调这里
#[tauri::command]
pub fn popup_resize(
    app: tauri::AppHandle,
    width: f64,
    height: f64,
    mode: String,
) -> Result<(), String> {
    let win = app
        .get_webview_window("popup")
        .ok_or_else(|| "浮层窗口不存在".to_string())?;

    let anchor = LAST_ANCHOR
        .lock()
        .ok()
        .and_then(|g| *g)
        .ok_or_else(|| "还没有锚点".to_string())?;

    let m = if mode == "card" { Mode::Card } else { Mode::Button };
    let w = width.clamp(120.0, 560.0);
    let h = height.clamp(36.0, 900.0);

    let (x, y) = compute(&win, anchor, w, h, m);
    win.set_size(LogicalSize::new(w, h))
        .map_err(|e| e.to_string())?;
    win.set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    Ok(())
}
