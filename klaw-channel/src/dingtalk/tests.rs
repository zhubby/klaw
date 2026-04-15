use super::{
    attachments::{
        build_unsupported_file_attachment_markdown, infer_dingtalk_file_type,
        supported_dingtalk_file_type,
    },
    callback_runtime_metadata,
    client::DingtalkApiClient,
    parsing::{
        ApprovalAction, CardCallbackEvent, EventDeduper, InboundEvent,
        build_approval_action_card_body, build_im_card_action_buttons,
        extract_dingtalk_media_references, extract_dingtalk_message_text,
        extract_shell_approval_id, is_sender_allowed, normalize_dingtalk_text_content,
        parse_card_callback_event, parse_inbound_event, parse_stream_data, resolve_approval_card,
        resolve_chat_id, resolve_download_code_candidates,
    },
};
use crate::{
    ChannelResponse,
    im_card::{ImCard, ImCardAction, ImCardActionKind, ImCardKind},
    render::{OutputRenderStyle, render_agent_output},
};
use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;

const BOT_TITLE: &str = "Klaw";

#[test]
fn callback_runtime_metadata_marks_turn_as_isolated() {
    let metadata = callback_runtime_metadata(Some("https://example/session"), BOT_TITLE);
    assert_eq!(
        metadata.get("agent.isolated_turn"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        metadata.get("channel.delivery_mode"),
        Some(&serde_json::Value::String("direct_reply".to_string()))
    );
}

#[test]
fn parse_inbound_text_event_reads_dingtalk_shape() {
    let payload = serde_json::json!({
        "conversationType": 1,
        "conversationId": "cid_1",
        "sessionWebhook": "https://example/session",
        "msgId": "mid_1",
        "robotCode": "robot_1",
        "senderStaffId": "staff_1",
        "text": { "content": "hello" }
    });

    let parsed = parse_inbound_event(&payload, BOT_TITLE).expect("should parse");
    assert_eq!(
        parsed,
        InboundEvent {
            event_id: "mid_1".to_string(),
            chat_id: "staff_1".to_string(),
            robot_code: "robot_1".to_string(),
            msg_type: "text".to_string(),
            sender_id: "staff_1".to_string(),
            session_webhook: "https://example/session".to_string(),
            text: "hello".to_string(),
            audio_recognition: None,
            media_references: Vec::new(),
        }
    );
}

#[test]
fn parse_inbound_group_text_requires_bot_mention() {
    let payload = serde_json::json!({
        "conversationType": 2,
        "conversationId": "cid_group",
        "sessionWebhook": "https://example/session-group",
        "msgId": "mid_group",
        "robotCode": "robot_group",
        "senderStaffId": "staff_group",
        "text": { "content": "hello everyone" }
    });

    assert_eq!(parse_inbound_event(&payload, BOT_TITLE), None);
}

#[test]
fn parse_inbound_group_text_accepts_structured_bot_mention() {
    let payload = serde_json::json!({
        "conversationType": 2,
        "conversationId": "cid_group_mention",
        "sessionWebhook": "https://example/session-group-mention",
        "msgId": "mid_group_mention",
        "robotCode": "robot_group_mention",
        "senderStaffId": "staff_group_mention",
        "atUsers": [
            { "dingtalkId": "robot_group_mention", "staffId": "ignored" }
        ],
        "text": { "content": "@Klaw 帮我查一下" }
    });

    let parsed =
        parse_inbound_event(&payload, BOT_TITLE).expect("should parse mentioned group text");
    assert_eq!(parsed.chat_id, "cid_group_mention");
    assert_eq!(parsed.text, "帮我查一下");
}

#[test]
fn parse_inbound_group_richtext_skips_bot_mention_block() {
    let payload = serde_json::json!({
        "conversationType": 2,
        "conversationId": "cid_rich_at",
        "sessionWebhook": "https://example/session-rich-at",
        "msgId": "mid_rich_at",
        "robotCode": "robot_rich_at",
        "senderStaffId": "staff_rich_at",
        "msgtype": "richText",
        "content": {
            "richText": [
                { "type": "at", "text": "@Klaw", "title": "Klaw", "robotCode": "robot_rich_at" },
                { "type": "text", "text": " 帮我总结下" }
            ]
        }
    });

    let parsed = parse_inbound_event(&payload, BOT_TITLE).expect("should parse richText at");
    assert_eq!(parsed.text, "帮我总结下");
}

#[test]
fn normalize_dingtalk_text_content_strips_fullwidth_and_ascii_mentions() {
    assert_eq!(
        normalize_dingtalk_text_content("@Klaw 你好 ＠Klaw 再问一次", BOT_TITLE),
        "你好 再问一次"
    );
}

#[test]
fn parse_inbound_picture_event_as_fallback_text() {
    let payload = serde_json::json!({
        "conversationType": 1,
        "conversationId": "cid_2",
        "sessionWebhook": "https://example/session2",
        "msgId": "mid_2",
        "robotCode": "robot_2",
        "senderStaffId": "staff_2",
        "msgtype": "picture",
        "picture": { "fileName": "screen.png" }
    });

    let parsed = parse_inbound_event(&payload, BOT_TITLE).expect("should parse picture");
    assert_eq!(parsed.chat_id, "staff_2");
    assert!(parsed.text.contains("图片消息"));
    assert_eq!(parsed.media_references.len(), 1);
    assert_eq!(
        parsed.media_references[0].filename.as_deref(),
        Some("screen.png")
    );
}

#[test]
fn parse_inbound_picture_event_reads_content_download_codes() {
    let payload = serde_json::json!({
        "conversationType": "1",
        "sessionWebhook": "https://example/session4",
        "msgId": "mid_4",
        "robotCode": "robot_4",
        "senderStaffId": "staff_4",
        "msgtype": "picture",
        "content": {
            "downloadCode": "d-code-1",
            "pictureDownloadCode": "p-code-1"
        }
    });

    let parsed = parse_inbound_event(&payload, BOT_TITLE).expect("should parse picture content");
    assert_eq!(parsed.media_references.len(), 1);
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.download_code")
            .and_then(serde_json::Value::as_str),
        Some("d-code-1")
    );
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.picture_download_code")
            .and_then(serde_json::Value::as_str),
        Some("p-code-1")
    );
}

