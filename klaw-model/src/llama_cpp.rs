use std::{
    collections::HashMap,
    num::NonZeroU32,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
};

use async_trait::async_trait;
use encoding_rs::UTF_8;
use llama_cpp_2::{
    LogOptions,
    context::params::{LlamaContextParams, LlamaPoolingType},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::params::LlamaModelParams,
    model::{AddBos, LlamaModel},
    sampling::LlamaSampler,
    send_logs_to_tracing,
};
use tokio::process::Command;
use tracing::{debug, error};

use crate::{ModelError, ModelStorage};

static LLAMA_BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEmbeddingRequest {
    pub model_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelEmbeddingResponse {
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRerankRequest {
    pub model_id: String,
    pub query: String,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRerankResponse {
    pub scores: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChatRequest {
    pub model_id: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChatResponse {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOrchestrateRequest {
    pub model_id: String,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOrchestrateResponse {
    pub intent: QueryIntent,
    pub expansions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryIntent {
    Exact,
    Conceptual,
    Relationship,
    Exploratory,
    Temporal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlamaBackendKind {
    RustBinding,
    Command,
}

pub const fn default_backend_kind() -> LlamaBackendKind {
    LlamaBackendKind::RustBinding
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptFormat {
    EmbeddingGemma,
    QwenEmbedding,
    Raw,
}

impl PromptFormat {
    pub fn detect(filename: &str) -> Self {
        let lower = filename.to_ascii_lowercase();
        if lower.contains("embeddinggemma") {
            Self::EmbeddingGemma
        } else if lower.contains("qwen") && lower.contains("embed") {
            Self::QwenEmbedding
        } else {
            Self::Raw
        }
    }

    pub fn format_query(&self, query: &str) -> String {
        match self {
            Self::EmbeddingGemma => format!("<bos>search_query: {query}"),
            Self::QwenEmbedding => {
                format!("Instruct: Retrieve relevant passages\nQuery: {query}<|endoftext|>")
            }
            Self::Raw => query.to_string(),
        }
    }

    pub fn format_document(&self, text: &str) -> String {
        match self {
            Self::EmbeddingGemma => format!("<bos>search_document: {text}"),
            Self::QwenEmbedding | Self::Raw => text.to_string(),
        }
    }
}

#[async_trait]
pub trait EmbeddingRuntime: Send + Sync {
    async fn embed(
        &self,
        request: ModelEmbeddingRequest,
    ) -> Result<ModelEmbeddingResponse, ModelError>;
}

#[async_trait]
pub trait RerankRuntime: Send + Sync {
    async fn rerank(&self, request: ModelRerankRequest) -> Result<ModelRerankResponse, ModelError>;
}

#[async_trait]
pub trait ChatRuntime: Send + Sync {
    async fn chat(&self, request: ModelChatRequest) -> Result<ModelChatResponse, ModelError>;
}

#[async_trait]
pub trait OrchestratorRuntime: Send + Sync {
    async fn orchestrate(
        &self,
        request: ModelOrchestrateRequest,
    ) -> Result<ModelOrchestrateResponse, ModelError>;
}

#[async_trait]
pub trait LlamaCppBackend: Send + Sync {
    async fn run_embedding(
        &self,
        model_path: PathBuf,
        request: ModelEmbeddingRequest,
    ) -> Result<ModelEmbeddingResponse, ModelError>;

    async fn run_rerank(
        &self,
        model_path: PathBuf,
        request: ModelRerankRequest,
    ) -> Result<ModelRerankResponse, ModelError>;

    async fn run_chat(
        &self,
        model_path: PathBuf,
        request: ModelChatRequest,
    ) -> Result<ModelChatResponse, ModelError>;
}

pub struct LlamaCppRsBackend {
    default_ctx_size: u32,
    model_cache: Arc<Mutex<HashMap<PathBuf, Arc<LlamaModel>>>>,
}

impl std::fmt::Debug for LlamaCppRsBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamaCppRsBackend")
            .field("default_ctx_size", &self.default_ctx_size)
            .finish()
    }
}

impl Clone for LlamaCppRsBackend {
    fn clone(&self) -> Self {
        Self {
            default_ctx_size: self.default_ctx_size,
            model_cache: Arc::clone(&self.model_cache),
        }
    }
}

impl LlamaCppRsBackend {
    pub fn new(default_ctx_size: u32) -> Self {
        Self {
            default_ctx_size: default_ctx_size.max(1),
            model_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn load_model(&self, model_path: &Path) -> Result<Arc<LlamaModel>, ModelError> {
        let cached = {
            let cache = self
                .model_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.get(model_path).cloned()
        };
        if let Some(model) = cached {
            return Ok(model);
        }

        let model_params = LlamaModelParams::default();
        debug!(
            path = %model_path.display(),
            n_gpu_layers = model_params.n_gpu_layers(),
            "loading GGUF model with llama.cpp"
        );
        let model = LlamaModel::load_from_file(shared_llama_backend()?, model_path, &model_params)
            .map_err(|err| {
                error!(path = %model_path.display(), error = %err, "llama.cpp failed to load GGUF model");
                ModelError::Runtime(format!(
                    "loading GGUF model {}: {err}",
                    model_path.display()
                ))
            })?;
        debug!(path = %model_path.display(), "loaded GGUF model with llama.cpp");
        let model = Arc::new(model);

        let mut cache = self
            .model_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache
            .entry(model_path.to_path_buf())
            .or_insert_with(|| Arc::clone(&model));
        Ok(Arc::clone(entry))
    }

    fn run_embedding_blocking(
        &self,
        model_path: PathBuf,
        request: ModelEmbeddingRequest,
    ) -> Result<ModelEmbeddingResponse, ModelError> {
        let model = self.load_model(&model_path)?;
        let file_name = model_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let prompt_format = PromptFormat::detect(file_name);
        let formatted = prompt_format.format_query(&request.text);
        debug!(
            model_id = %request.model_id,
            path = %model_path.display(),
            prompt_format = ?prompt_format,
            input_chars = request.text.chars().count(),
            formatted_chars = formatted.chars().count(),
            "tokenizing embedding input"
        );
        let tokens = model
            .str_to_token(&formatted, AddBos::Never)
            .map_err(|err| ModelError::Runtime(format!("tokenization failed: {err}")))?;
        if tokens.is_empty() {
            return Err(ModelError::Runtime(
                "tokenizer returned empty token sequence".to_string(),
            ));
        }

        let n_tokens = tokens.len() as u32;
        let ctx_params = embedding_context_params(n_tokens, &prompt_format);
        debug!(
            model_id = %request.model_id,
            token_count = n_tokens,
            default_ctx_size = self.default_ctx_size,
            n_ctx = ctx_params.n_ctx().map(NonZeroU32::get).unwrap_or_default(),
            pooling = ?ctx_params.pooling_type(),
            "creating embedding context"
        );
        let mut ctx = model
            .new_context(shared_llama_backend()?, ctx_params)
            .map_err(|err| ModelError::Runtime(format!("creating embedding context: {err}")))?;
        debug!(model_id = %request.model_id, "created embedding context");

        let mut batch = LlamaBatch::new(tokens.len() + 16, 1);
        debug!(
            model_id = %request.model_id,
            token_count = tokens.len(),
            "adding tokens to embedding batch"
        );
        batch.add_sequence(&tokens, 0, true).map_err(|err| {
            ModelError::Runtime(format!("adding sequence to embedding batch: {err}"))
        })?;

        debug!(model_id = %request.model_id, "encoding embedding batch");
        ctx.encode(&mut batch)
            .map_err(|err| ModelError::Runtime(format!("embedding encode failed: {err}")))?;
        debug!(model_id = %request.model_id, "encoded embedding batch");

        debug!(model_id = %request.model_id, "reading embedding vector");
        let embeddings = ctx
            .embeddings_seq_ith(0)
            .map_err(|err| ModelError::Runtime(format!("getting embeddings: {err}")))?;
        let vector = l2_normalize(embeddings.to_vec());
        debug!(
            model_id = %request.model_id,
            dimensions = vector.len(),
            "embedding vector ready"
        );
        Ok(ModelEmbeddingResponse { vector })
    }

    fn run_rerank_blocking(
        &self,
        model_path: PathBuf,
        request: ModelRerankRequest,
    ) -> Result<ModelRerankResponse, ModelError> {
        let model = self.load_model(&model_path)?;
        let yes_token_id = first_token_id(
            &model,
            "Yes",
            "model tokenizer returned no tokens for 'Yes'",
        )?;
        let no_token_id =
            first_token_id(&model, "No", "model tokenizer returned no tokens for 'No'")?;

        let mut scores = Vec::with_capacity(request.candidates.len());
        for candidate in &request.candidates {
            let input = format_reranker_input(&request.query, candidate);
            let tokens = model.str_to_token(&input, AddBos::Always).map_err(|err| {
                ModelError::Runtime(format!("reranker tokenization failed: {err}"))
            })?;
            if tokens.is_empty() {
                scores.push(0.0);
                continue;
            }

            let n_ctx = ctx_size(self.default_ctx_size, tokens.len() as u32 + 16);
            let ctx_params = LlamaContextParams::default().with_n_ctx(n_ctx);
            let mut ctx = model
                .new_context(shared_llama_backend()?, ctx_params)
                .map_err(|err| ModelError::Runtime(format!("creating reranker context: {err}")))?;

            let mut batch = LlamaBatch::new(tokens.len() + 16, 1);
            for (index, token) in tokens.iter().enumerate() {
                let is_last = index == tokens.len() - 1;
                batch
                    .add(*token, index as i32, &[0], is_last)
                    .map_err(|err| {
                        ModelError::Runtime(format!("adding token to reranker batch: {err}"))
                    })?;
            }

            ctx.decode(&mut batch)
                .map_err(|err| ModelError::Runtime(format!("reranker decode failed: {err}")))?;
            let logits = ctx.get_logits_ith(batch.n_tokens() - 1);
            let yes_logit = logits.get(yes_token_id as usize).copied().ok_or_else(|| {
                ModelError::Runtime("Yes token logit index out of bounds".to_string())
            })?;
            let no_logit = logits.get(no_token_id as usize).copied().ok_or_else(|| {
                ModelError::Runtime("No token logit index out of bounds".to_string())
            })?;
            scores.push(softmax_binary_probability(yes_logit, no_logit));
        }

        Ok(ModelRerankResponse { scores })
    }

    fn run_chat_blocking(
        &self,
        model_path: PathBuf,
        request: ModelChatRequest,
    ) -> Result<ModelChatResponse, ModelError> {
        const MAX_TOKENS: usize = 256;

        let model = self.load_model(&model_path)?;
        let tokens = model
            .str_to_token(&request.prompt, AddBos::Always)
            .map_err(|err| ModelError::Runtime(format!("chat tokenization failed: {err}")))?;
        if tokens.is_empty() {
            return Err(ModelError::Runtime(
                "tokenizer returned empty token sequence".to_string(),
            ));
        }

        let n_ctx = ctx_size(
            self.default_ctx_size,
            tokens.len() as u32 + MAX_TOKENS as u32 + 16,
        );
        let ctx_params = LlamaContextParams::default().with_n_ctx(n_ctx);
        let mut ctx = model
            .new_context(shared_llama_backend()?, ctx_params)
            .map_err(|err| ModelError::Runtime(format!("creating chat context: {err}")))?;

        let mut batch = LlamaBatch::new(tokens.len() + MAX_TOKENS + 16, 1);
        for (index, token) in tokens.iter().enumerate() {
            let is_last = index == tokens.len() - 1;
            batch
                .add(*token, index as i32, &[0], is_last)
                .map_err(|err| {
                    ModelError::Runtime(format!("adding prompt token to chat batch: {err}"))
                })?;
        }

        ctx.decode(&mut batch)
            .map_err(|err| ModelError::Runtime(format!("prompt decode failed: {err}")))?;

        let mut sampler = LlamaSampler::greedy();
        let mut output = String::new();
        let mut decoder = UTF_8.new_decoder();
        let prompt_len = tokens.len();

        for step in 0..MAX_TOKENS {
            let new_token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(new_token);
            if model.is_eog_token(new_token) {
                break;
            }

            let piece = model
                .token_to_piece(new_token, &mut decoder, false, None)
                .map_err(|err| ModelError::Runtime(format!("token_to_piece failed: {err}")))?;
            output.push_str(&piece);

            batch.clear();
            batch
                .add(new_token, (prompt_len + step) as i32, &[0], true)
                .map_err(|err| {
                    ModelError::Runtime(format!("adding generated token to batch: {err}"))
                })?;
            ctx.decode(&mut batch)
                .map_err(|err| ModelError::Runtime(format!("generation decode failed: {err}")))?;
        }

        Ok(ModelChatResponse {
            content: output.trim().to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct LlamaCppCommandBackend {
    command: String,
    default_ctx_size: u32,
}

impl LlamaCppCommandBackend {
    pub fn new(command: impl Into<String>, default_ctx_size: u32) -> Self {
        Self {
            command: command.into(),
            default_ctx_size,
        }
    }

    fn base_command(&self, model_path: PathBuf) -> Command {
        let mut command = Command::new(&self.command);
        command
            .arg("-m")
            .arg(model_path)
            .arg("-c")
            .arg(self.default_ctx_size.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

#[async_trait]
impl LlamaCppBackend for LlamaCppRsBackend {
    async fn run_embedding(
        &self,
        model_path: PathBuf,
        request: ModelEmbeddingRequest,
    ) -> Result<ModelEmbeddingResponse, ModelError> {
        let backend = self.clone();
        tokio::task::spawn_blocking(move || backend.run_embedding_blocking(model_path, request))
            .await
            .map_err(|err| ModelError::Runtime(format!("llama embedding task join error: {err}")))?
    }

    async fn run_rerank(
        &self,
        model_path: PathBuf,
        request: ModelRerankRequest,
    ) -> Result<ModelRerankResponse, ModelError> {
        let backend = self.clone();
        tokio::task::spawn_blocking(move || backend.run_rerank_blocking(model_path, request))
            .await
            .map_err(|err| ModelError::Runtime(format!("llama rerank task join error: {err}")))?
    }

    async fn run_chat(
        &self,
        model_path: PathBuf,
        request: ModelChatRequest,
    ) -> Result<ModelChatResponse, ModelError> {
        let backend = self.clone();
        tokio::task::spawn_blocking(move || backend.run_chat_blocking(model_path, request))
            .await
            .map_err(|err| ModelError::Runtime(format!("llama chat task join error: {err}")))?
    }
}

#[async_trait]
impl LlamaCppBackend for LlamaCppCommandBackend {
    async fn run_embedding(
        &self,
        _model_path: PathBuf,
        _request: ModelEmbeddingRequest,
    ) -> Result<ModelEmbeddingResponse, ModelError> {
        Err(ModelError::Unsupported(
            "llama.cpp embedding command integration is not configured".to_string(),
        ))
    }

    async fn run_rerank(
        &self,
        _model_path: PathBuf,
        request: ModelRerankRequest,
    ) -> Result<ModelRerankResponse, ModelError> {
        Ok(ModelRerankResponse {
            scores: request
                .candidates
                .iter()
                .map(|candidate| {
                    let candidate = candidate.to_ascii_lowercase();
                    let query = request.query.to_ascii_lowercase();
                    if candidate.contains(&query) { 1.0 } else { 0.0 }
                })
                .collect(),
        })
    }

    async fn run_chat(
        &self,
        model_path: PathBuf,
        request: ModelChatRequest,
    ) -> Result<ModelChatResponse, ModelError> {
        let output = self
            .base_command(model_path)
            .arg("-p")
            .arg(request.prompt)
            .output()
            .await?;
        if !output.status.success() {
            return Err(ModelError::Runtime(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(ModelChatResponse {
            content: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        })
    }
}

#[derive(Clone)]
pub struct ModelLlamaRuntime<B> {
    storage: ModelStorage,
    backend: B,
}

impl<B> ModelLlamaRuntime<B> {
    pub fn new(storage: ModelStorage, backend: B) -> Self {
        Self { storage, backend }
    }

    fn gguf_path_for_model(&self, model_id: &str) -> Result<PathBuf, ModelError> {
        let manifest = self.storage.load_manifest(model_id)?;
        if let Some(file) = manifest
            .default_gguf_model_file
            .as_ref()
            .and_then(|default_file| {
                manifest.files.iter().find(|file| {
                    file.relative_path == *default_file
                        && file.format == crate::ModelFileFormat::Gguf
                        && file.relative_path.ends_with(".gguf")
                })
            })
        {
            return Ok(self.storage.paths().root_dir.join(&file.relative_path));
        }
        if let Some(default_file) = manifest.default_gguf_model_file.as_ref() {
            return Err(ModelError::Manifest(format!(
                "default GGUF model file '{default_file}' is not a GGUF file in model '{model_id}'"
            )));
        }
        let file = manifest
            .files
            .into_iter()
            .find(|file| file.relative_path.ends_with(".gguf"))
            .ok_or_else(|| ModelError::NotFound(model_id.to_string()))?;
        Ok(self.storage.paths().root_dir.join(file.relative_path))
    }
}

fn shared_llama_backend() -> Result<&'static LlamaBackend, ModelError> {
    let backend = LLAMA_BACKEND.get_or_init(|| {
        send_logs_to_tracing(llama_log_options());
        let backend =
            LlamaBackend::init().map_err(|err| format!("initializing llama backend: {err}"))?;
        Ok(backend)
    });
    backend
        .as_ref()
        .map_err(|err| ModelError::Runtime(err.clone()))
}

fn llama_log_options() -> LogOptions {
    LogOptions::default().with_logs_enabled(false)
}

fn ctx_size(default_ctx_size: u32, minimum: u32) -> Option<NonZeroU32> {
    NonZeroU32::new(default_ctx_size.max(minimum).max(1))
}

fn embedding_context_params(n_tokens: u32, prompt_format: &PromptFormat) -> LlamaContextParams {
    let n_ctx = n_tokens.max(64).saturating_add(16);
    let params = LlamaContextParams::default()
        .with_embeddings(true)
        .with_n_ctx(NonZeroU32::new(n_ctx))
        .with_n_ubatch(n_tokens.max(512))
        .with_n_batch(n_tokens.max(512));
    match prompt_format {
        PromptFormat::QwenEmbedding => params
            .with_pooling_type(LlamaPoolingType::Last)
            .with_flash_attention_policy(llama_cpp_sys_2::LLAMA_FLASH_ATTN_TYPE_DISABLED),
        PromptFormat::EmbeddingGemma | PromptFormat::Raw => params,
    }
}

fn l2_normalize(values: Vec<f32>) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        values.into_iter().map(|value| value / norm).collect()
    } else {
        values
    }
}

fn first_token_id(model: &LlamaModel, text: &str, empty_message: &str) -> Result<i32, ModelError> {
    let tokens = model
        .str_to_token(text, AddBos::Never)
        .map_err(|err| ModelError::Runtime(format!("tokenizing '{text}' failed: {err}")))?;
    tokens
        .first()
        .copied()
        .map(|token| token.0)
        .ok_or_else(|| ModelError::Runtime(empty_message.to_string()))
}

fn softmax_binary_probability(yes_logit: f32, no_logit: f32) -> f32 {
    let max_logit = yes_logit.max(no_logit);
    let yes_exp = (yes_logit - max_logit).exp();
    let no_exp = (no_logit - max_logit).exp();
    yes_exp / (yes_exp + no_exp)
}

fn format_reranker_input(query: &str, document: &str) -> String {
    format!(
        "<|im_start|>system\nJudge whether the document is relevant to the search query. \
         Respond only with \"Yes\" or \"No\".<|im_end|>\n\
         <|im_start|>user\nSearch query: {query}\nDocument: {document}<|im_end|>\n\
         <|im_start|>assistant\n"
    )
}

const ORCHESTRATOR_SYSTEM_PROMPT: &str = r#"You are a search query analyzer. Given a user's search query, classify it and expand it.

Return JSON with:
- "intent": one of "exact", "conceptual", "relationship", "exploratory", "temporal"
- "expansions": 2-4 alternative phrasings (always include the original query first)

Be concise. Only return the JSON object."#;

fn format_orchestrator_prompt(query: &str) -> String {
    format!(
        "<|im_start|>system\n{ORCHESTRATOR_SYSTEM_PROMPT}<|im_end|>\n\
         <|im_start|>user\n{query}<|im_end|>\n\
         <|im_start|>assistant\n"
    )
}

fn parse_orchestrator_response(
    text: &str,
    original_query: &str,
) -> Result<ModelOrchestrateResponse, ModelError> {
    let json = extract_json_object(text).ok_or_else(|| {
        ModelError::Runtime("no JSON object found in orchestrator output".to_string())
    })?;
    let parsed: serde_json::Value = serde_json::from_str(json)
        .map_err(|err| ModelError::Runtime(format!("parsing orchestrator JSON failed: {err}")))?;
    let intent = parse_query_intent(parsed.get("intent").and_then(serde_json::Value::as_str));
    let mut expansions = parsed
        .get("expansions")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if expansions.is_empty() {
        return Err(ModelError::Runtime(
            "orchestrator response did not include expansions".to_string(),
        ));
    }
    if expansions
        .first()
        .is_none_or(|value| value != original_query)
    {
        expansions.insert(0, original_query.to_string());
    }
    expansions.dedup();
    Ok(ModelOrchestrateResponse { intent, expansions })
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0_i32;
    for (index, byte) in text[start..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + index + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

fn heuristic_orchestrator_expansions(query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let stopwords = [
        "how", "does", "the", "a", "an", "is", "are", "was", "to", "in", "on", "for", "with",
        "what", "when", "where",
    ];
    let mut expansions = vec![trimmed.to_string()];
    let words = trimmed.split_whitespace().collect::<Vec<_>>();
    if words.len() > 2 {
        expansions.extend(
            words
                .into_iter()
                .filter(|word| word.len() > 2)
                .filter(|word| !stopwords.contains(&word.to_ascii_lowercase().as_str()))
                .map(str::to_string),
        );
    }
    expansions.dedup();
    expansions
}

fn heuristic_orchestrator_response(query: &str) -> ModelOrchestrateResponse {
    ModelOrchestrateResponse {
        intent: heuristic_query_intent(query),
        expansions: heuristic_orchestrator_expansions(query),
    }
}

fn parse_query_intent(value: Option<&str>) -> QueryIntent {
    match value
        .unwrap_or("exploratory")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "exact" => QueryIntent::Exact,
        "conceptual" => QueryIntent::Conceptual,
        "relationship" => QueryIntent::Relationship,
        "temporal" => QueryIntent::Temporal,
        _ => QueryIntent::Exploratory,
    }
}

fn heuristic_query_intent(query: &str) -> QueryIntent {
    let normalized = query.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return QueryIntent::Exploratory;
    }
    if normalized.contains("today")
        || normalized.contains("recent")
        || normalized.contains("latest")
        || normalized.contains("yesterday")
        || normalized.contains("last week")
        || normalized.contains("this month")
        || normalized.contains("202")
    {
        return QueryIntent::Temporal;
    }
    if normalized.contains("[[")
        || normalized.contains("relationship")
        || normalized.contains("related")
        || normalized.contains("link")
        || normalized.contains("backlink")
        || normalized.contains("depends on")
    {
        return QueryIntent::Relationship;
    }
    if normalized.contains('"')
        || normalized.contains("exact")
        || normalized.contains("quote")
        || normalized.contains("id:")
    {
        return QueryIntent::Exact;
    }
    if normalized.contains("how")
        || normalized.contains("why")
        || normalized.contains("architecture")
        || normalized.contains("design")
        || normalized.contains("concept")
    {
        return QueryIntent::Conceptual;
    }
    QueryIntent::Exploratory
}

#[async_trait]
impl<B> EmbeddingRuntime for ModelLlamaRuntime<B>
where
    B: LlamaCppBackend,
{
    async fn embed(
        &self,
        request: ModelEmbeddingRequest,
    ) -> Result<ModelEmbeddingResponse, ModelError> {
        let model_path = self.gguf_path_for_model(&request.model_id)?;
        self.backend.run_embedding(model_path, request).await
    }
}

#[async_trait]
impl<B> RerankRuntime for ModelLlamaRuntime<B>
where
    B: LlamaCppBackend,
{
    async fn rerank(&self, request: ModelRerankRequest) -> Result<ModelRerankResponse, ModelError> {
        let model_path = self.gguf_path_for_model(&request.model_id)?;
        self.backend.run_rerank(model_path, request).await
    }
}

#[async_trait]
impl<B> ChatRuntime for ModelLlamaRuntime<B>
where
    B: LlamaCppBackend,
{
    async fn chat(&self, request: ModelChatRequest) -> Result<ModelChatResponse, ModelError> {
        let model_path = self.gguf_path_for_model(&request.model_id)?;
        self.backend.run_chat(model_path, request).await
    }
}

#[async_trait]
impl<B> OrchestratorRuntime for ModelLlamaRuntime<B>
where
    B: LlamaCppBackend,
{
    async fn orchestrate(
        &self,
        request: ModelOrchestrateRequest,
    ) -> Result<ModelOrchestrateResponse, ModelError> {
        let model_path = self.gguf_path_for_model(&request.model_id)?;
        let prompt = format_orchestrator_prompt(&request.query);
        match self
            .backend
            .run_chat(
                model_path,
                ModelChatRequest {
                    model_id: request.model_id,
                    prompt,
                },
            )
            .await
        {
            Ok(response) => Ok(
                parse_orchestrator_response(&response.content, &request.query)
                    .unwrap_or_else(|_| heuristic_orchestrator_response(&request.query)),
            ),
            Err(_) => Ok(heuristic_orchestrator_response(&request.query)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        InstalledModelFile, InstalledModelManifest, ModelCapability, ModelFileFormat,
        ModelStoragePaths,
    };

    #[test]
    fn resolves_first_gguf_path_from_manifest() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-runtime-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root.clone()));
        storage.paths().ensure_dirs().expect("dirs");
        storage
            .save_manifest(&InstalledModelManifest {
                model_id: "qwen-main".to_string(),
                source: "huggingface".to_string(),
                repo_id: "Qwen/Qwen".to_string(),
                revision: "main".to_string(),
                resolved_revision: Some("abc123".to_string()),
                default_gguf_model_file: None,
                files: vec![InstalledModelFile {
                    relative_path: "snapshots/qwen-main/model.gguf".to_string(),
                    size_bytes: 10,
                    sha256: None,
                    format: ModelFileFormat::Gguf,
                }],
                capabilities: vec![ModelCapability::Chat],
                quantization: None,
                size_bytes: 10,
                installed_at: "2026-04-25T00:00:00Z".to_string(),
                last_used_at: None,
            })
            .expect("manifest");
        let runtime =
            ModelLlamaRuntime::new(storage, LlamaCppCommandBackend::new("llama-cli", 4096));
        let path = runtime
            .gguf_path_for_model("qwen-main")
            .expect("gguf path should resolve");
        assert!(path.ends_with("snapshots/qwen-main/model.gguf"));
    }

    #[test]
    fn resolves_manifest_default_gguf_path_before_file_order() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-runtime-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root.clone()));
        storage.paths().ensure_dirs().expect("dirs");
        storage
            .save_manifest(&InstalledModelManifest {
                model_id: "qwen-main".to_string(),
                source: "huggingface".to_string(),
                repo_id: "Qwen/Qwen".to_string(),
                revision: "main".to_string(),
                resolved_revision: Some("abc123".to_string()),
                default_gguf_model_file: Some("snapshots/qwen-main/preferred.gguf".to_string()),
                files: vec![
                    InstalledModelFile {
                        relative_path: "snapshots/qwen-main/first.gguf".to_string(),
                        size_bytes: 10,
                        sha256: None,
                        format: ModelFileFormat::Gguf,
                    },
                    InstalledModelFile {
                        relative_path: "snapshots/qwen-main/preferred.gguf".to_string(),
                        size_bytes: 10,
                        sha256: None,
                        format: ModelFileFormat::Gguf,
                    },
                ],
                capabilities: vec![ModelCapability::Chat],
                quantization: None,
                size_bytes: 20,
                installed_at: "2026-04-25T00:00:00Z".to_string(),
                last_used_at: None,
            })
            .expect("manifest");

        let runtime =
            ModelLlamaRuntime::new(storage, LlamaCppCommandBackend::new("llama-cli", 4096));
        let path = runtime
            .gguf_path_for_model("qwen-main")
            .expect("gguf path should resolve");
        assert!(path.ends_with("snapshots/qwen-main/preferred.gguf"));
    }

    #[test]
    fn rejects_manifest_default_gguf_path_outside_file_list() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-runtime-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root.clone()));
        storage.paths().ensure_dirs().expect("dirs");
        storage
            .save_manifest(&InstalledModelManifest {
                model_id: "qwen-main".to_string(),
                source: "huggingface".to_string(),
                repo_id: "Qwen/Qwen".to_string(),
                revision: "main".to_string(),
                resolved_revision: Some("abc123".to_string()),
                default_gguf_model_file: Some("external/preferred.gguf".to_string()),
                files: vec![InstalledModelFile {
                    relative_path: "snapshots/qwen-main/model.gguf".to_string(),
                    size_bytes: 10,
                    sha256: None,
                    format: ModelFileFormat::Gguf,
                }],
                capabilities: vec![ModelCapability::Chat],
                quantization: None,
                size_bytes: 10,
                installed_at: "2026-04-25T00:00:00Z".to_string(),
                last_used_at: None,
            })
            .expect("manifest");

        let runtime =
            ModelLlamaRuntime::new(storage, LlamaCppCommandBackend::new("llama-cli", 4096));
        let err = runtime
            .gguf_path_for_model("qwen-main")
            .expect_err("invalid default gguf should fail");

        assert!(matches!(err, ModelError::Manifest(_)));
    }

    #[test]
    fn default_backend_kind_prefers_rust_binding() {
        assert_eq!(default_backend_kind(), LlamaBackendKind::RustBinding);
    }

    #[test]
    fn llama_log_options_disable_native_logs() {
        let rendered = format!("{:?}", llama_log_options());

        assert!(rendered.contains("disabled: true"));
    }

    #[test]
    fn prompt_format_detects_qwen_embedding_models() {
        let format = PromptFormat::detect("qwen3-embedding-0.6b-q8_0.gguf");
        assert_eq!(format, PromptFormat::QwenEmbedding);
        assert_eq!(
            format.format_query("how does auth work"),
            "Instruct: Retrieve relevant passages\nQuery: how does auth work<|endoftext|>"
        );
    }

    #[test]
    fn qwen_embedding_context_uses_last_pooling() {
        let params = embedding_context_params(16, &PromptFormat::QwenEmbedding);
        assert_eq!(params.pooling_type(), LlamaPoolingType::Last);
    }

    #[test]
    fn qwen_embedding_context_disables_flash_attention() {
        let params = embedding_context_params(16, &PromptFormat::QwenEmbedding);
        assert_eq!(
            params.flash_attention_policy(),
            llama_cpp_sys_2::LLAMA_FLASH_ATTN_TYPE_DISABLED
        );
    }

    #[test]
    fn embedding_context_sizes_to_current_batch() {
        let params = embedding_context_params(589, &PromptFormat::EmbeddingGemma);
        assert_eq!(params.n_ctx(), NonZeroU32::new(605));

        let short = embedding_context_params(8, &PromptFormat::EmbeddingGemma);
        assert_eq!(short.n_ctx(), NonZeroU32::new(80));
    }

    #[test]
    fn prompt_format_detects_embeddinggemma_models() {
        let format = PromptFormat::detect("embeddinggemma-300m-q8_0.gguf");
        assert_eq!(format, PromptFormat::EmbeddingGemma);
        assert_eq!(
            format.format_document("vault note"),
            "<bos>search_document: vault note"
        );
    }

    #[test]
    fn softmax_probability_prefers_higher_yes_logit() {
        let score = softmax_binary_probability(2.0, 1.0);
        assert!(score > 0.5);
    }

    #[test]
    fn parses_orchestrator_json_expansions_from_embedded_text() {
        let response = "thinking...\n{\"intent\":\"conceptual\",\"expansions\":[\"auth design\",\"authentication architecture\"]}\n";
        let parsed = parse_orchestrator_response(response, "auth design")
            .expect("orchestrator response should parse");
        assert_eq!(parsed.intent, QueryIntent::Conceptual);
        assert_eq!(
            parsed.expansions,
            vec!["auth design", "authentication architecture"]
        );
    }

    #[test]
    fn heuristic_orchestrator_fallback_keeps_original_query_first() {
        let expansions = heuristic_orchestrator_expansions("how does auth token rotation work");
        assert_eq!(
            expansions.first().map(String::as_str),
            Some("how does auth token rotation work")
        );
        assert!(expansions.len() > 1);
    }

    #[test]
    fn parser_inserts_original_query_when_missing() {
        let response = "{\"intent\":\"exact\",\"expansions\":[\"authentication architecture\"]}";
        let parsed = parse_orchestrator_response(response, "auth design")
            .expect("parser should add original query");
        assert_eq!(parsed.intent, QueryIntent::Exact);
        assert_eq!(
            parsed.expansions.first().map(String::as_str),
            Some("auth design")
        );
    }

    #[test]
    fn heuristic_intent_detects_temporal_queries() {
        assert_eq!(
            heuristic_query_intent("what changed last week"),
            QueryIntent::Temporal
        );
    }
}
