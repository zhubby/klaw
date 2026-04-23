use super::{config::DingtalkProxyConfig, error::DingtalkApiError};
use crate::ChannelResult;
use serde_json::Value;
use std::sync::OnceLock;
use tokio::time::Duration;
use tracing::debug;
use uuid::Uuid;

const DINGTALK_OPEN_API_BASE: &str = "https://api.dingtalk.com";
const DINGTALK_OAPI_BASE: &str = "https://oapi.dingtalk.com";
const CONNECTION_OPEN_PATH: &str = "/v1.0/gateway/connections/open";
const ACCESS_TOKEN_PATH: &str = "/v1.0/oauth2/accessToken";
const MESSAGE_FILE_DOWNLOAD_PATH: &str = "/v1.0/robot/messageFiles/download";
const GROUP_MESSAGE_SEND_PATH: &str = "/v1.0/robot/groupMessages/send";
const OTO_MESSAGE_BATCH_SEND_PATH: &str = "/v1.0/robot/oToMessages/batchSend";
const CARD_INSTANCE_CREATE_AND_DELIVER_PATH: &str = "/v1.0/card/instances/createAndDeliver";
const CARD_STREAMING_PATH: &str = "/v1.0/card/streaming";
const OAPI_MEDIA_UPLOAD_PATH: &str = "/media/upload";
const OAPI_ASR_TRANSLATE_PATH: &str = "/topapi/asr/voice/translate";
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

static RUSTLS_PROVIDER_INSTALLED: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone)]
pub(super) struct DingtalkApiClient {
    http: reqwest::Client,
    open_api_base: String,
    oapi_base: String,
}