#[test]
fn parse_inbound_richtext_event_extracts_text_and_pictures() {
    let payload = serde_json::json!({
        "conversationType": 1,
        "conversationId": "cid_3",
        "sessionWebhook": "https://example/session3",
        "msgId": "mid_3",
        "robotCode": "robot_3",
        "senderStaffId": "staff_3",
        "msgtype": "richText",
        "content": {
            "richText": [
                { "type": "picture", "downloadCode": "code-1", "pictureDownloadCode": "pcode-1" },
                { "type": "text", "text": "\n" },
                { "type": "picture", "downloadCode": "code-2", "pictureDownloadCode": "pcode-2" },
                { "type": "text", "text": "\n这是啥" }
            ]
        }
    });

    let parsed = parse_inbound_event(&payload, BOT_TITLE).expect("should parse richText");
    assert_eq!(parsed.text, "这是啥");
    assert_eq!(parsed.media_references.len(), 2);
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.download_code")
            .and_then(serde_json::Value::as_str),
        Some("code-1")
    );
    assert_eq!(
        parsed.media_references[1]
            .metadata
            .get("dingtalk.picture_download_code")
            .and_then(serde_json::Value::as_str),
        Some("pcode-2")
    );
}

#[test]
fn parse_inbound_video_event_extracts_media_reference() {
    let payload = serde_json::json!({
        "conversationType": 1,
        "conversationId": "cid_video",
        "sessionWebhook": "https://example/session-video",
        "msgId": "mid_video",
        "robotCode": "robot_video",
        "senderStaffId": "staff_video",
        "msgtype": "video",
        "video": {
            "fileName": "demo.mp4",
            "downloadCode": "video-code-1",
            "mimeType": "video/mp4",
            "fileType": "mp4"
        }
    });

    let parsed = parse_inbound_event(&payload, BOT_TITLE).expect("should parse video");
    assert!(parsed.text.contains("视频消息"));
    assert_eq!(parsed.media_references.len(), 1);
    assert_eq!(
        parsed.media_references[0].filename.as_deref(),
        Some("demo.mp4")
    );
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.download_code")
            .and_then(serde_json::Value::as_str),
        Some("video-code-1")
    );
    assert_eq!(
        parsed.media_references[0].mime_type.as_deref(),
        Some("video/mp4")
    );
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.file_extension")
            .and_then(serde_json::Value::as_str),
        Some("mp4")
    );
}

