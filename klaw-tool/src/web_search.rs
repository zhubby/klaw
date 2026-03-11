use async_trait::async_trait;
use klaw_config::{AppConfig, BraveWebSearchConfig, TavilyWebSearchConfig};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const DEFAULT_MAX_RESULTS: usize = 5;
const MAX_RESULTS_LIMIT: usize = 20;

pub struct WebSearchTool {
    provider: Box<dyn WebSearchProvider>,
}

#[derive(Debug, Clone)]
struct SearchRequest {
    query: String,
    max_results: usize,
}

#[derive(Debug, Clone)]
struct SearchResultItem {
    title: String,
    url: String,
    snippet: String,
}

#[async_trait]
trait WebSearchProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn search(&self, request: &SearchRequest) -> Result<Vec<SearchResultItem>, ToolError>;
}

#[derive(Clone)]
struct TavilyProvider {
    client: reqwest::Client,
    config: TavilyWebSearchConfig,
    api_key: Option<String>,
}

#[derive(Clone)]
struct BraveProvider {
    client: reqwest::Client,
    config: BraveWebSearchConfig,
    api_key: Option<String>,
}

impl WebSearchTool {
    pub fn new(config: &AppConfig) -> Result<Self, ToolError> {
        let provider_key = config.tools.web_search.provider.trim().to_ascii_lowercase();
        let provider: Box<dyn WebSearchProvider> = match provider_key.as_str() {
            "tavily" => Box::new(TavilyProvider::new(config.tools.web_search.tavily.clone())?),
            "brave" => Box::new(BraveProvider::new(config.tools.web_search.brave.clone())?),
            other => {
                return Err(ToolError::InvalidArgs(format!(
                    "unsupported web_search provider '{other}'"
                )))
            }
        };
        Ok(Self { provider })
    }
}

fn build_http_client() -> Result<reqwest::Client, ToolError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|err| ToolError::ExecutionFailed(format!("build web_search client failed: {err}")))
}

fn resolve_api_key(api_key: &Option<String>, env_key: &Option<String>) -> Option<String> {
    api_key.clone().or_else(|| {
        env_key
            .as_ref()
            .and_then(|env_name| std::env::var(env_name).ok())
    })
}

fn require_api_key(
    provider_name: &str,
    api_key: &Option<String>,
    env_key: &Option<String>,
) -> Result<String, ToolError> {
    resolve_api_key(api_key, env_key).ok_or_else(|| {
        ToolError::ExecutionFailed(format!(
            "{provider_name} requires api_key or env_key (or the env var must be set)"
        ))
    })
}

fn parse_request(args: Value) -> Result<SearchRequest, ToolError> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or_else(|| ToolError::InvalidArgs("missing `query`".to_string()))?
        .to_string();

    let max_results = match args.get("max_results") {
        Some(v) => v
            .as_u64()
            .ok_or_else(|| ToolError::InvalidArgs("`max_results` must be an integer".to_string()))?
            as usize,
        None => DEFAULT_MAX_RESULTS,
    };

    if max_results == 0 {
        return Err(ToolError::InvalidArgs(
            "`max_results` must be greater than 0".to_string(),
        ));
    }

    Ok(SearchRequest {
        query,
        max_results: max_results.min(MAX_RESULTS_LIMIT),
    })
}

fn format_results(provider_name: &str, query: &str, items: &[SearchResultItem]) -> String {
    if items.is_empty() {
        return format!("provider: {provider_name}\nquery: {query}\nno results");
    }

    let mut out = format!("provider: {provider_name}\nquery: {query}\nresults:\n");
    for (idx, item) in items.iter().enumerate() {
        out.push_str(&format!(
            "{}. {}\n   url: {}\n   snippet: {}\n",
            idx + 1,
            item.title,
            item.url,
            item.snippet
        ));
    }
    out.trim_end().to_string()
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the public web for up-to-date facts, documentation, news, and references. Use this when the answer depends on external or recent information not guaranteed to exist in local code/context."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Arguments for a web search request.",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural-language search query. Be specific and include key entities, timeframe, version numbers, or site names when relevant.",
                    "minLength": 1,
                    "examples": [
                        "Rust reqwest timeout per request",
                        "Brave Search API rate limits 2026",
                        "Tavily search_depth parameter docs"
                    ]
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return. Defaults to 5 and is clamped to [1, 20]. Use smaller values for focused lookups.",
                    "minimum": 1,
                    "maximum": 20,
                    "default": 5
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::NetworkRead
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = parse_request(args)?;
        let items = self.provider.search(&request).await?;
        let content = format_results(self.provider.name(), &request.query, &items);
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }
}

