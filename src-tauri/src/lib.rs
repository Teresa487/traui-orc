//! TrauiOCR —— 截图 OCR 客户端
//!
//! 流程：全局热键 → 隐藏主窗 → 截整屏 → 全屏遮罩框选 → 裁剪 → 调 API → 显示结果

mod api;
mod capture;
mod commands;
mod config;

use commands::AppState;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

/// 解析快捷键字符串。支持 "F4"、"Ctrl+Alt+O" 这类写法。
fn parse_hotkey(s: &str) -> Option<Shortcut> {
    let mut modifiers = Modifiers::empty();
    let mut code: Option<Code> = None;

    for raw in s.split('+') {
        let part = raw.trim();
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" | "cmdorctrl" | "commandorcontrol" => modifiers |= Modifiers::CONTROL,
            "alt" | "option" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "super" | "meta" | "win" | "command" => modifiers |= Modifiers::SUPER,
            _ => {
                code = parse_code(part);
            }
        }
    }

    let code = code?;
    Some(if modifiers.is_empty() {
        Shortcut::new(None, code)
    } else {
        Shortcut::new(Some(modifiers), code)
    })
}

fn parse_code(s: &str) -> Option<Code> {
    // 函数键
    if let Some(rest) = s.strip_prefix('F').or_else(|| s.strip_prefix('f')) {
        if let Ok(n) = rest.parse::<u8>() {
            return match n {
                1 => Some(Code::F1),
                2 => Some(Code::F2),
                3 => Some(Code::F3),
                4 => Some(Code::F4),
                5 => Some(Code::F5),
                6 => Some(Code::F6),
                7 => Some(Code::F7),
                8 => Some(Code::F8),
                9 => Some(Code::F9),
                10 => Some(Code::F10),
                11 => Some(Code::F11),
                12 => Some(Code::F12),
                _ => None,
            };
        }
    }
    // 单个字母
    if s.len() == 1 {
        let c = s.chars().next()?.to_ascii_uppercase();
        return match c {
            'A' => Some(Code::KeyA),
            'B' => Some(Code::KeyB),
            'C' => Some(Code::KeyC),
            'D' => Some(Code::KeyD),
            'E' => Some(Code::KeyE),
            'F' => Some(Code::KeyF),
            'G' => Some(Code::KeyG),
            'H' => Some(Code::KeyH),
            'I' => Some(Code::KeyI),
            'J' => Some(Code::KeyJ),
            'K' => Some(Code::KeyK),
            'L' => Some(Code::KeyL),
            'M' => Some(Code::KeyM),
            'N' => Some(Code::KeyN),
            'O' => Some(Code::KeyO),
            'P' => Some(Code::KeyP),
            'Q' => Some(Code::KeyQ),
            'R' => Some(Code::KeyR),
            'S' => Some(Code::KeyS),
            'T' => Some(Code::KeyT),
            'U' => Some(Code::KeyU),
            'V' => Some(Code::KeyV),
            'W' => Some(Code::KeyW),
            'X' => Some(Code::KeyX),
            'Y' => Some(Code::KeyY),
            'Z' => Some(Code::KeyZ),
            _ => None,
        };
    }
    match s.to_ascii_lowercase().as_str() {
        "printscreen" | "snapshot" => Some(Code::PrintScreen),
        "space" => Some(Code::Space),
        "enter" | "return" => Some(Code::Enter),
        "escape" | "esc" => Some(Code::Escape),
        _ => None,
    }
}

/// 遮罩出现前截下的屏幕内容，供前端取走当背景
pub struct OverlayShot(pub std::sync::Mutex<Option<OverlayShotData>>);

pub struct OverlayShotData {
    pub monitor: usize,
    pub png_b64: String,
}