#[test]
fn parse_inbound_file_event_extracts_media_reference() {
    let payload = serde_json::json!({
        "conversationType": 1,
        "conversationId": "cid_file",
        "sessionWebhook": "https://example/session-file",
        "msgId": "mid_file",
        "robotCode": "robot_file",
        "senderStaffId": "staff_file",
        "msgtype": "file",
        "file": {
            "fileName": "report.xlsx",
            "downloadCode": "file-code-1",
            "contentType": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "extension": "xlsx"
        }
    });

    let parsed = parse_inbound_event(&payload, BOT_TITLE).expect("should parse file");
    assert!(parsed.text.contains("文件消息"));
    assert_eq!(parsed.media_references.len(), 1);
    assert_eq!(
        parsed.media_references[0].filename.as_deref(),
        Some("report.xlsx")
    );
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.download_code")
            .and_then(serde_json::Value::as_str),
        Some("file-code-1")
    );
    assert_eq!(
        parsed.media_references[0].mime_type.as_deref(),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
    );
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.file_extension")
            .and_then(serde_json::Value::as_str),
        Some("xlsx")
    );
}

#[test]
fn parse_inbound_richtext_event_extracts_non_image_attachments() {
    let payload = serde_json::json!({
        "conversationType": 1,
        "conversationId": "cid_rich_file",
        "sessionWebhook": "https://example/session-rich-file",
        "msgId": "mid_rich_file",
        "robotCode": "robot_rich_file",
        "senderStaffId": "staff_rich_file",
        "msgtype": "richText",
        "content": {
            "richText": [
                {
                    "type": "file",
                    "fileName": "slides.pdf",
                    "downloadCode": "file-rich-code",
                    "mimeType": "application/pdf",
                    "fileType": "pdf"
                },
                {
                    "type": "video",
                    "fileName": "walkthrough.mp4",
                    "downloadCode": "video-rich-code",
                    "mimeType": "video/mp4"
                }
            ]
        }
    });

    let parsed =
        parse_inbound_event(&payload, BOT_TITLE).expect("should parse richText file/video");
    assert_eq!(parsed.media_references.len(), 2);
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.richtext_block_type")
            .and_then(serde_json::Value::as_str),
        Some("file")
    );
    assert_eq!(
        parsed.media_references[1]
            .metadata
            .get("dingtalk.richtext_block_type")
            .and_then(serde_json::Value::as_str),
        Some("video")
    );
    assert_eq!(
        parsed.media_references[0].mime_type.as_deref(),
        Some("application/pdf")
    );
    assert_eq!(
        parsed.media_references[0]
            .metadata
            .get("dingtalk.file_extension")
            .and_then(serde_json::Value::as_str),
        Some("pdf")
    );
}

#[test]
fn parse_stream_data_supports_string_payload() {
    let frame_data = serde_json::json!("{\"text\":{\"content\":\"hello\"}}");
    let parsed = parse_stream_data(&frame_data).expect("should parse");
    assert_eq!(
        parsed.get("text").and_then(|v| v.get("content")),
        Some(&serde_json::json!("hello"))
    );
}

#[test]
fn event_deduper_rejects_duplicates_within_ttl() {
    let mut deduper = EventDeduper::new(Duration::from_millis(30), 10);
    assert!(deduper.insert_if_new("evt-1"));
    assert!(!deduper.insert_if_new("evt-1"));
}

#[test]
fn event_deduper_expires_entries_after_ttl() {
    let mut deduper = EventDeduper::new(Duration::from_millis(5), 10);
    assert!(deduper.insert_if_new("evt-1"));
    thread::sleep(Duration::from_millis(8));
    assert!(deduper.insert_if_new("evt-1"));
}

#[test]
fn event_deduper_evicts_oldest_when_capacity_reached() {
    let mut deduper = EventDeduper::new(Duration::from_secs(5), 2);
    assert!(deduper.insert_if_new("evt-1"));
    assert!(deduper.insert_if_new("evt-2"));
    assert!(deduper.insert_if_new("evt-3"));
    assert!(deduper.insert_if_new("evt-1"));
}

