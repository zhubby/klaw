use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use ipnet::IpNet;
use klaw_config::AppConfig;
use reqwest::Url;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

struct CacheEntry {
    value: Value,
    expires_at: Instant,
}

pub struct WebFetchTool {
    max_chars: usize,
    timeout: Duration,
    cache_ttl: Duration,
    max_redirects: u8,
    readability: bool,
    ssrf_allowlist: Vec<IpNet>,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl WebFetchTool {
    pub fn new(config: &AppConfig) -> Self {
        let cfg = &config.tools.web_fetch;
        let ssrf_allowlist = cfg
            .ssrf_allowlist
            .iter()
            .filter_map(|s| s.parse::<IpNet>().ok())
            .collect();

        Self {
            max_chars: cfg.max_chars,
            timeout: Duration::from_secs(cfg.timeout_seconds),
            cache_ttl: Duration::from_secs(cfg.cache_ttl_minutes.saturating_mul(60)),
            max_redirects: cfg.max_redirects,
            readability: cfg.readability,
            ssrf_allowlist,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cache_get(&self, key: &str) -> Option<Value> {
        let cache = self.cache.lock().ok()?;
        let entry = cache.get(key)?;
        if Instant::now() < entry.expires_at {
            Some(entry.value.clone())
        } else {
            None
        }
    }

    fn cache_set(&self, key: String, value: Value) {
        if self.cache_ttl.is_zero() {
            return;
        }
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() > 100 {
                let now = Instant::now();
                cache.retain(|_, entry| entry.expires_at > now);
            }
            cache.insert(
                key,
                CacheEntry {
                    value,
                    expires_at: Instant::now() + self.cache_ttl,
                },
            );
        }
    }

    async fn fetch_url(
        &self,
        url_str: &str,
        extract_mode: &str,
        max_chars: usize,
        accept_language: Option<&str>,
    ) -> Result<Value, ToolError> {
        let mut current_url = Url::parse(url_str)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid `url`: {err}")))?;

        match current_url.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(ToolError::InvalidArgs(format!(
                    "unsupported URL scheme: {scheme}"
                )));
            }
        }

        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| ToolError::ExecutionFailed(format!("build client failed: {err}")))?;

        let mut visited = Vec::new();
        let mut hops = 0u8;

        loop {
            ssrf_check(&current_url, &self.ssrf_allowlist).await?;
            visited.push(current_url.to_string());

            let mut req = client.get(current_url.as_str());
            if let Some(lang) = accept_language {
                req = req.header("Accept-Language", lang);
            }

            let resp = req
                .send()
                .await
                .map_err(|err| ToolError::ExecutionFailed(format!("request failed: {err}")))?;
            let status = resp.status();

            if status.is_redirection() {
                if hops >= self.max_redirects {
                    return Err(ToolError::ExecutionFailed(format!(
                        "too many redirects ({} hops, max {})",
                        hops + 1,
                        self.max_redirects
                    )));
                }

                let location = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        ToolError::ExecutionFailed("redirect without Location header".to_string())
                    })?;

                let next = current_url.join(location).map_err(|err| {
                    ToolError::ExecutionFailed(format!("invalid redirect location: {err}"))
                })?;

                if visited.iter().any(|u| u == next.as_str()) {
                    return Err(ToolError::ExecutionFailed(format!(
                        "redirect loop detected: {} -> {}",
                        current_url, next
                    )));
                }

                current_url = next;
                hops = hops.saturating_add(1);
                continue;
            }

            if !status.is_success() {
                return Ok(json!({
                    "error": format!("HTTP {status}"),
                    "url": current_url.to_string(),
                }));
            }

            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = resp
                .text()
                .await
                .map_err(|err| ToolError::ExecutionFailed(format!("read body failed: {err}")))?;

            let (content, detected_mode) =
                extract_content(&body, &content_type, extract_mode, self.readability);

            let truncated = content.len() > max_chars;
            let content = if truncated {
                truncate_at_char_boundary(&content, max_chars)
            } else {
                content
            };

            return Ok(json!({
                "url": current_url.to_string(),
                "content_type": content_type,
                "extract_mode": detected_mode,
                "content": content,
                "truncated": truncated,
                "original_length": body.len(),
            }));
        }
    }
}

fn is_ssrf_allowed(ip: &IpAddr, allowlist: &[IpNet]) -> bool {
    allowlist.iter().any(|net| net.contains(ip))
}

