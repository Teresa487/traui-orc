//! TrauiOCR —— 截图 OCR 客户端
//!
//! 流程：全局热键 → 隐藏主窗 → 截整屏 → 全屏遮罩框选 → 裁剪 → 调 API → 显示结果

mod api;
mod capture;
mod commands;
mod config;

use commands::AppState;
use std::sync::Mutex;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
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

/// 拉起全屏遮罩窗口。已经存在就复用。
///
/// 关键点：屏幕内容必须在遮罩出现**之前**截下来，否则截到的是遮罩本身。
/// 所以这里先截屏存进 OverlayShot，前端再从命令里把图取走当背景。
fn spawn_overlay(app: &tauri::AppHandle) -> Result<(), String> {
    // 先定位要覆盖哪块屏幕
    let (idx, m) = capture::monitor_at(0, 0)?;

    // 遮罩还没出现，此刻截屏拿到的才是真实画面
    let shot = capture::capture_monitor_png_base64(idx)?;
    {
        let state = app.state::<OverlayShot>();
        *state.0.lock().unwrap() = Some(OverlayShotData { monitor: idx, png_b64: shot });
    }

    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.show();
        let _ = w.set_focus();
        let _ = app.emit("begin-capture", ());
        return Ok(());
    }

    let win = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
        .title("选择区域")
        .inner_size(m.width as f64, m.height as f64)
        .position(m.x as f64, m.y as f64)
        .decorations(false)
        .transparent(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(true)
        .build()
        .map_err(|e| format!("创建遮罩窗口失败: {e}"))?;

    let _ = win.set_focus();
    let app2 = app.clone();
    // 等遮罩真正挂载后再通知它取图
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(180));
        let _ = app2.emit("begin-capture", ());
    });

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
            // 先藏主窗口，再把遮罩拉起来
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
            if let Err(err) = spawn_overlay(app) {
                eprintln!("拉起遮罩失败: {err}");
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                }
            }
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
        .setup(|_app| Ok(()))
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::list_monitors,
            commands::take_overlay_shot,
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
