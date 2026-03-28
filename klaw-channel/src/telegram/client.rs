use super::render::TelegramParseMode;
use super::types::{TelegramFile, TelegramInlineKeyboardMarkup, TelegramMessage, TelegramUpdate};
use crate::ChannelResult;
use klaw_config::TelegramProxyConfig;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt;
use tokio::time::Duration;

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";
const TELEGRAM_LONG_POLL_TIMEOUT_SECS: u64 = 20;
const TELEGRAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct TelegramApiClient {
    http: reqwest::Client,
    api_base: String,
    file_base: String,
}

impl TelegramApiClient {
    pub fn new(bot_token: &str, proxy: &TelegramProxyConfig) -> ChannelResult<Self> {
        let mut builder = reqwest::Client::builder().timeout(TELEGRAM_REQUEST_TIMEOUT);
        if proxy.enabled {
            let proxy_url = proxy.url.trim();
            if proxy_url.is_empty() {
                return Err("telegram proxy.url is required when proxy.enabled=true".into());
            }
            builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
        }
        let http = builder.build()?;
        let api_base = format!("{TELEGRAM_API_BASE}/bot{}", bot_token.trim());
        let file_base = format!("{TELEGRAM_API_BASE}/file/bot{}", bot_token.trim());
        Ok(Self {
            http,
            api_base,
            file_base,
        })
    }

    pub async fn get_updates(&self, offset: Option<i64>) -> ChannelResult<Vec<TelegramUpdate>> {
        let payload = GetUpdatesRequest {
            offset,
            timeout: TELEGRAM_LONG_POLL_TIMEOUT_SECS,
            allowed_updates: vec!["message".to_string(), "callback_query".to_string()],
        };
        self.post("getUpdates", &payload).await
    }

    pub async fn get_file(&self, file_id: &str) -> ChannelResult<TelegramFile> {
        self.post("getFile", &GetFileRequest { file_id }).await
    }

    pub async fn set_my_commands(&self, commands: Vec<BotCommand>) -> ChannelResult<()> {
        let _: TelegramBoolResponse = self
            .post("setMyCommands", &SetMyCommandsRequest { commands })
            .await?;
        Ok(())
    }

    pub async fn send_message(
        &self,
        request: SendMessageRequest,
    ) -> ChannelResult<TelegramMessage> {
        self.post("sendMessage", &request).await
    }

    pub async fn edit_message_text(
        &self,
        request: EditMessageTextRequest,
    ) -> ChannelResult<TelegramMessage> {
        self.post("editMessageText", &request).await
    }

