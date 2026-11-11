//! 全局取词 + 全局快捷键。
//!
//! 取词策略：低级鼠标钩子判断「拖选」，然后模拟 Ctrl+C 读剪贴板（读前清空、读后还原）。
//! 拿不到选区位置，所以用鼠标松开点作为浮层锚点 —— 这和划词工具的实际手感一致。
//!
//! 快捷键策略：同一个键盘钩子里跟踪修饰键状态，匹配到配置的组合就复用同一个锚点。
//! 配置放在内存（OnceLock），钩子回调里绝不做 I/O，否则会卡住整个系统的键鼠。

use arboard::Clipboard;
use rdev::{listen, Button, EventType, Key};
use std::sync::mpsc::channel;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// 一次取词请求的锚点（屏幕物理像素）
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub via: &'static str,
}

// ---------------- 快捷键配置 ----------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hotkey {
    pub enabled: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: Option<Key>,
}

static HOTKEY: OnceLock<Mutex<Hotkey>> = OnceLock::new();

/// 设置变更或启动时调用，把字符串规格解析成内存里的匹配规则
pub fn apply_hotkey(spec: &str, enabled: bool) {
    let mut parsed = parse_hotkey(spec);
    // 总开关关掉时，配置照样解析（用户可能只是临时停用），但匹配时直接不认
    if !enabled {
        parsed.enabled = false;
    }
    eprintln!(
        "[huayi] 快捷键：{} → {:?}",
        if spec.trim().is_empty() { "(无)" } else { spec },
        parsed
    );
    match HOTKEY.get() {
        Some(m) => {
            if let Ok(mut g) = m.lock() {
                *g = parsed;
            }
        }
        None => {
            let _ = HOTKEY.set(Mutex::new(parsed));
        }
    }
}

fn hotkey_now() -> Hotkey {
    HOTKEY
        .get()
        .and_then(|m| m.lock().ok().map(|g| g.clone()))
        .unwrap_or_default()
}

fn parse_hotkey(spec: &str) -> Hotkey {
    let spec = spec.trim();
    if spec.is_empty() {
        return Hotkey::default(); // enabled = false
    }

    let mut h = Hotkey {
        enabled: true,
        ..Default::default()
    };

    for part in spec.split('+') {
        match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => h.ctrl = true,
            "alt" => h.alt = true,
            "shift" => h.shift = true,
            "meta" | "win" | "super" => h.meta = true,
            other => match map_key(other) {
                Some(k) => h.key = Some(k),
                None => return Hotkey::default(), // 认不出来的组合直接禁用
            },
        }
    }

    if h.key.is_none() {
        return Hotkey::default();
    }
    h
}

fn map_key(s: &str) -> Option<Key> {
    use Key::*;
    let up = s.trim().to_ascii_uppercase();
    Some(match up.as_str() {
        "A" => KeyA,
        "B" => KeyB,
        "C" => KeyC,
        "D" => KeyD,
        "E" => KeyE,
        "F" => KeyF,
        "G" => KeyG,
        "H" => KeyH,
        "I" => KeyI,
        "J" => KeyJ,
        "K" => KeyK,
        "L" => KeyL,
        "M" => KeyM,
        "N" => KeyN,
        "O" => KeyO,
        "P" => KeyP,
        "Q" => KeyQ,
        "R" => KeyR,
        "S" => KeyS,
        "T" => KeyT,
        "U" => KeyU,
        "V" => KeyV,
        "W" => KeyW,
        "X" => KeyX,
        "Y" => KeyY,
        "Z" => KeyZ,
        "0" => Num0,
        "1" => Num1,
        "2" => Num2,
        "3" => Num3,
        "4" => Num4,
        "5" => Num5,
        "6" => Num6,
        "7" => Num7,
        "8" => Num8,
        "9" => Num9,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        "F5" => F5,
        "F6" => F6,
        "F7" => F7,
        "F8" => F8,
        "F9" => F9,
        "F10" => F10,
        "F11" => F11,
        "F12" => F12,
        "SPACE" => Space,
        "ENTER" | "RETURN" => Return,
        "TAB" => Tab,
        "ESC" | "ESCAPE" => Escape,
        "BACKSPACE" => Backspace,
        "DELETE" | "DEL" => Delete,
        "INSERT" | "INS" => Insert,
        "HOME" => Home,
        "END" => End,
        "PAGEUP" | "PGUP" => PageUp,
        "PAGEDOWN" | "PGDN" => PageDown,
        "UP" => UpArrow,
        "DOWN" => DownArrow,
        "LEFT" => LeftArrow,
        "RIGHT" => RightArrow,
        _ => return None,
    })
}

