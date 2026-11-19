//! Tauri 命令层：前端通过 invoke 调这些

use crate::api;
use crate::capture;
use crate::config::{self, AppConfig};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use std::sync::Mutex;

pub struct AppState {
    pub cfg: Mutex<AppConfig>,
}

#[derive(Debug, Serialize)]
pub struct MonitorDto {
    pub index: usize,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppConfig {
    state.cfg.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_config(state: State<'_, AppState>, cfg: AppConfig) -> Result<(), String> {
    config::save(&cfg)?;
    *state.cfg.lock().unwrap() = cfg;
    Ok(())
}

#[tauri::command]
pub fn list_monitors() -> Result<Vec<MonitorDto>, String> {
    let mons = capture::list_monitors()?;
    Ok(mons
        .into_iter()
        .enumerate()
        .map(|(index, m)| MonitorDto {
            index,
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
            scale_factor: m.scale_factor,
        })
        .collect())
}

/// 取出热键触发时预存的屏幕截图，供遮罩层当背景。
/// 返回 (显示器索引, PNG base64)。
///
/// 这里用 clone 而不是 take：裁剪阶段还要用同一张图，
/// 取走的话识别就会报「截图已失效」。
#[tauri::command]
pub fn take_overlay_shot(
    state: State<'_, crate::OverlayShot>,
) -> Result<(usize, String), String> {
    let guard = state.0.lock().unwrap();
    let data = guard.as_ref().ok_or("没有待用的屏幕截图")?;
    Ok((data.monitor, data.png_b64.clone()))
}

/// 按选区截图并识别。
/// crop 参数是相对该显示器的物理像素坐标。
#[tauri::command]
pub async fn recognize_region(
    state: State<'_, AppState>,
    shot: State<'_, crate::OverlayShot>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let (cfg, mode) = {
        let g = state.cfg.lock().unwrap();
        let mode = g
            .modes
            .iter()
            .find(|m| m.mode == g.active_mode)
            .cloned()
            .or_else(|| g.modes.first().cloned())
            .ok_or("没有可用的识别模式")?;
        (g.clone(), mode)
    };

    if cfg.api_key.trim().is_empty() {
        return Err("还没填 API Key，请先在设置里配置".into());
    }
    if width < 4 || height < 4 {
        return Err("选区太小了".into());
    }

    // 1. 从热键触发时存下的整屏图里裁剪。
    //    不能重新截屏 —— 遮罩此刻正盖在屏幕上，重新截只能截到遮罩。
    let full_png = {
        let guard = shot.0.lock().unwrap();
        guard
            .as_ref()
            .map(|d| d.png_b64.clone())
            .ok_or("截图已失效，请重新按快捷键")?
    };
    let png_b64 = capture::crop_png_base64(&full_png, x, y, width, height, cfg.upscale)?;

    // 2. 转成 JPEG，体积小很多
    let jpeg_b64 = capture::png_base64_to_jpeg_base64(&png_b64, 92)?;

    // 3. 调 API，一次拿完整结果
    api::ocr_once(
        &cfg.api_base,
        &cfg.api_key,
        &cfg.model,
        &mode.system_prompt,
        &mode.prompt,
        &jpeg_b64,
        mode.temperature,
    )
    .await
}

/// 直接从剪贴板里的图片识别（暂不支持二进制剪贴板，留作文件导入）
#[tauri::command]
pub async fn recognize_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let (cfg, mode) = {
        let g = state.cfg.lock().unwrap();
        let mode = g
            .modes
            .iter()
            .find(|m| m.mode == g.active_mode)
            .cloned()
            .or_else(|| g.modes.first().cloned())
            .ok_or("没有可用的识别模式")?;
        (g.clone(), mode)
    };

    if cfg.api_key.trim().is_empty() {
        return Err("还没填 API Key，请先在设置里配置".into());
    }

    let raw = std::fs::read(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    use base64::Engine;
    let png_b64 = base64::engine::general_purpose::STANDARD.encode(&raw);
    let jpeg_b64 = capture::png_base64_to_jpeg_base64(&png_b64, 92)?;

    api::ocr_once(
        &cfg.api_base,
        &cfg.api_key,
        &cfg.model,
        &mode.system_prompt,
        &mode.prompt,
        &jpeg_b64,
        mode.temperature,
    )
    .await
}

/// 遮罩框选完成后调用：把选区坐标转给主窗口，由主窗口去识别和显示。
///
/// 结果不能留在遮罩里 —— 它识别完就隐藏了，没人显示结果。
#[tauri::command]
pub fn submit_selection(
    app: AppHandle,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    // 先收起遮罩，否则它会一直盖在主窗口上面
    if let Some(o) = app.get_webview_window("overlay") {
        let _ = o.hide();
    }
    // 再恢复主窗口，用户才看得到进度
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    app.emit(
        "selection",
        serde_json::json!({ "x": x, "y": y, "width": width, "height": height }),
    )
    .map_err(|e| format!("通知主窗口失败: {e}"))?;
    Ok(())
}

/// 从前端传上来的图片字节直接识别（浏览器拿不到真实文件路径）
#[tauri::command]
pub async fn recognize_image_bytes(
    state: State<'_, AppState>,
    data: Vec<u8>,
    filename: String,
) -> Result<String, String> {
    let (cfg, mode) = {
        let g = state.cfg.lock().unwrap();
        let mode = g
            .modes
            .iter()
            .find(|m| m.mode == g.active_mode)
            .cloned()
            .or_else(|| g.modes.first().cloned())
            .ok_or("没有可用的识别模式")?;
        (g.clone(), mode)
    };

    if cfg.api_key.trim().is_empty() {
        return Err("还没填 API Key，请先在设置里配置".into());
    }
    if data.is_empty() {
        return Err(format!("文件 {filename} 是空的"));
    }

    use base64::Engine;
    let png_b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    let jpeg_b64 =
        capture::png_base64_to_jpeg_base64(&png_b64, 92).map_err(|e| format!("{filename}: {e}"))?;

    api::ocr_once(
        &cfg.api_base,
        &cfg.api_key,
        &cfg.model,
        &mode.system_prompt,
        &mode.prompt,
        &jpeg_b64,
        mode.temperature,
    )
    .await
}

/// 连通性自检
#[tauri::command]
pub async fn test_connection(api_base: String, api_key: String, model: String) -> Result<String, String> {
    api::ping(&api_base, &api_key, &model).await
}

/// 开始截图流程：窗口隐藏 → 前端拉起遮罩
#[tauri::command]
pub fn begin_capture(app: AppHandle) -> Result<(), String> {
    // 主窗口先藏起来，不然会挡着自己
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    let _ = app.emit("begin-capture", ());
    Ok(())
}

/// 框选完成后恢复主窗口
#[tauri::command]
pub fn end_capture(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
