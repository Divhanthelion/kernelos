//! SSE streaming transport for DeepSeek chat completions.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One parsed SSE payload.
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    Data(Value),
    Done,
}

/// Incremental SSE line parser. Buffers raw bytes and splits on `b'\n'`.
pub struct SseParser {
    buffer: Vec<u8>,
    done: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            done: false,
        }
    }

    /// Feed raw bytes from the response body. Returns newly parsed events.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        if self.done {
            return Vec::new();
        }

        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        loop {
            let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') else {
                break;
            };

            let line_bytes: Vec<u8> = self.buffer.drain(..pos + 1).take(pos).collect();

            // Every complete line is valid UTF-8: 0x0A cannot appear inside a
            // multi-byte UTF-8 sequence.
            let line = match std::str::from_utf8(&line_bytes) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("SSE line UTF-8 decode failure: {e}");
                    continue;
                }
            };

            if let Some(event) = Self::process_line(line) {
                if matches!(event, SseEvent::Done) {
                    self.done = true;
                }
                events.push(event);
            }
        }

        events
    }

    fn process_line(line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return None;
        }
        if line.starts_with(':') {
            return None;
        }
        let data = line.strip_prefix("data: ")?;
        if data == "[DONE]" {
            return Some(SseEvent::Done);
        }
        match serde_json::from_str::<Value>(data) {
            Ok(v) => Some(SseEvent::Data(v)),
            Err(e) => {
                log::warn!("SSE JSON parse failure: {e}: {data}");
                None
            }
        }
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AssistantToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant_with_tools(
        content: Option<String>,
        reasoning_content: Option<String>,
        tool_calls: Vec<AssistantToolCall>,
    ) -> Self {
        Self {
            role: "assistant".into(),
            content,
            reasoning_content,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: AssistantFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Nested thinking control. DeepSeek v4 defaults to thinking enabled; sampling
/// params are silently ignored while thinking is on.
#[derive(Debug, Clone, Serialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

impl ThinkingConfig {
    pub fn enabled() -> Self {
        Self {
            type_: "enabled".into(),
            reasoning_effort: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    pub stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

/// Current DeepSeek chat model. Docs list only `deepseek-v4-flash` and
/// `deepseek-v4-pro`; the legacy `deepseek-chat` id is retired.
pub const DEEPSEEK_MODEL: &str = "deepseek-v4-flash";

impl ChatRequest {
    pub fn streaming(user_content: &str) -> Self {
        Self {
            model: DEEPSEEK_MODEL.into(),
            messages: vec![ChatMessage::user(user_content)],
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
            thinking: Some(ThinkingConfig::enabled()),
            tools: None,
        }
    }

    /// Streaming request with VFS tools. Uses the `/beta` base so `strict: true`
    /// tool schemas are honoured.
    pub fn streaming_with_tools(user_content: &str, tools: Vec<serde_json::Value>) -> Self {
        let mut req = Self::streaming(user_content);
        req.tools = Some(tools);
        req
    }

    pub fn api_url(&self) -> &'static str {
        if self.tools.is_some() {
            DEEPSEEK_BETA_API_URL
        } else {
            DEEPSEEK_API_URL
        }
    }
}

pub const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";
pub const DEEPSEEK_BETA_API_URL: &str = "https://api.deepseek.com/beta/chat/completions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    Http { status: u16, message: String },
    Network(String),
    Aborted,
    BodyRead(String),
}

impl StreamError {
    pub fn user_message(&self) -> &str {
        match self {
            StreamError::Http { message, .. } => message,
            StreamError::Network(msg) => msg,
            StreamError::Aborted => "stream cancelled",
            StreamError::BodyRead(msg) => msg,
        }
    }
}

impl StreamError {
    pub fn from_status(status: u16) -> Self {
        let message = match status {
            401 => "API key is invalid".into(),
            402 => "DeepSeek account is out of credit".into(),
            429 => "rate limited, slow down".into(),
            _ => format!("HTTP {status}"),
        };
        StreamError::Http { status, message }
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn stream_completion(
    api_key: &str,
    request: &ChatRequest,
    abort: &web_sys::AbortController,
    mut on_event: impl FnMut(SseEvent),
) -> Result<(), StreamError> {
    use js_sys::{Reflect, Uint8Array};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Headers, Request, RequestInit, Response};

    let body = serde_json::to_string(request).map_err(|e| StreamError::Network(e.to_string()))?;

    let headers = Headers::new().map_err(|e| StreamError::Network(format!("{e:?}")))?;
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| StreamError::Network(format!("{e:?}")))?;
    headers
        .set("Authorization", &format!("Bearer {api_key}"))
        .map_err(|e| StreamError::Network(format!("{e:?}")))?;

    let init = RequestInit::new();
    init.set_method("POST");
    init.set_headers(&headers);
    init.set_body(&wasm_bindgen::JsValue::from_str(&body));
    init.set_signal(Some(&abort.signal()));

    let window = web_sys::window().ok_or_else(|| StreamError::Network("no window".into()))?;
    let req = Request::new_with_str_and_init(request.api_url(), &init)
        .map_err(|e| StreamError::Network(format!("{e:?}")))?;

    let resp_val = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|e| {
            if abort.signal().aborted() {
                return StreamError::Aborted;
            }
            StreamError::Network(format!("{e:?}"))
        })?;

    if abort.signal().aborted() {
        return Err(StreamError::Aborted);
    }

    let response: Response = resp_val
        .dyn_into()
        .map_err(|_| StreamError::Network("response cast failed".into()))?;

    let status = response.status();
    if status != 200 {
        return Err(StreamError::from_status(status));
    }

    let body = response
        .body()
        .ok_or_else(|| StreamError::BodyRead("response has no body".into()))?;

    let reader = body
        .get_reader()
        .dyn_into::<web_sys::ReadableStreamDefaultReader>()
        .map_err(|_| StreamError::BodyRead("failed to get stream reader".into()))?;

    let mut parser = SseParser::new();

    loop {
        if abort.signal().aborted() {
            return Err(StreamError::Aborted);
        }

        let read_promise = reader.read();
        let chunk_val = JsFuture::from(read_promise)
            .await
            .map_err(|e| {
                if abort.signal().aborted() {
                    StreamError::Aborted
                } else {
                    StreamError::BodyRead(format!("{e:?}"))
                }
            })?;

        let done = Reflect::get(&chunk_val, &"done".into())
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if done {
            break;
        }

        let value = Reflect::get(&chunk_val, &"value".into())
            .map_err(|e| StreamError::BodyRead(format!("{e:?}")))?;

        if value.is_undefined() || value.is_null() {
            continue;
        }

        let array = Uint8Array::from(value);
        let mut bytes = vec![0u8; array.length() as usize];
        array.copy_to(&mut bytes);

        for event in parser.push(&bytes) {
            if matches!(&event, SseEvent::Done) {
                on_event(event);
                return Ok(());
            }
            on_event(event);
        }
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn stream_completion(
    _api_key: &str,
    _request: &ChatRequest,
    _abort: &web_sys::AbortController,
    _on_event: impl FnMut(SseEvent),
) -> Result<(), StreamError> {
    Err(StreamError::Network(
        "stream_completion is only available on wasm32".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_split_across_chunk_boundary() {
        let json = r#"{"choices":[{"delta":{"content":"hello"}}]}"#;
        let event = format!("data: {json}\n\n");
        let bytes = event.as_bytes();
        let split_at = 7;

        let mut parser = SseParser::new();
        let ev1 = parser.push(&bytes[..split_at]);
        assert!(ev1.is_empty());

        let ev2 = parser.push(&bytes[split_at..]);
        assert_eq!(ev2.len(), 1);
        match &ev2[0] {
            SseEvent::Data(v) => {
                assert_eq!(v["choices"][0]["delta"]["content"], "hello");
            }
            other => panic!("expected Data, got {other:?}"),
        }
    }

    #[test]
    fn sse_parser_skips_keep_alive_comment() {
        let mut parser = SseParser::new();
        let data = b": keep-alive\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n";
        let events = parser.push(data);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SseEvent::Data(v) => assert_eq!(v["choices"][0]["delta"]["content"], "x"),
            other => panic!("expected Data, got {other:?}"),
        }
    }

    #[test]
    fn sse_parser_handles_done() {
        let mut parser = SseParser::new();
        let events = parser.push(b"data: [DONE]\n\n");
        assert_eq!(events, vec![SseEvent::Done]);
        assert!(parser.push(b"data: {}\n").is_empty());
    }

    #[test]
    fn malformed_sse_line_logs_warning_and_continues() {
        let mut parser = SseParser::new();
        let data = b"data: {not json}\n\ndata: {\"choices\":[]}\n\n";
        let events = parser.push(data);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], SseEvent::Data(_)));
    }

    #[test]
    fn http_status_messages_are_distinct() {
        assert_eq!(
            StreamError::from_status(401).user_message(),
            "API key is invalid"
        );
        assert_eq!(
            StreamError::from_status(402).user_message(),
            "DeepSeek account is out of credit"
        );
        assert_eq!(
            StreamError::from_status(429).user_message(),
            "rate limited, slow down"
        );
        assert_ne!(
            StreamError::from_status(401).user_message(),
            StreamError::from_status(402).user_message()
        );
    }
}