    pub async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: &str,
    ) -> ChannelResult<()> {
        let _: TelegramBoolResponse = self
            .post(
                "answerCallbackQuery",
                &AnswerCallbackQueryRequest {
                    callback_query_id: callback_query_id.trim().to_string(),
                    text: text.trim().to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn send_photo_bytes(
        &self,
        chat_id: &str,
        filename: &str,
        bytes: &[u8],
        caption: Option<&str>,
    ) -> ChannelResult<TelegramMessage> {
        self.post_multipart("sendPhoto", "photo", chat_id, filename, bytes, caption)
            .await
    }

    pub async fn send_document_bytes(
        &self,
        chat_id: &str,
        filename: &str,
        bytes: &[u8],
        caption: Option<&str>,
    ) -> ChannelResult<TelegramMessage> {
        self.post_multipart(
            "sendDocument",
            "document",
            chat_id,
            filename,
            bytes,
            caption,
        )
        .await
    }

    pub async fn download_file(&self, file_path: &str) -> ChannelResult<Vec<u8>> {
        let url = format!("{}/{}", self.file_base, file_path.trim_start_matches('/'));
        let response = self.http.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "telegram download file failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        Ok(response.bytes().await?.to_vec())
    }

    async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        body: &B,
    ) -> ChannelResult<T> {
        let url = format!("{}/{}", self.api_base, method);
        let response = self.http.post(url).json(body).send().await?;
        let status = response.status();
        let envelope: TelegramApiEnvelope<T> = response.json().await?;
        if !status.is_success() || !envelope.ok {
            return Err(format!(
                "telegram {} failed: HTTP {} description={}",
                method,
                status,
                envelope
                    .description
                    .as_deref()
                    .unwrap_or("unknown telegram api error")
            )
            .into());
        }
        envelope
            .result
            .ok_or_else(|| format!("telegram {} missing result", method).into())
    }

    async fn post_multipart<T: DeserializeOwned>(
        &self,
        method: &str,
        field_name: &str,
        chat_id: &str,
        filename: &str,
        bytes: &[u8],
        caption: Option<&str>,
    ) -> ChannelResult<T> {
        let url = format!("{}/{}", self.api_base, method);
        let part =
            reqwest::multipart::Part::bytes(bytes.to_vec()).file_name(filename.trim().to_string());
        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.trim().to_string())
            .part(field_name.to_string(), part);
        if let Some(caption) = caption.map(str::trim).filter(|value| !value.is_empty()) {
            form = form
                .text("caption", caption.to_string())
                .text("parse_mode", TelegramParseMode::Html.as_str().to_string());
        }
        let response = self.http.post(url).multipart(form).send().await?;
        let status = response.status();
        let envelope: TelegramApiEnvelope<T> = response.json().await?;
        if !status.is_success() || !envelope.ok {
            return Err(format!(
                "telegram {} failed: HTTP {} description={}",
                method,
                status,
                envelope
                    .description
                    .as_deref()
                    .unwrap_or("unknown telegram api error")
            )
            .into());
        }
        envelope
            .result
            .ok_or_else(|| format!("telegram {} missing result", method).into())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct TelegramApiEnvelope<T> {
    pub ok: bool,
    #[serde(default)]
    pub result: Option<T>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GetUpdatesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<i64>,
    timeout: u64,
    allowed_updates: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GetFileRequest<'a> {
    file_id: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct SetMyCommandsRequest {
    commands: Vec<BotCommand>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BotCommand {
    pub command: String,
    pub description: String,
}

impl BotCommand {
    pub fn new(command: &str, description: &str) -> Self {
        Self {
            command: command.trim().to_string(),
            description: description.trim().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SendMessageRequest {
    pub chat_id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_markup: Option<TelegramInlineKeyboardMarkup>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditMessageTextRequest {
    pub chat_id: String,
    pub message_id: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_markup: Option<TelegramInlineKeyboardMarkup>,
}

impl SendMessageRequest {
    pub fn html(chat_id: &str, text: &str) -> Self {
        Self {
            chat_id: chat_id.trim().to_string(),
            text: text.trim().to_string(),
            parse_mode: Some(TelegramParseMode::Html.as_str().to_string()),
            reply_markup: None,
        }
    }

    pub fn with_reply_markup(mut self, reply_markup: TelegramInlineKeyboardMarkup) -> Self {
        self.reply_markup = Some(reply_markup);
        self
    }
}

impl EditMessageTextRequest {
    pub fn html(chat_id: &str, message_id: i64, text: &str) -> Self {
        Self {
            chat_id: chat_id.trim().to_string(),
            message_id,
            text: text.trim().to_string(),
            parse_mode: Some(TelegramParseMode::Html.as_str().to_string()),
            reply_markup: None,
        }
    }

    pub fn with_reply_markup(mut self, reply_markup: TelegramInlineKeyboardMarkup) -> Self {
        self.reply_markup = Some(reply_markup);
        self
    }
}

#[derive(Debug, Clone, Serialize)]
struct AnswerCallbackQueryRequest {
    callback_query_id: String,
    text: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TelegramBoolResponse(bool);

impl fmt::Display for TelegramBoolResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0 { "true" } else { "false" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_message_request_defaults_to_html() {
        let request = SendMessageRequest::html(" 42 ", " <b>x</b> ");
        assert_eq!(request.chat_id, "42");
        assert_eq!(request.text, "<b>x</b>");
        assert_eq!(request.parse_mode.as_deref(), Some("HTML"));
        assert!(request.reply_markup.is_none());
    }

    #[test]
    fn bot_command_new_trims_fields() {
        let command = BotCommand::new(" start ", " Show help ");
        assert_eq!(
            command,
            BotCommand {
                command: "start".to_string(),
                description: "Show help".to_string(),
            }
        );
    }
}