#[derive(Debug, Clone)]
pub(super) struct StreamConnectionTicket {
    pub endpoint: String,
    pub ticket: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProactiveChatTargetKind {
    Group,
    User,
}

pub(super) fn proactive_chat_target_kind(chat_id: &str) -> ProactiveChatTargetKind {
    if chat_id.trim_start().starts_with("cid") {
        ProactiveChatTargetKind::Group
    } else {
        ProactiveChatTargetKind::User
    }
}

pub(super) fn build_proactive_markdown_payload(
    robot_code: &str,
    chat_id: &str,
    title: &str,
    text: &str,
) -> Value {
    let mut payload = serde_json::json!({
        "robotCode": robot_code.trim(),
        "msgKey": "sampleMarkdown",
        "msgParam": serde_json::to_string(&serde_json::json!({
            "title": title,
            "text": text,
        }))
        .expect("markdown payload should serialize"),
    });
    match proactive_chat_target_kind(chat_id) {
        ProactiveChatTargetKind::Group => {
            payload["openConversationId"] = Value::String(chat_id.trim().to_string());
        }
        ProactiveChatTargetKind::User => {
            payload["userIds"] = serde_json::json!([chat_id.trim()]);
        }
    }
    payload
}

pub(super) fn build_ai_card_card_data(content_key: &str, content: &str) -> Value {
    serde_json::json!({
        "cardParamMap": {
            content_key.trim(): content,
        }
    })
}

pub(super) fn build_create_and_deliver_ai_card_payload(
    template_id: &str,
    robot_code: &str,
    chat_id: &str,
    out_track_id: &str,
    card_data: Value,
) -> Value {
    let chat_id = chat_id.trim();
    let open_space_id = match proactive_chat_target_kind(chat_id) {
        ProactiveChatTargetKind::Group => format!("dtv1.card//IM_GROUP.{chat_id}"),
        ProactiveChatTargetKind::User => format!("dtv1.card//IM_ROBOT.{chat_id}"),
    };
    let mut payload = serde_json::json!({
        "cardTemplateId": template_id.trim(),
        "outTrackId": out_track_id.trim(),
        "cardData": card_data,
        "openSpaceId": open_space_id,
        "userIdType": 1,
    });
    match proactive_chat_target_kind(chat_id) {
        ProactiveChatTargetKind::Group => {
            payload["imGroupOpenDeliverModel"] = serde_json::json!({
                "robotCode": robot_code.trim(),
            });
            payload["imGroupOpenSpaceModel"] = serde_json::json!({
                "supportForward": false,
            });
        }
        ProactiveChatTargetKind::User => {
            payload["userId"] = Value::String(chat_id.to_string());
            payload["imRobotOpenDeliverModel"] = serde_json::json!({
                "robotCode": robot_code.trim(),
                "spaceType": "IM_ROBOT",
            });
            payload["imRobotOpenSpaceModel"] = serde_json::json!({
                "supportForward": false,
            });
        }
    }
    payload
}

pub(super) fn build_streaming_ai_card_payload(
    out_track_id: &str,
    guid: &str,
    key: &str,
    content: &str,
    is_finalize: bool,
    is_error: bool,
) -> Value {
    serde_json::json!({
        "outTrackId": out_track_id.trim(),
        "guid": guid.trim(),
        "key": key.trim(),
        "content": content,
        "isFull": true,
        "isFinalize": is_finalize,
        "isError": is_error,
    })
}

fn ai_card_delivery_result_summaries(body: &Value) -> Vec<String> {
    body.pointer("/result/deliverResults")
        .and_then(Value::as_array)
        .map(|results| {
            results
                .iter()
                .map(|result| {
                    let space_type = result
                        .get("spaceType")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let space_id = result
                        .get("spaceId")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let success = result
                        .get("success")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let carrier_id = result
                        .get("carrierId")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let error_msg = result.get("errorMsg").and_then(Value::as_str).unwrap_or("");
                    let mut summary = format!("{space_type}:{space_id}:success={success}");
                    if !carrier_id.trim().is_empty() {
                        summary.push_str(&format!(":carrierId={carrier_id}"));
                    }
                    if !error_msg.trim().is_empty() {
                        summary.push_str(&format!(":error={error_msg}"));
                    }
                    summary
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn ensure_ai_card_delivery_success(body: &Value, context: &str) -> ChannelResult<()> {
    let success = body.get("success").and_then(Value::as_bool).unwrap_or(true);
    if !success {
        return Err(format!("dingtalk ai card {context} failed: body={body}").into());
    }

    let failures = body
        .pointer("/result/deliverResults")
        .and_then(Value::as_array)
        .map(|results| {
            results
                .iter()
                .filter(|result| {
                    !result
                        .get("success")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .map(|result| {
                    let space_type = result
                        .get("spaceType")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let space_id = result
                        .get("spaceId")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let error_msg = result
                        .get("errorMsg")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    format!("{space_type}:{space_id}:{error_msg}")
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if failures.is_empty() {
        return Ok(());
    }

    Err(format!(
        "dingtalk ai card {context} had failed deliver results: {}; body={body}",
        failures.join(", ")
    )
    .into())
}

impl DingtalkApiClient {
    pub(super) fn new(proxy: &DingtalkProxyConfig) -> ChannelResult<Self> {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .timeout(HTTP_REQUEST_TIMEOUT);
        if proxy.enabled {
            let proxy_url = proxy.url.trim();
            if proxy_url.is_empty() {
                return Err("dingtalk proxy.url is required when proxy.enabled=true".into());
            }
            builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
        }
        let http = builder.build()?;
        Ok(Self {
            http,
            open_api_base: DINGTALK_OPEN_API_BASE.to_string(),
            oapi_base: DINGTALK_OAPI_BASE.to_string(),
        })
    }

    #[cfg(test)]
    pub(super) fn with_base_urls(
        proxy: &DingtalkProxyConfig,
        open_api_base: &str,
        oapi_base: &str,
    ) -> ChannelResult<Self> {
        let mut client = Self::new(proxy)?;
        client.open_api_base = open_api_base.trim_end_matches('/').to_string();
        client.oapi_base = oapi_base.trim_end_matches('/').to_string();
        Ok(client)
    }

    pub(super) fn ensure_rustls_crypto_provider() {
        if RUSTLS_PROVIDER_INSTALLED.get().is_some() {
            return;
        }

        if rustls::crypto::ring::default_provider()
            .install_default()
            .is_ok()
        {
            let _ = RUSTLS_PROVIDER_INSTALLED.set(());
        }
    }

    pub(super) async fn open_stream_connection(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> ChannelResult<StreamConnectionTicket> {
        let url = format!("{}{}", self.open_api_base, CONNECTION_OPEN_PATH);
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "clientId": client_id,
                "clientSecret": client_secret,
                "subscriptions": [
                    {
                        "type": "CALLBACK",
                        "topic": "/v1.0/im/bot/messages/get"
                    }
                ],
                "ua": "klaw/dingtalk"
            }))
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "open dingtalk stream connection failed: HTTP {} body={}",
                status, body
            )
            .into());
        }

        let endpoint = body
            .get("endpoint")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or("missing endpoint from dingtalk stream response")?
            .to_string();

        let ticket = body
            .get("ticket")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or("missing ticket from dingtalk stream response")?
            .to_string();

        Ok(StreamConnectionTicket { endpoint, ticket })
    }

    pub(super) async fn fetch_access_token(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> ChannelResult<String> {
        let url = format!("{}{}", self.open_api_base, ACCESS_TOKEN_PATH);
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "appKey": client_id,
                "appSecret": client_secret,
                "clientId": client_id,
                "clientSecret": client_secret,
            }))
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk access token request failed: HTTP {} body={}",
                status, body
            )
            .into());
        }

        body.get("accessToken")
            .or_else(|| body.get("access_token"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| "missing accessToken from dingtalk access token response".into())
    }

    pub(super) async fn download_message_file(
        &self,
        access_token: &str,
        robot_code: &str,
        download_code: &str,
    ) -> ChannelResult<Vec<u8>> {
        let url = format!("{}{}", self.open_api_base, MESSAGE_FILE_DOWNLOAD_PATH);
        let response = self
            .http
            .post(url)
            .header("x-acs-dingtalk-access-token", access_token)
            .json(&serde_json::json!({
                "downloadCode": download_code,
                "robotCode": robot_code,
            }))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            return Err(format!(
                "dingtalk media download failed: HTTP {} body={}",
                status, body
            )
            .into());
        }

        let body: Value = response.json().await?;
        let download_url = body
            .get("downloadUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                format!("missing downloadUrl in dingtalk download response body={body}")
            })?;
        let file_response = self.http.get(download_url).send().await?;
        if !file_response.status().is_success() {
            let status = file_response.status();
            let body = file_response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            return Err(format!(
                "dingtalk media file fetch failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        let bytes = file_response.bytes().await?;
        Ok(bytes.to_vec())
    }

    pub(super) async fn transcribe_audio(
        &self,
        access_token: &str,
        audio_bytes: &[u8],
    ) -> ChannelResult<String> {
        let media_id = self
            .upload_media(
                access_token,
                audio_bytes,
                "voice",
                "voice.wav",
                Some("audio/wav"),
            )
            .await?;
        let url = format!(
            "{}{}?access_token={}",
            self.oapi_base,
            OAPI_ASR_TRANSLATE_PATH,
            urlencoding::encode(access_token)
        );
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "media_id": media_id,
            }))
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk asr translate request failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        let errcode = body.get("errcode").and_then(Value::as_i64).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = body
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(format!(
                "dingtalk asr failed: errcode={errcode} errmsg={errmsg} body={body}"
            )
            .into());
        }
        body.get("result")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("missing result in dingtalk asr response body={body}").into())
    }

    pub(super) async fn upload_media(
        &self,
        access_token: &str,
        bytes: &[u8],
        media_type: &str,
        filename: &str,
        mime_type: Option<&str>,
    ) -> ChannelResult<String> {
        debug!(
            media_type = media_type,
            filename = filename.trim(),
            mime_type = mime_type.unwrap_or("unknown"),
            size_bytes = bytes.len(),
            "calling dingtalk media upload"
        );
        let url = format!(
            "{}{}?access_token={}&type={}",
            self.oapi_base,
            OAPI_MEDIA_UPLOAD_PATH,
            urlencoding::encode(access_token),
            urlencoding::encode(media_type),
        );
        let mut part =
            reqwest::multipart::Part::bytes(bytes.to_vec()).file_name(filename.trim().to_string());
        if let Some(mime_type) = mime_type.map(str::trim).filter(|value| !value.is_empty()) {
            part = part.mime_str(mime_type)?;
        }
        let form = reqwest::multipart::Form::new().part("media", part);
        let response = self.http.post(url).multipart(form).send().await?;
        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk media upload failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        let errcode = body.get("errcode").and_then(Value::as_i64).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = body
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(format!(
                "dingtalk media upload failed: errcode={errcode} errmsg={errmsg} body={body}"
            )
            .into());
        }
        let Some(media_id) = body
            .get("media_id")
            .or_else(|| body.get("mediaId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
        else {
            return Err(
                format!("missing media_id in dingtalk media upload response body={body}").into(),
            );
        };
        debug!(
            media_type = media_type,
            filename = filename.trim(),
            media_id = media_id.as_str(),
            "dingtalk media upload succeeded"
        );
        Ok(media_id)
    }

    pub(super) fn ensure_session_webhook_success(body: &str, context: &str) -> ChannelResult<()> {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let Ok(payload) = serde_json::from_str::<Value>(trimmed) else {
            return Ok(());
        };
        let Some(errcode) = payload.get("errcode").and_then(Value::as_i64) else {
            return Ok(());
        };
        if errcode == 0 {
            return Ok(());
        }
        let errmsg = payload
            .get("errmsg")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        Err(DingtalkApiError::SessionWebhookBusiness {
            context: context.to_string(),
            errcode,
            errmsg: errmsg.to_string(),
            body: payload,
        }
        .into())
    }

    pub(super) async fn send_session_webhook_markdown(
        &self,
        session_webhook: &str,
        title: &str,
        text: &str,
    ) -> ChannelResult<()> {
        let response = self
            .http
            .post(session_webhook)
            .json(&serde_json::json!({
                "msgtype": "markdown",
                "markdown": {
                    "title": title,
                    "text": text,
                }
            }))
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk session webhook send failed with HTTP {}: {}",
                status, body
            )
            .into());
        }
        Self::ensure_session_webhook_success(&body, "markdown send")?;

        Ok(())
    }

    pub(super) async fn send_session_webhook_image_markdown(
        &self,
        session_webhook: &str,
        title: &str,
        media_id: &str,
        caption: Option<&str>,
    ) -> ChannelResult<()> {
        let mut lines = Vec::new();
        if let Some(caption) = caption.map(str::trim).filter(|value| !value.is_empty()) {
            lines.push(caption.to_string());
        }
        lines.push(format!("![]({})", media_id.trim()));
        self.send_session_webhook_markdown(session_webhook, title, &lines.join("\n\n"))
            .await
    }

    pub(super) async fn send_session_webhook_file(
        &self,
        session_webhook: &str,
        media_id: &str,
        file_type: &str,
        filename: &str,
    ) -> ChannelResult<()> {
        let response = self
            .http
            .post(session_webhook)
            .json(&serde_json::json!({
                "msgtype": "file",
                "file": {
                    "fileType": file_type,
                    "fileName": filename,
                    "mediaId": media_id,
                    "media_id": media_id,
                }
            }))
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk session webhook file send failed with HTTP {}: {}",
                status, body
            )
            .into());
        }
        Self::ensure_session_webhook_success(&body, "file send")?;

        Ok(())
    }

    pub(super) async fn send_session_webhook_generic_action_card(
        &self,
        session_webhook: &str,
        title: &str,
        text: &str,
        buttons: &[(String, String)],
    ) -> ChannelResult<()> {
        let buttons = buttons
            .iter()
            .map(|(title, action_url)| {
                serde_json::json!({
                    "title": title,
                    "actionURL": action_url,
                })
            })
            .collect::<Vec<_>>();
        let response = self
            .http
            .post(session_webhook)
            .json(&serde_json::json!({
                "msgtype": "actionCard",
                "actionCard": {
                    "title": title,
                    "text": text,
                    "btnOrientation": "1",
                    "btns": buttons
                }
            }))
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk session webhook actionCard send failed with HTTP {}: {}",
                status, body
            )
            .into());
        }
        Self::ensure_session_webhook_success(&body, "actionCard send")?;
        Ok(())
    }

    pub(super) async fn send_proactive_markdown(
        &self,
        access_token: &str,
        robot_code: &str,
        chat_id: &str,
        title: &str,
        text: &str,
    ) -> ChannelResult<()> {
        let path = match proactive_chat_target_kind(chat_id) {
            ProactiveChatTargetKind::Group => GROUP_MESSAGE_SEND_PATH,
            ProactiveChatTargetKind::User => OTO_MESSAGE_BATCH_SEND_PATH,
        };
        let url = format!("{}{}", self.open_api_base, path);
        let payload = build_proactive_markdown_payload(robot_code, chat_id, title, text);
        let response = self
            .http
            .post(url)
            .header("x-acs-dingtalk-access-token", access_token)
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk proactive markdown send failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        if let Some(errcode) = body.get("errcode").and_then(Value::as_i64)
            && errcode != 0
        {
            let errmsg = body
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(format!(
                "dingtalk proactive markdown send failed: errcode={errcode} errmsg={errmsg} body={body}"
            )
            .into());
        }
        Ok(())
    }

    pub(super) async fn create_and_deliver_ai_card(
        &self,
        access_token: &str,
        template_id: &str,
        robot_code: &str,
        chat_id: &str,
        out_track_id: &str,
        card_data: Value,
    ) -> ChannelResult<()> {
        let url = format!(
            "{}{}",
            self.open_api_base, CARD_INSTANCE_CREATE_AND_DELIVER_PATH
        );
        let payload = build_create_and_deliver_ai_card_payload(
            template_id,
            robot_code,
            chat_id,
            out_track_id,
            card_data,
        );
        let open_space_id = payload
            .get("openSpaceId")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let im_robot_open_space_model = payload
            .get("imRobotOpenSpaceModel")
            .cloned()
            .unwrap_or(Value::Null);
        let im_group_open_space_model = payload
            .get("imGroupOpenSpaceModel")
            .cloned()
            .unwrap_or(Value::Null);
        debug!(
            out_track_id = out_track_id.trim(),
            template_id = template_id.trim(),
            robot_code = robot_code.trim(),
            chat_id = chat_id.trim(),
            open_space_id,
            im_robot_open_space_model = %im_robot_open_space_model,
            im_group_open_space_model = %im_group_open_space_model,
            "calling dingtalk ai card createAndDeliver"
        );
        let response = self
            .http
            .post(url)
            .header("x-acs-dingtalk-access-token", access_token)
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;
        debug!(
            out_track_id = out_track_id.trim(),
            http_status = status.as_u16(),
            deliver_results = ?ai_card_delivery_result_summaries(&body),
            response_body = %body,
            "received dingtalk ai card createAndDeliver response"
        );
        if !status.is_success() {
            return Err(format!(
                "dingtalk ai card createAndDeliver failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        ensure_ai_card_delivery_success(&body, "createAndDeliver")
    }

    pub(super) async fn stream_ai_card(
        &self,
        access_token: &str,
        out_track_id: &str,
        key: &str,
        content: &str,
        is_finalize: bool,
        is_error: bool,
    ) -> ChannelResult<()> {
        let url = format!("{}{}", self.open_api_base, CARD_STREAMING_PATH);
        let guid = Uuid::new_v4().to_string();
        let payload = build_streaming_ai_card_payload(
            out_track_id,
            &guid,
            key,
            content,
            is_finalize,
            is_error,
        );
        debug!(
            out_track_id = out_track_id.trim(),
            guid = guid.as_str(),
            key = key.trim(),
            content_chars = content.chars().count(),
            is_finalize,
            is_error,
            "calling dingtalk ai card streaming update"
        );
        let response = self
            .http
            .put(url)
            .header("x-acs-dingtalk-access-token", access_token)
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;
        debug!(
            out_track_id = out_track_id.trim(),
            guid = guid.as_str(),
            http_status = status.as_u16(),
            is_finalize,
            is_error,
            response_body = %body,
            "received dingtalk ai card streaming update response"
        );
        if !status.is_success() {
            return Err(format!(
                "dingtalk ai card streaming update failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        ensure_ai_card_delivery_success(&body, "streaming update")
    }

    pub(super) fn build_ws_url(endpoint: &str, ticket: &str) -> String {
        if endpoint.contains('?') {
            format!("{endpoint}&ticket={}", urlencoding::encode(ticket))
        } else {
            format!("{endpoint}?ticket={}", urlencoding::encode(ticket))
        }
    }
}