#[test]
fn resolve_chat_id_handles_private_chat() {
    let data = serde_json::json!({
        "conversationType": "1",
        "conversationId": "cid-group",
    });
    assert_eq!(resolve_chat_id(&data, "staff-1"), "staff-1");
}

#[test]
fn sender_allowlist_supports_wildcard() {
    assert!(is_sender_allowed(&["*".to_string()], "staff-1"));
    assert!(!is_sender_allowed(&["staff-2".to_string()], "staff-1"));
}

#[test]
fn render_reasoning_when_enabled() {
    let output = render_agent_output(
        &ChannelResponse {
            content: "done".to_string(),
            reasoning: Some("step1\nstep2".to_string()),
            metadata: BTreeMap::new(),
            attachments: Vec::new(),
        },
        true,
        OutputRenderStyle::Markdown,
    );
    assert!(output.contains("done"));
    assert!(output.contains("**Reasoning**"));
    assert!(output.contains("> step1"));
}

#[test]
fn build_ws_url_appends_ticket() {
    assert_eq!(
        DingtalkApiClient::build_ws_url("wss://example/ws", "abc=="),
        "wss://example/ws?ticket=abc%3D%3D"
    );
    assert_eq!(
        DingtalkApiClient::build_ws_url("wss://example/ws?v=1", "abc=="),
        "wss://example/ws?v=1&ticket=abc%3D%3D"
    );
}

#[test]
fn session_webhook_success_accepts_errcode_zero_json() {
    DingtalkApiClient::ensure_session_webhook_success(
        r#"{"errcode":0,"errmsg":"ok"}"#,
        "markdown send",
    )
    .expect("errcode 0 should succeed");
}

#[test]
fn session_webhook_success_rejects_non_zero_errcode_json() {
    let err = DingtalkApiClient::ensure_session_webhook_success(
        r#"{"errcode":310000,"errmsg":"invalid session"}"#,
        "markdown send",
    )
    .expect_err("non-zero errcode should fail");
    assert!(err.to_string().contains("errcode=310000"));
    assert!(err.to_string().contains("invalid session"));
}

#[test]
fn infer_dingtalk_file_type_prefers_filename_extension() {
    assert_eq!(
        infer_dingtalk_file_type("江苏电信电子发票-202601031936.PDF", Some("application/pdf")),
        "pdf"
    );
}

#[test]
fn infer_dingtalk_file_type_falls_back_to_mime_subtype() {
    assert_eq!(
        infer_dingtalk_file_type(
            "attachment",
            Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        ),
        "vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(
        infer_dingtalk_file_type("attachment", Some("application/json+zip")),
        "zip"
    );
}

#[test]
fn infer_dingtalk_file_type_defaults_to_bin() {
    assert_eq!(infer_dingtalk_file_type("attachment", None), "bin");
}

#[test]
fn supported_dingtalk_file_type_accepts_documented_extensions() {
    assert_eq!(
        supported_dingtalk_file_type("report.pdf", None),
        Some("pdf")
    );
    assert_eq!(
        supported_dingtalk_file_type("report.doc", None),
        Some("doc")
    );
    assert_eq!(
        supported_dingtalk_file_type("report.docx", None),
        Some("docx")
    );
    assert_eq!(
        supported_dingtalk_file_type("report.xlsx", None),
        Some("xlsx")
    );
    assert_eq!(
        supported_dingtalk_file_type("report.zip", None),
        Some("zip")
    );
    assert_eq!(
        supported_dingtalk_file_type("report.rar", None),
        Some("rar")
    );
}

#[test]
fn supported_dingtalk_file_type_rejects_other_extensions() {
    assert_eq!(supported_dingtalk_file_type("slides.pptx", None), None);
    assert_eq!(
        supported_dingtalk_file_type("notes.txt", Some("text/plain")),
        None
    );
}

#[test]
fn unsupported_file_attachment_markdown_mentions_supported_types() {
    let markdown = build_unsupported_file_attachment_markdown("slides.pptx", Some("附件如下"));
    assert!(markdown.contains("附件如下"));
    assert!(markdown.contains("pdf/doc/docx/xlsx/zip/rar"));
    assert!(markdown.contains("slides.pptx"));
}