/// 拉起遮罩：先抓屏存进内存，再显示已存在的遮罩窗口。
///
/// 注意不要在事件处理器里新建窗口 —— Tauri 官方文档指出，Windows 上在
/// 同步的事件处理里创建 webview 会死锁。所以遮罩窗口预先在 tauri.conf.json
/// 里声明好，这里只做「抓图 + 显示」。
fn spawn_overlay(app: &tauri::AppHandle) -> Result<(), String> {
    // 跟鼠标走：用户在哪块屏幕上按的快捷键，就在哪块屏幕上框选
    let (cx, cy) = app
        .cursor_position()
        .map(|p| (p.x as i32, p.y as i32))
        .unwrap_or((0, 0));
    let _ = app.emit("capture-stage", format!("鼠标位置 {cx},{cy}"));

    let (idx, m) = capture::monitor_at(cx, cy).map_err(|e| format!("定位显示器失败: {e}"))?;
    let _ = app.emit(
        "capture-stage",
        format!("屏幕 {idx}: {}x{} @{:.1}x", m.width, m.height, m.scale_factor),
    );

    // 遮罩还没出现，此刻抓的才是真实画面
    let shot = capture::capture_monitor_png_base64(idx).map_err(|e| format!("抓屏失败: {e}"))?;
    let _ = app.emit("capture-stage", format!("抓屏成功 {} 字节", shot.len()));

    {
        let state = app.state::<OverlayShot>();
        *state.0.lock().unwrap() = Some(OverlayShotData {
            monitor: idx,
            png_b64: shot,
        });
    }

    // 遮罩是全屏窗口，只在鼠标所在那块屏幕上显示
    let win = app
        .get_webview_window("overlay")
        .ok_or("找不到遮罩窗口")?;

    // 用物理像素精确定位到鼠标所在的那块屏幕
    let _ = win.set_position(tauri::PhysicalPosition::new(m.x, m.y));
    let _ = win.set_size(tauri::PhysicalSize::new(m.width, m.height));
    let _ = win.show();
    let _ = win.set_focus();

    // 遮罩窗口在启动时就已加载，JS 不会随显示重新执行，
    // 所以每次显示后都要通知它重新取图。
    // 稍等一帧，等窗口真正可见再发，避免事件先于窗口就绪。
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        let _ = app2.emit("begin-capture", ());
    });

    Ok(())
}

/// 把主窗口藏起来（关窗口按钮不退出程序，只收进托盘）
fn hide_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// 从托盘恢复主窗口
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 建托盘图标 + 右键菜单
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let item_capture = MenuItem::with_id(app, "capture", "立即截图", true, None::<&str>)?;
    let item_show = MenuItem::with_id(app, "show", "打开主窗口", true, None::<&str>)?;
    let item_quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&item_capture, &item_show, &item_quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("缺少窗口图标".into()))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("TrauiOCR —— 按 F4 截图识字")
        .menu(&menu)
        // 左键单击直接拉起截图，比先开菜单少一步
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture" => {
                let _ = spawn_overlay(app);
            }
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击恢复主窗口。截图走右键菜单或 F4，避免误触。
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cfg = config::load();
    let hotkey_str = cfg.hotkey.clone();
    let shortcut = parse_hotkey(&hotkey_str);
    if shortcut.is_none() {
        eprintln!("快捷键 {hotkey_str} 解析失败，请在设置里换一个");
    }

    // 热键插件必须只注册一次：这里同时配置好快捷键与回调
    let mut shortcut_plugin = tauri_plugin_global_shortcut::Builder::new();
    if let Some(sc) = shortcut {
        shortcut_plugin = shortcut_plugin
            .with_shortcut(sc)
            .expect("快捷键格式无效");
    }
    let shortcut_plugin = shortcut_plugin
        .with_handler(|app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let _ = app.emit("capture-stage", "热键已触发");

            // 藏掉主窗口，免得被抓进截图里
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }

            // 抓屏 + 显示遮罩。窗口是预先建好的，这里不新建，
            // 避免在事件处理里创建 webview 造成死锁。
            let app2 = app.clone();
            std::thread::spawn(move || {
                // 等主窗口真正消失，否则会截到它
                std::thread::sleep(std::time::Duration::from_millis(150));
                if let Err(err) = spawn_overlay(&app2) {
                    let _ = app2.emit("capture-error", format!("截图失败: {err}"));
                    if let Some(w) = app2.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            });
        })
        .build();

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(shortcut_plugin)
        .manage(AppState {
            cfg: Mutex::new(cfg),
        })
        .manage(OverlayShot(std::sync::Mutex::new(None)))
        .setup(|app| {
            let handle = app.handle().clone();

            // 托盘常驻，方便确认程序在跑
            if let Err(e) = setup_tray(&handle) {
                eprintln!("创建托盘图标失败: {e}");
            }

            // 点关闭按钮不退出，收进托盘；真正退出走托盘菜单
            if let Some(win) = app.get_webview_window("main") {
                let h = handle.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        hide_main(&h);
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::list_monitors,
            commands::take_overlay_shot,
            commands::submit_selection,
            commands::recognize_region,
            commands::recognize_file,
            commands::recognize_image_bytes,
            commands::test_connection,
            commands::begin_capture,
            commands::end_capture,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
