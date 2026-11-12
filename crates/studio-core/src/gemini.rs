//! Client for Google's Gemini image models, using the Interactions API
//! (`POST /v1beta/interactions`).
//!
//! The rest of the app talks to [`ImageGenerator`], so a model or API change
//! only touches this file.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Serialize;
use serde_json::{json, Value};

use crate::reference::ReferenceImage;

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Delays before the second and third attempt at a failed request.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(3), Duration::from_secs(10)];

#[derive(Debug, Clone)]
pub struct GenerateRequest {
    pub model: String,
    pub prompt: String,
    /// Reference photos of the item. The first one sets the viewing angle.
    pub references: Vec<Arc<ReferenceImage>>,
    /// e.g. "1:1". `None` lets the model follow the reference photo.
    pub aspect_ratio: Option<String>,
    /// e.g. "2K". `None` uses the model's default.
    pub image_size: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GeneratedImage {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

impl GeneratedImage {
    pub fn extension(&self) -> &'static str {
        match self.mime_type.as_str() {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct GenerateOutput {
    pub image: GeneratedImage,
    /// Any text the model returned next to the image.
    pub notes: Option<String>,
    pub usage: Usage,
}

#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("could not reach Gemini: {0}")]
    Network(String),
    #[error("Gemini refused the request (HTTP {status}): {message}")]
    Api { status: u16, message: String },
    #[error("Gemini returned no image: {reason}")]
    NoImage {
        reason: String,
        /// The response with image data removed, for troubleshooting.
        response: String,
    },
    #[error("could not understand Gemini's response: {0}")]
    BadResponse(String),
}

impl GenerateError {
    /// Whether trying the same request again could succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) | Self::NoImage { .. } => true,
            Self::Api { status, .. } => matches!(status, 429 | 500 | 502 | 503 | 504),
            Self::BadResponse(_) => false,
        }
    }
}

/// Anything that can turn a prompt plus reference photos into one image.
pub trait ImageGenerator: Send + Sync {
    fn generate(
        &self,
        request: &GenerateRequest,
    ) -> impl Future<Output = Result<GenerateOutput, GenerateError>> + Send;
}

/// Holds the API key, so it deliberately does not implement `Debug`.
#[derive(Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl GeminiClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self, GenerateError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            // Image generation routinely takes over a minute.
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(network_error)?;
        Ok(Self {
            http,
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    /// Sends requests somewhere other than Google, such as a local test
    /// server or a company proxy.
    pub fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// Model ids this key can use. Costs nothing, so it doubles as a key check.
    pub async fn list_models(&self) -> Result<Vec<String>, GenerateError> {
        let url = format!("{}/models?pageSize=1000", self.base_url);
        let body = self.send(self.http.get(url)).await?;
        let value: Value =
            serde_json::from_str(&body).map_err(|e| GenerateError::BadResponse(e.to_string()))?;
        let mut names: Vec<String> = value
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|model| model.get("name").and_then(Value::as_str))
            .map(|name| name.trim_start_matches("models/").to_string())
            .collect();
        names.sort();
        Ok(names)
    }

    async fn generate_once(
        &self,
        request: &GenerateRequest,
    ) -> Result<GenerateOutput, GenerateError> {
        let url = format!("{}/interactions", self.base_url);
        let body = self
            .send(self.http.post(url).json(&request_body(request)))
            .await?;
        let value: Value =
            serde_json::from_str(&body).map_err(|e| GenerateError::BadResponse(e.to_string()))?;
        parse_interaction(value)
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<String, GenerateError> {
        let response = request
            .header("x-goog-api-key", self.api_key.as_str())
            .send()
            .await
            .map_err(network_error)?;
        let status = response.status();
        let body = response.text().await.map_err(network_error)?;
        if !status.is_success() {
            return Err(GenerateError::Api {
                status: status.as_u16(),
                message: api_error_message(&body),
            });
        }
        Ok(body)
    }
}

impl ImageGenerator for GeminiClient {
    async fn generate(&self, request: &GenerateRequest) -> Result<GenerateOutput, GenerateError> {
        let mut delays = RETRY_DELAYS.iter();
        loop {
            let error = match self.generate_once(request).await {
                Ok(output) => return Ok(output),
                Err(error) => error,
            };
            match delays.next() {
                Some(delay) if error.is_retryable() => tokio::time::sleep(*delay).await,
                _ => return Err(error),
            }
        }
    }
}

fn request_body(request: &GenerateRequest) -> Value {
    let mut input = vec![json!({ "type": "text", "text": request.prompt })];
    for reference in &request.references {
        input.push(json!({
            "type": "image",
            "mime_type": reference.mime_type,
            "data": BASE64.encode(&reference.bytes),
        }));
    }

    let mut body = json!({
        "model": request.model,
        "input": input,
        // Do not keep the retailer's photos on Google's side after the call.
        "store": false,
    });
    if request.aspect_ratio.is_some() || request.image_size.is_some() {
        let mut format = json!({ "type": "image" });
        if let Some(aspect_ratio) = &request.aspect_ratio {
            format["aspect_ratio"] = json!(aspect_ratio);
        }
        if let Some(image_size) = &request.image_size {
            format["image_size"] = json!(image_size);
        }
        body["response_format"] = format;
    }
    body
}

/// Pulls the final image out of an Interactions response. Interim "thought"
/// images live in steps of another type and are ignored.
fn parse_interaction(mut body: Value) -> Result<GenerateOutput, GenerateError> {
    let mut image = None;
    let mut notes = Vec::new();

    let steps = body.get("steps").and_then(Value::as_array);
    for step in steps.into_iter().flatten() {
        if step.get("type").and_then(Value::as_str) != Some("model_output") {
            continue;
        }
        let content = step.get("content").and_then(Value::as_array);
        for block in content.into_iter().flatten() {
            match block.get("type").and_then(Value::as_str) {
                Some("image") => {
                    let Some(data) = block.get("data").and_then(Value::as_str) else {
                        continue;
                    };
                    let bytes = BASE64.decode(data.trim()).map_err(|e| {
                        GenerateError::BadResponse(format!("image data is not valid base64: {e}"))
                    })?;
                    let mime_type = block
                        .get("mime_type")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| sniff_mime_type(&bytes).to_owned());
                    // If the model returns several, the last is the final render.
                    image = Some(GeneratedImage { mime_type, bytes });
                }
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        notes.push(text.trim().to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    notes.retain(|note| !note.is_empty());

    let Some(image) = image else {
        let reason = no_image_reason(&body, &notes);
        redact_image_data(&mut body);
        let response = serde_json::to_string_pretty(&body).unwrap_or_default();
        return Err(GenerateError::NoImage { reason, response });
    };

    let usage = body.get("usage");
    let tokens = |field: &str| {
        usage
            .and_then(|usage| usage.get(field))
            .and_then(Value::as_u64)
    };
    Ok(GenerateOutput {
        image,
        notes: (!notes.is_empty()).then(|| notes.join("\n")),
        usage: Usage {
            input_tokens: tokens("total_input_tokens"),
            output_tokens: tokens("total_output_tokens"),
            total_tokens: tokens("total_tokens"),
        },
    })
}

fn no_image_reason(body: &Value, notes: &[String]) -> String {
    let status = body
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut reason = format!("status \"{status}\"");
    let errors = body.get("errors").and_then(Value::as_array);
    for error in errors.into_iter().flatten() {
        if let Some(message) = error.get("message").and_then(Value::as_str) {
            reason.push_str(&format!("; {message}"));
        }
    }
    if let Some(note) = notes.first() {
        reason.push_str(&format!("; the model said: {}", truncate(note, 300)));
    }
    reason
}

/// Google's error bodies look like `{"error": {"message": "...", "status": "..."}}`.
/// Anything else is passed through, shortened.
fn api_error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let error = parsed.as_ref().and_then(|value| value.get("error"));
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str);
    match message {
        Some(message) => {
            let status = error
                .and_then(|error| error.get("status"))
                .and_then(Value::as_str);
            match status {
                Some(status) => format!("{message} [{status}]"),
                None => message.to_owned(),
            }
        }
        None if body.trim().is_empty() => "no details given".to_owned(),
        None => truncate(body.trim(), 500),
    }
}

fn redact_image_data(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, inner) in fields.iter_mut() {
                let long_data = (key == "data")
                    .then(|| inner.as_str().map(str::len))
                    .flatten()
                    .filter(|length| *length > 256);
                match long_data {
                    Some(length) => {
                        *inner = Value::String(format!("<{length} base64 characters removed>"));
                    }
                    None => redact_image_data(inner),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_image_data),
        _ => {}
    }
}

fn sniff_mime_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/png"
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let shortened: String = text.chars().take(max_chars).collect();
    format!("{shortened}...")
}

fn network_error(error: reqwest::Error) -> GenerateError {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(inner) = source {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        source = inner.source();
    }
    GenerateError::Network(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> Arc<ReferenceImage> {
        Arc::new(ReferenceImage {
            mime_type: "image/jpeg".into(),
            bytes: vec![0xFF, 0xD8, 0x01],
            width: 1,
            height: 1,
        })
    }

    fn request() -> GenerateRequest {
        GenerateRequest {
            model: "test-model".into(),
            prompt: "the prompt".into(),
            references: vec![reference(), reference()],
            aspect_ratio: None,
            image_size: Some("2K".into()),
        }
    }

    #[test]
    fn request_body_matches_the_interactions_api() {
        let body = request_body(&request());
        assert_eq!(
            body,
            json!({
                "model": "test-model",
                "input": [
                    { "type": "text", "text": "the prompt" },
                    { "type": "image", "mime_type": "image/jpeg", "data": "/9gB" },
                    { "type": "image", "mime_type": "image/jpeg", "data": "/9gB" },
                ],
                "store": false,
                "response_format": { "type": "image", "image_size": "2K" },
            })
        );
    }

    #[test]
    fn response_format_is_left_out_when_nothing_is_requested() {
        let mut request = request();
        request.image_size = None;
        assert!(request_body(&request).get("response_format").is_none());

        request.aspect_ratio = Some("1:1".into());
        assert_eq!(
            request_body(&request)["response_format"],
            json!({ "type": "image", "aspect_ratio": "1:1" })
        );
    }

    #[test]
    fn final_image_is_taken_from_the_model_output_step() {
        let png = [0x89, b'P', b'N', b'G'];
        let response = json!({
            "id": "abc",
            "status": "completed",
            "steps": [
                { "type": "thought", "content": [
                    { "type": "image", "mime_type": "image/png", "data": BASE64.encode([1, 2, 3]) }
                ]},
                { "type": "model_output", "content": [
                    { "type": "text", "text": " Here is your image. " },
                    { "type": "image", "mime_type": "image/png", "data": BASE64.encode(png) }
                ]}
            ],
            "usage": { "total_input_tokens": 600, "total_output_tokens": 1680, "total_tokens": 2280 }
        });

        let output = parse_interaction(response).unwrap();
        assert_eq!(output.image.bytes, png);
        assert_eq!(output.image.extension(), "png");
        assert_eq!(output.notes.as_deref(), Some("Here is your image."));
        assert_eq!(output.usage.output_tokens, Some(1680));
        assert_eq!(output.usage.total_tokens, Some(2280));
    }

    #[test]
    fn missing_mime_type_is_detected_from_the_bytes() {
        let response = json!({
            "status": "completed",
            "steps": [{ "type": "model_output", "content": [
                { "type": "image", "data": BASE64.encode([0xFF, 0xD8, 0xFF]) }
            ]}]
        });
        let output = parse_interaction(response).unwrap();
        assert_eq!(output.image.mime_type, "image/jpeg");
        assert_eq!(output.image.extension(), "jpg");
    }

    #[test]
    fn a_response_without_an_image_explains_why_and_hides_image_data() {
        let response = json!({
            "status": "failed",
            "errors": [{ "code": "blocked", "message": "Blocked by safety filters" }],
            "steps": [
                { "type": "thought", "content": [
                    { "type": "image", "data": "A".repeat(1000) }
                ]},
                { "type": "model_output", "content": [
                    { "type": "text", "text": "I cannot create that." }
                ]}
            ]
        });

        let error = parse_interaction(response).unwrap_err();
        let GenerateError::NoImage { reason, response } = &error else {
            panic!("expected NoImage, got {error:?}");
        };
        assert!(reason.contains("status \"failed\""), "{reason}");
        assert!(reason.contains("Blocked by safety filters"), "{reason}");
        assert!(reason.contains("I cannot create that."), "{reason}");
        assert!(response.contains("<1000 base64 characters removed>"));
        assert!(!response.contains("AAAAAAAA"));
        assert!(error.is_retryable());
    }

    #[test]
    fn broken_image_data_is_a_bad_response() {
        let response = json!({
            "steps": [{ "type": "model_output", "content": [
                { "type": "image", "data": "not base64!" }
            ]}]
        });
        let error = parse_interaction(response).unwrap_err();
        assert!(matches!(error, GenerateError::BadResponse(_)));
        assert!(!error.is_retryable());
    }

    #[test]
    fn api_errors_show_googles_message() {
        let body = r#"{"error": {"code": 400, "message": "API key not valid.", "status": "INVALID_ARGUMENT"}}"#;
        assert_eq!(
            api_error_message(body),
            "API key not valid. [INVALID_ARGUMENT]"
        );
        assert_eq!(
            api_error_message("<html>Bad gateway</html>"),
            "<html>Bad gateway</html>"
        );
        assert_eq!(api_error_message("  "), "no details given");
    }

    #[test]
    fn only_temporary_failures_are_retried() {
        let api = |status| GenerateError::Api {
            status,
            message: String::new(),
        };
        assert!(api(429).is_retryable());
        assert!(api(503).is_retryable());
        assert!(!api(400).is_retryable());
        assert!(!api(403).is_retryable());
    }
}