#[test]
fn non_text_messages_fall_back_to_summary() {
    let payload = serde_json::json!({
        "audio": { "duration": 8 }
    });
    let text = extract_dingtalk_message_text(&payload, "audio", None, "robot-a1", BOT_TITLE)
        .expect("audio fallback");
    assert!(text.contains("语音消息"));
}

#[test]
fn audio_message_exposes_media_placeholder() {
    let payload = serde_json::json!({
        "audio": { "duration": 8 }
    });
    let media = extract_dingtalk_media_references(&payload, "audio", "msg-a1");
    assert_eq!(media.len(), 1);
    assert_eq!(media[0].message_id.as_deref(), Some("msg-a1"));
    assert_eq!(
        media[0]
            .metadata
            .get("dingtalk.duration_seconds")
            .and_then(serde_json::Value::as_i64),
        Some(8)
    );
}

#[test]
fn resolve_download_code_prefers_download_code() {
    let metadata = BTreeMap::from([
        (
            "dingtalk.download_code".to_string(),
            serde_json::json!("download-code"),
        ),
        (
            "dingtalk.picture_download_code".to_string(),
            serde_json::json!("picture-code"),
        ),
    ]);
    let resolved = resolve_download_code_candidates(&metadata)
        .into_iter()
        .next()
        .expect("should resolve");
    assert_eq!(resolved.0, "download-code");
    assert_eq!(resolved.1, "download_code");
}

#[test]
fn audio_message_prefers_recognition_text() {
    let payload = serde_json::json!({
        "content": { "recognition": "这是一段语音转文字" },
        "audio": { "duration": 5 }
    });
    let text = extract_dingtalk_message_text(
        &payload,
        "audio",
        Some("这是一段语音转文字"),
        "robot-a2",
        BOT_TITLE,
    )
    .expect("audio recognition");
    assert_eq!(text, "这是一段语音转文字");
}

#[test]
fn extract_shell_approval_id_from_text() {
    let content =
        "approval required: approval_id=123e4567-e89b-12d3-a456-426614174000; retry later";
    let approval_id = extract_shell_approval_id(content).expect("approval id");
    assert_eq!(approval_id, "123e4567-e89b-12d3-a456-426614174000");
}

#[test]
fn extract_shell_approval_id_from_json_payload() {
    let content = serde_json::json!({
        "action": "request",
        "approval": { "id": "approval-json-1" }
    })
    .to_string();
    let approval_id = extract_shell_approval_id(&content).expect("approval id");
    assert_eq!(approval_id, "approval-json-1");
}

#[test]
fn extract_shell_approval_id_from_json_approval_id_camel_case() {
    let content = serde_json::json!({
        "action": "request",
        "approvalId": "approval-json-2"
    })
    .to_string();
    let approval_id = extract_shell_approval_id(&content).expect("approval id");
    assert_eq!(approval_id, "approval-json-2");
}

#[test]
fn extract_shell_approval_id_from_natural_language() {
    let content =
        "我已经请求了批准。审批 ID 是 e4d1e3bf-2d00-49da-b091-23e818a83483。请您批准该操作。";
    let approval_id = extract_shell_approval_id(content).expect("approval id");
    assert_eq!(approval_id, "e4d1e3bf-2d00-49da-b091-23e818a83483");
}

#[test]
fn extract_shell_approval_id_from_natural_language_approve_wording() {
    let content =
        "我已经请求批准来执行浏览器自动化任务。批准ID: 3a24e1d4-9c94-4ee1-ac16-1f750ca78acf";
    let approval_id = extract_shell_approval_id(content).expect("approval id");
    assert_eq!(approval_id, "3a24e1d4-9c94-4ee1-ac16-1f750ca78acf");
}

#[test]
fn extract_approval_id_for_action_card_prefers_structured_metadata() {
    let output = ChannelResponse {
        content: "approval required: approval_id=from-content".to_string(),
        reasoning: None,
        metadata: BTreeMap::from([(
            "approval.id".to_string(),
            serde_json::json!("from-metadata"),
        )]),
        attachments: Vec::new(),
    };
    let card = resolve_approval_card(&output).expect("approval card");
    assert_eq!(card.approval_id(), Some("from-metadata"));
}