// ---------------- 主流程 ----------------

pub fn start(app: AppHandle) {
    let (tx, rx) = channel::<(f64, f64, &'static str)>();

    // ---- 工作线程：真正取词并唤起浮层 ----
    let worker_app = app.clone();
    thread::spawn(move || {
        while let Ok((x, y, via)) = rx.recv() {
            // 自己在自己窗口里拖选，不处理，否则会自激
            if is_own_window(x as i32, y as i32) {
                continue;
            }

            let auto = {
                let db = worker_app.state::<crate::db::Db>();
                db.load_settings().auto_popup
            };
            if !auto && via == "drag" {
                continue;
            }

            // 等目标应用把选区状态更新完再发 Ctrl+C
            thread::sleep(Duration::from_millis(45));

            let text = match try_uia() {
                Some(t) => t,
                None => match grab_via_clipboard() {
                    Some(t) => t,
                    None => continue, // 静默失败，不弹
                },
            };

            let anchor = Anchor { x, y, via };
            crate::popup::show_pending(&worker_app, &text, anchor);
        }
    });

    // ---- 钩子线程：只做判断和转发，绝不在回调里阻塞 ----
    let hook_app = app.clone();
    thread::spawn(move || {
        let mut down: Option<(f64, f64)> = None;
        let mut pos = (0.0f64, 0.0f64);
        let mut last_anchor = (0.0f64, 0.0f64);
        let mut last_fire: Option<Instant> = None;

        // 修饰键状态自己维护 —— rdev 事件里不附带修饰键状态
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut meta = false;

        let result = listen(move |ev| {
            match ev.event_type {
                // rdev 0.5 只有 MouseMove 带坐标，按键事件要靠最后一次移动来定位
                EventType::MouseMove { x, y } => {
                    pos = (x, y);
                }
                EventType::ButtonPress(Button::Left) => {
                    // 点到浮层外面就收起来（浮层不抢焦点，所以要靠这里兜底关闭）
                    let (cx, cy) = cursor_pos();
                    crate::popup::hide_if_clicked_outside(&hook_app, cx, cy);
                    down = Some(pos);
                }
                EventType::ButtonRelease(Button::Left) => {
                    if let Some((dx, dy)) = down.take() {
                        let moved = ((pos.0 - dx).powi(2) + (pos.1 - dy).powi(2)).sqrt();
                        if moved >= 12.0 {
                            last_anchor = pos;
                            let _ = tx.send((pos.0, pos.1, "drag"));
                        }
                    }
                }

                EventType::KeyPress(k) => match k {
                    Key::ControlLeft | Key::ControlRight => ctrl = true,
                    Key::Alt | Key::AltGr => alt = true,
                    Key::ShiftLeft | Key::ShiftRight => shift = true,
                    Key::MetaLeft | Key::MetaRight => meta = true,
                    _ => {
                        let hk = hotkey_now();
                        // 这里绝对不能打日志：钩子回调里做 I/O 会拖慢整个系统的键盘输入
                        let hit = hk.enabled
                            && hk.key == Some(k)
                            && hk.ctrl == ctrl
                            && hk.alt == alt
                            && hk.shift == shift
                            && hk.meta == meta;
                        if hit {
                            // 长按会连续触发 KeyPress，防抖
                            let now = Instant::now();
                            let ready = last_fire
                                .map(|t| now.duration_since(t).as_millis() > 400)
                                .unwrap_or(true);
                            if ready {
                                last_fire = Some(now);
                                let at = if pos == (0.0, 0.0) { last_anchor } else { pos };
                                let _ = tx.send((at.0, at.1, "hotkey"));
                            }
                        }
                    }
                },
                EventType::KeyRelease(k) => match k {
                    Key::ControlLeft | Key::ControlRight => ctrl = false,
                    Key::Alt | Key::AltGr => alt = false,
                    Key::ShiftLeft | Key::ShiftRight => shift = false,
                    Key::MetaLeft | Key::MetaRight => meta = false,
                    _ => {}
                },
                _ => {}
            }
        });

        if let Err(e) = result {
            eprintln!("[huayi] 全局钩子启动失败：{:?}（可能是安全软件拦截）", e);
        }
    });
}

/// UIA 加速通道。当前版本直接返回 None（走剪贴板），接口先留着。
#[allow(unused_variables)]
fn try_uia() -> Option<String> {
    // TODO(阶段二)：用 uiautomation crate 取焦点元素的 TextPattern.GetSelection()。
    // 命中时不碰剪贴板，速度快且无副作用；Chrome/VS Code 需要各自开启无障碍。
    None
}

/// 剪贴板方案：清空 → Ctrl+C → 读 → 还原。
/// 任何一步不对就返回 None，调用方静默跳过。
fn grab_via_clipboard() -> Option<String> {
    let mut cb = Clipboard::new().ok()?;

    // 备份原内容（只保文本，图片/富文本会丢 —— MVP 已知限制）
    let backup = cb.get_text().ok();

    // 清空，这样"剪贴板有内容"才能证明 Ctrl+C 真的生效了
    if cb.set_text(String::new()).is_err() {
        return None;
    }

    send_ctrl_c();
    thread::sleep(Duration::from_millis(130));

    let grabbed = cb.get_text().ok().unwrap_or_default();

    // 无论如何都要还原，哪怕没拿到东西
    if let Some(b) = backup {
        let _ = cb.set_text(b);
    }

    let text = grabbed.trim().to_string();

    if text.is_empty() || text.chars().count() > 1500 {
        return None;
    }
    // 没字母就不是英文，不弹
    if !text.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    // 全是控制字符的垃圾
    if text.chars().filter(|c| c.is_control()).count() > 4 {
        return None;
    }

    Some(text)
}

#[cfg(windows)]
fn send_ctrl_c() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, GetAsyncKeyState, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL, VK_C, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    /// 键当前物理按下吗
    unsafe fn is_down(vk: VIRTUAL_KEY) -> bool {
        (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0
    }

    /// 把还按着的修饰键抬起来。
    /// 只发 keyup、不补 keydown —— 用户真实松手时会再发一次 keyup，
    /// 重复的 keyup 是无害的，反过来补 keydown 则可能把键卡住。
    unsafe fn release(vk: VIRTUAL_KEY) {
        if is_down(vk) {
            keybd_event(vk.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }

    unsafe {
        // 快捷键触发时用户必然按着 Alt（或 Ctrl/Shift），
        // 不清掉这些修饰键，发出去的就不是 Ctrl+C 而是 Alt+Ctrl+C，
        // 目标应用不认，剪贴板拿不到东西 —— 表现就是"按了没反应"。
        release(VK_LWIN);
        release(VK_RWIN);
        release(VK_MENU);
        release(VK_CONTROL);
        release(VK_SHIFT);

        thread::sleep(Duration::from_millis(14));

        keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_C.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        thread::sleep(Duration::from_millis(18));
        keybd_event(VK_C.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

#[cfg(windows)]
fn cursor_pos() -> (i32, i32) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut p).is_ok() {
            (p.x, p.y)
        } else {
            (0, 0)
        }
    }
}

#[cfg(not(windows))]
fn cursor_pos() -> (i32, i32) {
    (0, 0)
}

/// 鼠标落点是不是我们自己的窗口（浮层或主窗口），避免自激
#[cfg(windows)]
fn is_own_window(x: i32, y: i32) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, WindowFromPoint};

    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

#[cfg(not(windows))]
fn is_own_window(_x: i32, _y: i32) -> bool {
    false
}
