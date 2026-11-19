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
    /// 是否使用流式响应（边识别边显示）
    pub stream: bool,
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
            active_mode: "文本识别".into(),
            modes: builtin_modes(),
            stream: false,
            auto_copy: true,
            upscale: 1,
        }
    }
}

/// 内置模式，照搬天若 OCR 那 6 套提示词思路
pub fn builtin_modes() -> Vec<OcrMode> {
    vec![
        OcrMode {
            mode: "文本识别".into(),
            description: "通用模式，保留原格式".into(),
            system_prompt: "你是一个专业的 OCR 引擎。任务是识别图片中的文字。请严格遵守：\n1. 直接输出识别到的内容，严禁输出“好的”“识别结果如下”等任何废话。\n2. 严禁使用 Markdown 代码块包裹内容，除非图片原文里真的有。\n3. 尽量保持原始的换行和段落格式。".into(),
            prompt: "请识别这张图片中的文字：".into(),
            temperature: 0.1,
        },
        OcrMode {
            mode: "公式识别 (LaTeX)".into(),
            description: "把数学公式转成 LaTeX 源码".into(),
            system_prompt: "你是一个数学公式 OCR 引擎。只输出 LaTeX 代码，严禁使用 ``` 代码块。行内公式用 $ 包裹，独立公式用 $$ 包裹。".into(),
            prompt: "将图片中的数学公式转换为 LaTeX 格式：".into(),
            temperature: 0.1,
        },
        OcrMode {
            mode: "表格识别 (Markdown)".into(),
            description: "输出 Markdown 表格".into(),
            system_prompt: "你是一个表格结构化专家。直接输出 Markdown 表格源码，严禁添加代码块标记，不要任何解释性文字。".into(),
            prompt: "请将图片中的表格转换为 Markdown 格式：".into(),
            temperature: 0.1,
        },
        OcrMode {
            mode: "表格识别 (CSV)".into(),
            description: "输出 CSV，可直接贴进 Excel".into(),
            system_prompt: "你是一个数据处理专家。把图片表格转成 CSV（英文逗号分隔），严禁用代码块包裹，直接输出数据流。".into(),
            prompt: "请将图片表格转换为 CSV 数据格式：".into(),
            temperature: 0.1,
        },
        OcrMode {
            mode: "图片翻译".into(),
            description: "识别并翻译，中英互译".into(),
            system_prompt: "你是一个资深翻译引擎。自动检测语言：中文译成英文，英文译成中文。只输出翻译结果，不要原文，不要解释。".into(),
            prompt: "请翻译图片中的文字内容：".into(),
            temperature: 0.5,
        },
        OcrMode {
            mode: "图片总结".into(),
            description: "分析图片，提炼核心信息".into(),
            system_prompt: "你是一个视觉内容分析师。用简练语言概括图片核心内容，直接输出总结段落，不要“这张图片展示了”之类起手式。".into(),
            prompt: "请简要总结这张图片的核心内容：".into(),
            temperature: 0.5,
        },
    ]
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
