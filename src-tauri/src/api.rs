//! OpenAI 兼容的视觉 API 客户端
//!
//! 支持非流式与 SSE 流式。流式走 `choices[0].delta.content`，
//! 非流式走 `choices[0].message.content` —— 两者字段名不同，写错取不到内容。

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message>,
    temperature: f64,
    stream: bool,
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
    delta: Option<RespDelta>,
}

#[derive(Debug, Deserialize)]
struct RespMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RespDelta {
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
    stream: bool,
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
        stream,
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
    let body = build_body(model, system_prompt, user_prompt, jpeg_b64, temperature, false);

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

/// 流式调用。每收到一段文本就回调一次，实现边识别边显示。
pub async fn ocr_stream<F>(
    api_base: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    jpeg_b64: &str,
    temperature: f64,
    mut on_chunk: F,
) -> Result<String, String>
where
    F: FnMut(&str),
{
    let body = build_body(model, system_prompt, user_prompt, jpeg_b64, temperature, true);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
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
    if status != 200 {
        let text = resp.text().await.unwrap_or_default();
        return Err(extract_error(status, &text));
    }

    let mut full = String::new();
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取流失败: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // 按行切，SSE 每条消息以换行分隔
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(full);
            }
            // 有些服务会夹带心跳或空对象，解析失败直接跳过
            let Ok(v) = serde_json::from_str::<ChatResponse>(data) else {
                continue;
            };
            if let Some(delta) = v
                .choices
                .first()
                .and_then(|c| c.delta.as_ref())
                .and_then(|d| d.content.as_ref())
            {
                if !delta.is_empty() {
                    full.push_str(delta);
                    on_chunk(delta);
                }
            }
        }
    }

    if full.trim().is_empty() {
        return Err("流式响应结束但没有收到任何内容".into());
    }
    Ok(full)
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
