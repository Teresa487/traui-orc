//! 屏幕捕获：先截整屏，交给前端框选，再按选区裁剪

use image::{ImageEncoder, RgbaImage};
use std::io::Cursor;

pub struct MonitorInfo {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
}

/// 列出所有显示器。前端用坐标来判断遮罩层该覆盖哪一块。
pub fn list_monitors() -> Result<Vec<MonitorInfo>, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("枚举显示器失败: {e}"))?;
    let mut out = Vec::new();
    for m in monitors {
        out.push(MonitorInfo {
            x: m.x().map_err(|e| format!("读取 x 失败: {e}"))?,
            y: m.y().map_err(|e| format!("读取 y 失败: {e}"))?,
            width: m.width().map_err(|e| format!("读取宽度失败: {e}"))?,
            height: m.height().map_err(|e| format!("读取高度失败: {e}"))?,
            scale_factor: m.scale_factor().map_err(|e| format!("读取缩放失败: {e}"))?,
        });
    }
    if out.is_empty() {
        return Err("没有检测到任何显示器".into());
    }
    Ok(out)
}

/// 找到包含指定桌面坐标的显示器索引。用于「用户在哪块屏幕上按的快捷键」。
pub fn monitor_at(point_x: i32, point_y: i32) -> Result<(usize, MonitorInfo), String> {
    let monitors = list_monitors()?;
    for (i, m) in monitors.iter().enumerate() {
        if point_x >= m.x
            && point_x < m.x + m.width as i32
            && point_y >= m.y
            && point_y < m.y + m.height as i32
        {
            return Ok((i, MonitorInfo {
                x: m.x,
                y: m.y,
                width: m.width,
                height: m.height,
                scale_factor: m.scale_factor,
            }));
        }
    }
    // 落在所有显示器之外（例如负坐标边界），退回第一块
    let m = &monitors[0];
    Ok((0, MonitorInfo {
        x: m.x,
        y: m.y,
        width: m.width,
        height: m.height,
        scale_factor: m.scale_factor,
    }))
}

/// 截取整块显示器，返回 PNG 的 base64（前端用作遮罩层背景）
pub fn capture_monitor_png_base64(index: usize) -> Result<String, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("枚举显示器失败: {e}"))?;
    let m = monitors
        .get(index)
        .ok_or_else(|| format!("显示器索引 {index} 不存在"))?;
    let img = m.capture_image().map_err(|e| format!("截图失败: {e}"))?;
    encode_png_base64(&img)
}

/// 按选区截图。坐标是相对于该显示器的物理像素。
pub fn capture_region_png_base64(
    monitor_index: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    upscale: u32,
) -> Result<String, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("枚举显示器失败: {e}"))?;
    let m = monitors
        .get(monitor_index)
        .ok_or_else(|| format!("显示器索引 {monitor_index} 不存在"))?;

    let mon_w = m.width().map_err(|e| format!("读取宽度失败: {e}"))?;
    let mon_h = m.height().map_err(|e| format!("读取高度失败: {e}"))?;

    // 夹紧到屏幕内，避免越界报错
    let x = x.min(mon_w.saturating_sub(1));
    let y = y.min(mon_h.saturating_sub(1));
    let width = width.min(mon_w - x).max(1);
    let height = height.min(mon_h - y).max(1);

    let img = m
        .capture_region(x, y, width, height)
        .map_err(|e| format!("区域截图失败: {e}"))?;

    let img = if upscale > 1 {
        image::imageops::resize(
            &img,
            width * upscale,
            height * upscale,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    encode_png_base64(&img)
}

fn encode_png_base64(img: &RgbaImage) -> Result<String, String> {
    let mut buf = Cursor::new(Vec::new());
    img.write_with_encoder(image::codecs::png::PngEncoder::new(&mut buf))
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    Ok(base64_encode(buf.get_ref()))
}

/// 把 PNG base64 转成 JPEG base64 —— 发给 API 用，体积小得多
pub fn png_base64_to_jpeg_base64(png_b64: &str, quality: u8) -> Result<String, String> {
    let raw = base64_decode(png_b64)?;
    let img = image::load_from_memory(&raw).map_err(|e| format!("解码图片失败: {e}"))?;
    let rgb = img.to_rgb8();

    let mut buf = Cursor::new(Vec::new());
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
    enc.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )
    .map_err(|e| format!("JPEG 编码失败: {e}"))?;

    Ok(base64_encode(buf.get_ref()))
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| format!("base64 解码失败: {e}"))
}
