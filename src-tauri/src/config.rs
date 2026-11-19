//! 配置持久化：存到用户配置目录下的 config.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("TrauiOCR").join("config.json")
}

/// 一个可选的提示词模式，对应天若 OCR 的 OCRModes.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrMode {
    pub mode: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
}

fn default_temperature() -> f64 {
    0.1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// API 根地址，例如 https://api.mistral.ai/v1
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    /// 触发截图的全局快捷键，Tauri accelerator 语法
    pub hotkey: String,
    /// 当前选中的模式名
    pub active_mode: String,
    pub modes: Vec<OcrMode>,
    /// 识别完成后自动复制到剪贴板
    pub auto_copy: bool,
    /// 截图时额外放大的倍数，便于识别小字
    pub upscale: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_base: "https://api.mistral.ai/v1".into(),
            api_key: String::new(),
            model: "ministral-14b-latest".into(),
            hotkey: "F4".into(),
            active_mode: "文字提取".into(),
            modes: builtin_modes(),
            auto_copy: true,
            upscale: 1,
        }
    }
}

/// 只做一件事：把图片里的文字提取出来
pub fn builtin_modes() -> Vec<OcrMode> {
    vec![OcrMode {
        mode: "文字提取".into(),
        description: "把图片里的文字转成可复制的文本".into(),
        system_prompt: "你是一个 OCR 引擎。你的唯一任务是识别图片中的文字并原样输出。\n规则：\n1. 只输出识别到的文字，不要任何开场白、解释或总结。\n2. 不要用 Markdown 代码块包裹结果。\n3. 保持图片中原始的换行和段落结构。\n4. 看不清的字用 □ 代替，不要猜测。".into(),
        prompt: "识别图片中的文字：".into(),
        temperature: 0.1,
    }]
}

pub fn load() -> AppConfig {
    let p = config_path();
    match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            eprintln!("配置解析失败，使用默认值: {e}");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    }
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let s = serde_json::to_string_pretty(cfg).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(&p, s).map_err(|e| format!("写入配置失败: {e}"))?;
    Ok(())
}