impl TavilyProvider {
    fn new(config: TavilyWebSearchConfig) -> Result<Self, ToolError> {
        let api_key = config.api_key.clone();
        Ok(Self {
            client: build_http_client()?,
            config,
            api_key,
        })
    }
}

#[async_trait]
impl WebSearchProvider for TavilyProvider {
    fn name(&self) -> &str {
        "tavily"
    }

    async fn search(&self, request: &SearchRequest) -> Result<Vec<SearchResultItem>, ToolError> {
        let api_key = require_api_key("tavily", &self.api_key, &self.config.env_key)?;
        let url = format!(
            "{}/search",
            self.config
                .base_url
                .as_deref()
                .unwrap_or("https://api.tavily.com")
                .trim_end_matches('/')
        );

        let mut body = json!({
            "query": request.query,
            "max_results": request.max_results,
            "search_depth": self.config.search_depth,
        });
        if let Some(topic) = &self.config.topic {
            body["topic"] = json!(topic);
        }
        if let Some(include_answer) = self.config.include_answer {
            body["include_answer"] = json!(include_answer);
        }
        if let Some(include_raw_content) = self.config.include_raw_content {
            body["include_raw_content"] = json!(include_raw_content);
        }
        if let Some(include_images) = self.config.include_images {
            body["include_images"] = json!(include_images);
        }

        let mut req = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body);
        if let Some(project_id) = &self.config.project_id {
            req = req.header("X-Project-ID", project_id);
        }

        let resp = req
            .send()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("tavily request failed: {err}")))?;
        if !resp.status().is_success() {
            return Err(ToolError::ExecutionFailed(format!(
                "tavily request failed with status {}",
                resp.status()
            )));
        }

        let payload: TavilySearchResponse = resp
            .json()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("invalid tavily response: {err}")))?;
        Ok(payload
            .results
            .into_iter()
            .map(|item| SearchResultItem {
                title: item.title,
                url: item.url,
                snippet: item.content.unwrap_or_default(),
            })
            .collect())
    }
}

impl BraveProvider {
    fn new(config: BraveWebSearchConfig) -> Result<Self, ToolError> {
        let api_key = config.api_key.clone();
        Ok(Self {
            client: build_http_client()?,
            config,
            api_key,
        })
    }
}

#[async_trait]
impl WebSearchProvider for BraveProvider {
    fn name(&self) -> &str {
        "brave"
    }