async fn ssrf_check(url: &Url, allowlist: &[IpNet]) -> Result<(), ToolError> {
    let host = url
        .host_str()
        .ok_or_else(|| ToolError::InvalidArgs("URL has no host".to_string()))?;

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) && !is_ssrf_allowed(&ip, allowlist) {
            return Err(ToolError::ExecutionFailed(format!(
                "SSRF blocked: {host} resolves to private IP {ip}"
            )));
        }
        return Ok(());
    }

    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<_> = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|err| ToolError::ExecutionFailed(format!("DNS lookup failed for {host}: {err}")))?
        .collect();

    if addrs.is_empty() {
        return Err(ToolError::ExecutionFailed(format!(
            "DNS resolution failed for {host}"
        )));
    }

    for addr in addrs {
        let ip = addr.ip();
        if is_private_ip(&ip) && !is_ssrf_allowed(&ip, allowlist) {
            return Err(ToolError::ExecutionFailed(format!(
                "SSRF blocked: {host} resolves to private IP {ip}"
            )));
        }
    }

    Ok(())
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64)
                || (v4.octets()[0] == 192 && v4.octets()[1] == 0 && v4.octets()[2] == 0)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xFE00) == 0xFC00
                || (v6.segments()[0] & 0xFFC0) == 0xFE80
        }
    }
}

fn extract_content(
    body: &str,
    content_type: &str,
    requested_mode: &str,
    use_readability: bool,
) -> (String, String) {
    let ct = content_type.to_ascii_lowercase();

    if ct.contains("json") {
        if let Ok(parsed) = serde_json::from_str::<Value>(body) {
            let pretty = serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| body.to_string());
            return (pretty, "json".to_string());
        }
        return (body.to_string(), "text".to_string());
    }

    if ct.contains("text/plain") || !ct.contains("html") {
        return (body.to_string(), "text".to_string());
    }

    if use_readability && (requested_mode == "markdown" || requested_mode.is_empty()) {
        let cleaned = html_to_text(body);
        return (cleaned, "markdown".to_string());
    }

    let cleaned = html_to_text(body);
    (cleaned, "text".to_string())
}

fn html_to_text(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut last_was_space = false;

    let html_lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let lower_bytes = html_lower.as_bytes();

    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if i + 7 < lower_bytes.len() && &lower_bytes[i..i + 7] == b"<script" {
                in_script = true;
            }
            if i + 9 < lower_bytes.len() && &lower_bytes[i..i + 9] == b"</script>" {
                in_script = false;
            }
            if i + 6 < lower_bytes.len() && &lower_bytes[i..i + 6] == b"<style" {
                in_style = true;
            }
            if i + 8 < lower_bytes.len() && &lower_bytes[i..i + 8] == b"</style>" {
                in_style = false;
            }

            if !in_script && !in_style {
                let rest = &html_lower[i..];
                if rest.starts_with("<br")
                    || rest.starts_with("<p")
                    || rest.starts_with("</p")
                    || rest.starts_with("<div")
                    || rest.starts_with("</div")
                    || rest.starts_with("<h")
                    || rest.starts_with("</h")
                    || rest.starts_with("<li")
                {
                    if !result.ends_with('\n') {
                        result.push('\n');
                    }
                    last_was_space = true;
                }
            }

            in_tag = true;
            i += 1;
            continue;
        }

        if bytes[i] == b'>' {
            in_tag = false;
            i += 1;
            continue;
        }

        if in_tag || in_script || in_style {
            i += 1;
            continue;
        }

        if bytes[i] == b'&' {
            let rest = &html[i..];
            if let Some(semi) = rest.find(';') {
                let entity = &rest[..semi + 1];
                let decoded = match entity {
                    "&amp;" => "&",
                    "&lt;" => "<",
                    "&gt;" => ">",
                    "&quot;" => "\"",
                    "&apos;" | "&#39;" => "'",
                    "&nbsp;" | "&#160;" => " ",
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                result.push_str(decoded);
                last_was_space = decoded == " ";
                i += entity.len();
                continue;
            }
        }

        let ch = bytes[i] as char;
        if ch.is_ascii_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }

        i += 1;
    }

    result.trim().to_string()
}

