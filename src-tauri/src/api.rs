//! OpenAI 兼容的视觉 API 客户端
//!
//! 只需一次完整响应，不走 SSE 流式：OCR 结果是几行字，
//! 分片传输只会增加复杂度和出错面。

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message>,
    temperature: f64,
}

#[derive(Debug, Serialize)]
struct Message {
    role: &'static str,
    content: Content,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Parts(Vec<Part>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum Part {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<RespMessage>,
}

#[derive(Debug, Deserialize)]
struct RespMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: Option<ErrDetail>,
    message: Option<String>,
    detail: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrDetail {
    message: Option<String>,
}

/// 组装请求体
fn build_body<'a>(
    model: &'a str,
    system_prompt: &str,
    user_prompt: &str,
    jpeg_b64: &str,
    temperature: f64,
) -> ChatRequest<'a> {
    let mut messages = Vec::new();

    if !system_prompt.trim().is_empty() {
        messages.push(Message {
            role: "system",
            content: Content::Text(system_prompt.to_string()),
        });
    }

    // user 消息用 parts 数组，图文混排
    let mut parts = Vec::new();
    if !user_prompt.trim().is_empty() {
        parts.push(Part::Text {
            text: user_prompt.to_string(),
        });
    }
    // 图片必须挂上，否则没得识别
    parts.push(Part::ImageUrl {
        image_url: ImageUrl {
            url: format!("data:image/jpeg;base64,{jpeg_b64}"),
        },
    });

    messages.push(Message {
        role: "user",
        content: Content::Parts(parts),
    });

    ChatRequest {
        model,
        messages,
        temperature,
    }
}

fn endpoint(api_base: &str) -> String {
    let base = api_base.trim_end_matches('/');
    // 允许用户填到 /v1 或直接填完整地址
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

/// 从错误响应体里挖出人类能看懂的消息
fn extract_error(status: u16, body: &str) -> String {
    if let Ok(e) = serde_json::from_str::<ApiError>(body) {
        if let Some(d) = e.error.and_then(|x| x.message) {
            return format!("API 错误 {status}: {d}");
        }
        if let Some(m) = e.message {
            return format!("API 错误 {status}: {m}");
        }
        if let Some(d) = e.detail {
            return format!("API 错误 {status}: {d}");
        }
    }
    let snippet: String = body.chars().take(300).collect();
    format!("API 错误 {status}: {snippet}")
}

/// 非流式调用，一次性拿完整结果
pub async fn ocr_once(
    api_base: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    jpeg_b64: &str,
    temperature: f64,
) -> Result<String, String> {
    let body = build_body(model, system_prompt, user_prompt, jpeg_b64, temperature);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let resp = client
        .post(endpoint(api_base))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;

    let status = resp.status().as_u16();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败: {e}"))?;

    if status != 200 {
        return Err(extract_error(status, &text));
    }

    let parsed: ChatResponse =
        serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {e}\n原文: {}", &text[..text.len().min(300)]))?;

    let content = parsed
        .choices
        .first()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.clone())
        .unwrap_or_default();

    if content.trim().is_empty() {
        return Err("API 返回了空内容".into());
    }
    Ok(content)
}

/// 连通性自检：发一条最小纯文本请求，验证 key 和地址
pub async fn ping(api_base: &str, api_key: &str, model: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 4
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let resp = client
        .post(endpoint(api_base))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;

    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status != 200 {
        return Err(extract_error(status, &text));
    }
    Ok(format!("连接正常（HTTP {status}），模型 {model} 可用"))
}
