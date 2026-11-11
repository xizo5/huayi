mod commands;
mod db;
mod models;
mod platform;
mod popup;
mod selection;
mod translate;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        // 单实例必须最先注册：否则用户双击两次 exe 就有两套全局钩子、
        // 两个托盘图标、两份数据库连接，关掉一个另一个还在，看起来像"关不掉"。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                platform::show_and_focus(&w);
            }
        }))
        .setup(|app| {
            // 数据目录 + SQLite
            let dir = app
                .path()
                .app_data_dir()
                .expect("拿不到 app_data_dir");
            std::fs::create_dir_all(&dir).expect("创建数据目录失败");
            let db_path = dir.join("huayi.db");
            let db = db::Db::open(&db_path).expect("初始化数据库失败");
            app.manage(db);

            setup_tray(app)?;

            // 全局取词：鼠标钩子 + 剪贴板。
            // 快捷键配置必须在 start 之前塞进去，否则钩子线程第一帧读到的是默认值。
            let cfg = app.state::<db::Db>().load_settings();
            selection::apply_hotkey(&cfg.hotkey, cfg.hotkey_enabled);
            selection::start(app.handle().clone());

            // 首次启动把主窗口亮出来，否则用户以为没反应。
            // 尺寸和居中必须在这里显式设置：visible:false 创建时 Windows 给的是占位坐标，
            // 放在屏幕外（-25216,-25600）那种，直接 show 会看不见。
            if let Some(w) = app.get_webview_window("main") {
                use tauri::LogicalSize;
                let _ = w.set_size(LogicalSize::new(900.0, 660.0));
                let _ = w.center();
                // 启动时不激活窗口：用户可能正在别的窗口打字，别把他的字打断
                platform::show_no_activate(&w);
            }

            // 调试：HUAYI_DEBUG_POPUP=1 启动后直接摆一个浮层出来，省得每次都去划词
            if std::env::var("HUAYI_DEBUG_POPUP").is_ok() {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1600));
                    popup::show_pending(
                        &h,
                        "Organize text files into a daily information report.",
                        selection::Anchor {
                            x: 900.0,
                            y: 640.0,
                            via: "debug",
                        },
                    );
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::translate,
            commands::add_word,
            commands::list_words,
            commands::delete_word,
            commands::stats,
            commands::get_settings,
            commands::save_settings,
            commands::test_engine,
            commands::prepare_review,
            commands::get_review,
            commands::answer_question,
            commands::show_main,
            commands::hide_popup,
            popup::popup_resize,
        ])
        .on_window_event(|window, event| {
            // 主窗口点 × 只是收进托盘，不退出 —— 划词监听必须一直在跑。
            // 注意这里不能用 window.hide()：主窗口是 SW_RESTORE 显示的，
            // Tauri 内部可见状态跟实际不一致，hide 会被跳过（表现为"关闭没反应"）。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();

                    let app = window.app_handle();
                    let db = app.state::<db::Db>();
                    if !db.get_flag("close_hint_shown") {
                        platform::info_box(
                            "划译已收进托盘",
                            "划词监听还在后台运行。\n\n重新打开生词本：单击托盘图标。\n完全退出：右键托盘图标 → 退出划译。",
                        );
                        let _ = db.set_flag("close_hint_shown", true);
                    }

                    if let Some(w) = app.get_webview_window("main") {
                        platform::hide(&w);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开生词本", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "收起浮层", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出划译", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &hide, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("划译 · 划词即译")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    platform::show_and_focus(&w);
                }
            }
            "hide" => {
                if let Some(w) = app.get_webview_window("popup") {
                    platform::hide(&w);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    platform::show_and_focus(&w);
                }
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}