fn truncate_at_char_boundary(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a specific URL and extract readable content (markdown/text). Use this when you already have a target page and need its body content, not a list of search results."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Fetch and extract a single web page.",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "HTTP/HTTPS URL to fetch."
                },
                "extract_mode": {
                    "type": "string",
                    "enum": ["markdown", "text"],
                    "description": "Extraction mode. Defaults to markdown."
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to return. Defaults to configured max_chars.",
                    "minimum": 1
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::NetworkRead
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let url = args
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `url`".to_string()))?;

        let extract_mode = args
            .get("extract_mode")
            .and_then(Value::as_str)
            .unwrap_or("markdown");

        if extract_mode != "markdown" && extract_mode != "text" {
            return Err(ToolError::InvalidArgs(
                "`extract_mode` must be one of: markdown, text".to_string(),
            ));
        }

        let max_chars = match args.get("max_chars") {
            Some(v) => {
                let value = v.as_u64().ok_or_else(|| {
                    ToolError::InvalidArgs("`max_chars` must be an integer".to_string())
                })? as usize;
                if value == 0 {
                    return Err(ToolError::InvalidArgs(
                        "`max_chars` must be greater than 0".to_string(),
                    ));
                }
                value
            }
            None => self.max_chars,
        };

        let cache_key = format!("{url}:{extract_mode}:{max_chars}");
        if let Some(cached) = self.cache_get(&cache_key) {
            let rendered = serde_json::to_string_pretty(&cached).map_err(|err| {
                ToolError::ExecutionFailed(format!("serialize output failed: {err}"))
            })?;
            return Ok(ToolOutput {
                content_for_model: rendered.clone(),
                content_for_user: Some(rendered),
            });
        }

        let accept_language = ctx
            .metadata
            .get("accept_language")
            .and_then(Value::as_str)
            .or_else(|| ctx.metadata.get("_accept_language").and_then(Value::as_str));

        let fetched = self
            .fetch_url(url, extract_mode, max_chars, accept_language)
            .await?;

        self.cache_set(cache_key, fetched.clone());

        let rendered = serde_json::to_string_pretty(&fetched)
            .map_err(|err| ToolError::ExecutionFailed(format!("serialize output failed: {err}")))?;

        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{AppConfig, ModelProviderConfig, ToolsConfig, WebFetchConfig};
    use std::collections::BTreeMap;

    fn default_tool() -> WebFetchTool {
        let mut providers = BTreeMap::new();
        providers.insert(
            "openai".to_string(),
            ModelProviderConfig {
                name: Some("OpenAI".to_string()),
                base_url: "https://api.openai.com/v1".to_string(),
                wire_api: "chat_completions".to_string(),
                default_model: "gpt-4o-mini".to_string(),
                tokenizer_path: None,
                proxy: false,
                api_key: Some("mock".to_string()),
                env_key: None,
            },
        );

        let app = AppConfig {
            model_provider: "openai".to_string(),
            model_providers: providers,
            tools: ToolsConfig {
                web_fetch: WebFetchConfig {
                    enabled: true,
                    max_chars: 50_000,
                    timeout_seconds: 5,
                    cache_ttl_minutes: 1,
                    max_redirects: 3,
                    readability: true,
                    ssrf_allowlist: vec![],
                },
                ..Default::default()
            },
            ..Default::default()
        };

        WebFetchTool::new(&app)
    }

    #[test]
    fn tool_name_and_schema() {
        let tool = default_tool();
        assert_eq!(tool.name(), "web_fetch");
        let schema = tool.parameters();
        assert_eq!(schema["required"][0], "url");
    }

    #[test]
    fn html_to_text_basic() {
        let html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn html_to_text_strips_scripts_and_styles() {
        let html =
            "<p>Before</p><script>alert('x')</script><style>.a{color:red}</style><p>After</p>";
        let text = html_to_text(html);
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color"));
    }

    #[test]
    fn extract_json_content() {
        let (content, mode) = extract_content(r#"{"a":1}"#, "application/json", "text", true);
        assert_eq!(mode, "json");
        assert!(content.contains("\"a\""));
    }

    #[test]
    fn truncate_preserves_utf8_boundary() {
        let value = "héllo";
        let t = truncate_at_char_boundary(value, 2);
        assert!(std::str::from_utf8(t.as_bytes()).is_ok());
    }

    #[test]
    fn private_ip_detection() {
        let private: IpAddr = "127.0.0.1".parse().unwrap();
        let public: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(is_private_ip(&private));
        assert!(!is_private_ip(&public));
    }

    #[test]
    fn ssrf_allowlist_match() {
        let allowlist: Vec<IpNet> = vec!["172.22.0.0/16".parse().unwrap()];
        let ip: IpAddr = "172.22.1.5".parse().unwrap();
        assert!(is_ssrf_allowed(&ip, &allowlist));
    }

    #[tokio::test]
    async fn execute_requires_url() {
        let tool = default_tool();
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };
        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("should fail");
        assert!(format!("{err}").contains("missing `url`"));
    }

    #[tokio::test]
    async fn ssrf_blocks_private_ip_url() {
        let tool = default_tool();
        let err = tool
            .fetch_url("http://127.0.0.1/test", "text", 1000, None)
            .await
            .expect_err("should block");
        assert!(format!("{err}").contains("SSRF blocked"));
    }

    #[tokio::test]
    async fn unsupported_scheme_fails() {
        let tool = default_tool();
        let err = tool
            .fetch_url("ftp://example.com", "text", 1000, None)
            .await
            .expect_err("should fail");
        assert!(format!("{err}").contains("unsupported URL scheme"));
    }
}