#[test]
fn extract_approval_id_for_action_card_falls_back_to_content() {
    let output = ChannelResponse {
        content: "approval required: approval_id=from-content".to_string(),
        reasoning: None,
        metadata: BTreeMap::new(),
        attachments: Vec::new(),
    };
    let card = resolve_approval_card(&output).expect("approval card");
    assert_eq!(card.approval_id(), Some("from-content"));
}

#[test]
fn extract_approval_command_preview_reads_structured_metadata() {
    let output = ChannelResponse {
        content: "approval required".to_string(),
        reasoning: None,
        metadata: BTreeMap::from([(
            "approval.signal".to_string(),
            serde_json::json!({
                "approval_id": "approval-1",
                "command_preview": "pip3 install pymupdf -q"
            }),
        )]),
        attachments: Vec::new(),
    };
    let card = resolve_approval_card(&output).expect("approval card");
    assert_eq!(card.command_preview(), Some("pip3 install pymupdf -q"));
}

#[test]
fn build_approval_action_card_body_includes_command_preview() {
    let output = ChannelResponse {
        content: "This shell command requires approval.".to_string(),
        reasoning: None,
        metadata: BTreeMap::from([(
            "approval.signal".to_string(),
            serde_json::json!({
                "approval_id": "approval-1",
                "command_preview": "python3 -c \"print(1)\""
            }),
        )]),
        attachments: Vec::new(),
    };
    let card = resolve_approval_card(&output).expect("approval card");
    let body = build_approval_action_card_body(&card);
    assert!(body.contains("待执行命令"));
    assert!(body.contains("```\npython3 -c \"print(1)\"\n```"));
    assert!(body.contains("审批单: `approval-1`"));
}

#[test]
fn build_im_card_action_buttons_supports_commands_and_urls() {
    let card = ImCard {
        kind: ImCardKind::QuestionSingleSelect,
        title: Some("Pick one".to_string()),
        body: "Question body".to_string(),
        actions: vec![
            ImCardAction {
                kind: ImCardActionKind::SubmitCommand,
                label: Some("A".to_string()),
                value: None,
                url: None,
                command: Some("/card_answer q-1 a".to_string()),
            },
            ImCardAction {
                kind: ImCardActionKind::OpenUrl,
                label: Some("Docs".to_string()),
                value: None,
                url: Some("https://example.com/docs".to_string()),
                command: None,
            },
        ],
        fallback_text: None,
        metadata: BTreeMap::new(),
    };

    let buttons = build_im_card_action_buttons(&card);
    assert_eq!(buttons[0].0, "A");
    assert!(
        buttons[0]
            .1
            .starts_with("dtmd://dingtalkclient/sendMessage?content=")
    );
    assert_eq!(buttons[1].0, "Docs");
    assert_eq!(buttons[1].1, "https://example.com/docs");
}

#[test]
fn parse_card_callback_event_reads_approve_token() {
    let payload = serde_json::json!({
        "eventId": "evt-1",
        "conversationId": "cid-1",
        "sessionWebhook": "https://example/session",
        "senderStaffId": "staff-1",
        "value": "approve_approval-123"
    });
    let parsed = parse_card_callback_event(&payload).expect("callback");
    assert_eq!(
        parsed,
        CardCallbackEvent {
            event_id: Some("evt-1".to_string()),
            action: ApprovalAction::Approve,
            approval_id: "approval-123".to_string(),
            sender_id: "staff-1".to_string(),
            chat_id: "cid-1".to_string(),
            session_webhook: Some("https://example/session".to_string()),
        }
    );
}

#[test]
fn parse_card_callback_event_reads_reject_token_from_nested_payload() {
    let payload = serde_json::json!({
        "userId": "staff-2",
        "callbackData": {
            "action": "reject:approval-99"
        }
    });
    let parsed = parse_card_callback_event(&payload).expect("callback");
    assert_eq!(parsed.action, ApprovalAction::Reject);
    assert_eq!(parsed.approval_id, "approval-99");
    assert_eq!(parsed.chat_id, "staff-2");
    assert_eq!(parsed.sender_id, "staff-2");
    assert!(parsed.session_webhook.is_none());
}