    async fn search(&self, request: &SearchRequest) -> Result<Vec<SearchResultItem>, ToolError> {
        let api_key = require_api_key("brave", &self.api_key, &self.config.env_key)?;
        let url = format!(
            "{}/res/v1/web/search",
            self.config
                .base_url
                .as_deref()
                .unwrap_or("https://api.search.brave.com")
                .trim_end_matches('/')
        );

        let mut query_params: Vec<(String, String)> = vec![
            ("q".to_string(), request.query.clone()),
            ("count".to_string(), request.max_results.to_string()),
        ];
        if let Some(country) = &self.config.country {
            query_params.push(("country".to_string(), country.clone()));
        }
        if let Some(search_lang) = &self.config.search_lang {
            query_params.push(("search_lang".to_string(), search_lang.clone()));
        }
        if let Some(ui_lang) = &self.config.ui_lang {
            query_params.push(("ui_lang".to_string(), ui_lang.clone()));
        }
        if let Some(safesearch) = &self.config.safesearch {
            query_params.push(("safesearch".to_string(), safesearch.clone()));
        }
        if let Some(freshness) = &self.config.freshness {
            query_params.push(("freshness".to_string(), freshness.clone()));
        }

        let resp = self
            .client
            .get(url)
            .query(&query_params)
            .header("Accept", "application/json")
            .header("Accept-Encoding", "gzip")
            .header("X-Subscription-Token", api_key)
            .send()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("brave request failed: {err}")))?;
        if !resp.status().is_success() {
            return Err(ToolError::ExecutionFailed(format!(
                "brave request failed with status {}",
                resp.status()
            )));
        }

        let payload: BraveSearchResponse = resp
            .json()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("invalid brave response: {err}")))?;
        Ok(payload
            .web
            .map(|web| {
                web.results
                    .into_iter()
                    .map(|item| SearchResultItem {
                        title: item.title.unwrap_or_else(|| "(untitled)".to_string()),
                        url: item.url,
                        snippet: item.description.unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[derive(Debug, Deserialize)]
struct TavilySearchResponse {
    #[serde(default)]
    results: Vec<TavilySearchItem>,
}

#[derive(Debug, Deserialize)]
struct TavilySearchItem {
    title: String,
    url: String,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BraveSearchResponse {
    #[serde(default)]
    web: Option<BraveWebResults>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    #[serde(default)]
    results: Vec<BraveSearchItem>,
}

#[derive(Debug, Deserialize)]
struct BraveSearchItem {
    #[serde(default)]
    title: Option<String>,
    url: String,
    #[serde(default)]
    description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{
        MemoryConfig, ModelProviderConfig, ShellConfig, ToolsConfig, WebSearchConfig,
    };
    use std::collections::BTreeMap;

    fn base_app_config() -> AppConfig {
        let mut model_providers = BTreeMap::new();
        model_providers.insert(
            "openai".to_string(),
            ModelProviderConfig {
                name: Some("OpenAI".to_string()),
                base_url: "https://api.openai.com/v1".to_string(),
                wire_api: "chat_completions".to_string(),
                default_model: "gpt-4o-mini".to_string(),
                api_key: Some("mock".to_string()),
                env_key: None,
            },
        );
        AppConfig {
            model_provider: "openai".to_string(),
            model_providers,
            memory: MemoryConfig::default(),
            tools: ToolsConfig {
                shell: ShellConfig::default(),
                web_search: WebSearchConfig::default(),
                ..ToolsConfig::default()
            },
        }
    }

    #[test]
    fn parse_request_requires_query() {
        let err = parse_request(json!({})).expect_err("should fail");
        assert!(format!("{err}").contains("missing `query`"));
    }

    #[test]
    fn parse_request_clamps_limit() {
        let req = parse_request(json!({"query": "rust", "max_results": 200})).unwrap();
        assert_eq!(req.max_results, MAX_RESULTS_LIMIT);
    }

    #[test]
    fn build_tool_with_tavily_provider() {
        let mut cfg = base_app_config();
        cfg.tools.web_search.enabled = true;
        cfg.tools.web_search.provider = "tavily".to_string();
        cfg.tools.web_search.tavily.api_key = Some("test-key".to_string());
        let tool = WebSearchTool::new(&cfg).expect("tool should build");
        assert_eq!(tool.provider.name(), "tavily");
    }

    #[test]
    fn build_tool_with_brave_provider() {
        let mut cfg = base_app_config();
        cfg.tools.web_search.enabled = true;
        cfg.tools.web_search.provider = "brave".to_string();
        cfg.tools.web_search.brave.api_key = Some("test-key".to_string());
        let tool = WebSearchTool::new(&cfg).expect("tool should build");
        assert_eq!(tool.provider.name(), "brave");
    }

    #[test]
    fn format_results_contains_entries() {
        let items = vec![SearchResultItem {
            title: "Rust".to_string(),
            url: "https://www.rust-lang.org".to_string(),
            snippet: "Rust language".to_string(),
        }];
        let text = format_results("tavily", "rust", &items);
        assert!(text.contains("provider: tavily"));
        assert!(text.contains("Rust"));
        assert!(text.contains("https://www.rust-lang.org"));
    }
}
