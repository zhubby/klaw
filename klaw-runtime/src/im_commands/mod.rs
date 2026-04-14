mod parser;
mod shell;
mod usage;

use super::*;
use klaw_storage::{SessionStorage, ToolAuditQuery, ToolAuditRecord, ToolAuditSortOrder};
use klaw_tool::ToolOutput;

use parser::now_ms;
use shell::execute_im_shell;
use usage::usage_response;

pub(super) fn parse_im_command(input: &str) -> Option<(&str, Option<&str>)> {
    parser::parse_im_command(input)
}

pub(super) fn first_arg_token(arg: Option<&str>) -> Option<&str> {
    parser::first_arg_token(arg)
}

pub(super) fn second_arg_token(arg: Option<&str>) -> Option<&str> {
    parser::second_arg_token(arg)
}

fn parse_chat_record_metadata_json(raw: &str) -> Option<BTreeMap<String, Value>> {
    serde_json::from_str::<serde_json::Map<String, Value>>(raw)
        .ok()
        .map(|metadata| metadata.into_iter().collect())
}

fn is_approval_prompt_record(record: &ChatRecord, approval_id: &str) -> bool {
    if record.role != "assistant" {
        return false;
    }
    let Some(metadata) = record
        .metadata_json
        .as_deref()
        .and_then(parse_chat_record_metadata_json)
    else {
        return false;
    };
    let is_approval = metadata
        .get("approval.required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let matches_approval_id = metadata
        .get("approval.id")
        .and_then(Value::as_str)
        .is_some_and(|value| value == approval_id);
    is_approval && matches_approval_id
}

#[derive(Debug, Clone)]
struct ApprovedToolReplay {
    tool_name: String,
    tool_call_id: String,
    arguments: Value,
    tool_message_content: String,
}

fn parse_tool_signals_json(raw: Option<&str>) -> Vec<klaw_tool::ToolSignal> {
    raw.and_then(|value| serde_json::from_str::<Vec<klaw_tool::ToolSignal>>(value).ok())
        .unwrap_or_default()
}

fn parse_tool_call_id(metadata_json: Option<&str>) -> Option<String> {
    metadata_json
        .and_then(parse_chat_record_metadata_json)
        .and_then(|metadata| {
            metadata
                .get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
}

fn tool_audit_matches_approval(
    audit: &ToolAuditRecord,
    tool_name: &str,
    approval_id: &str,
) -> bool {
    if audit.tool_name != tool_name || !audit.approval_required {
        return false;
    }
    parse_tool_signals_json(audit.signals_json.as_deref())
        .into_iter()
        .filter(|signal| signal.kind == "approval_required")
        .any(|signal| {
            signal
                .payload
                .get("approval_id")
                .and_then(Value::as_str)
                .is_some_and(|value| value == approval_id)
        })
}

async fn find_approved_tool_audit(
    runtime: &RuntimeBundle,
    session_key: &str,
    tool_name: &str,
    approval_id: &str,
) -> Option<ToolAuditRecord> {
    runtime
        .session_store
        .list_tool_audit(&ToolAuditQuery {
            session_key: Some(session_key.to_string()),
            tool_name: Some(tool_name.to_string()),
            started_from_ms: None,
            started_to_ms: None,
            limit: 100,
            offset: 0,
            sort_order: ToolAuditSortOrder::StartedAtDesc,
        })
        .await
        .ok()?
        .into_iter()
        .find(|audit| tool_audit_matches_approval(audit, tool_name, approval_id))
}

fn replay_tool_result_content(
    tool_name: &str,
    output: Result<ToolOutput, klaw_tool::ToolError>,
) -> String {
    match output {
        Ok(output) => {
            let signals = output
                .signals
                .into_iter()
                .map(|signal| klaw_agent::ToolInvocationSignal {
                    kind: signal.kind,
                    payload: signal.payload,
                })
                .collect::<Vec<_>>();
            let result = if signals.is_empty() {
                klaw_agent::ToolInvocationResult::success(output.content_for_model)
            } else {
                klaw_agent::ToolInvocationResult::success_with_signals(
                    output.content_for_model,
                    signals,
                )
            };
            result.to_tool_message_content(tool_name)
        }
        Err(err) => klaw_agent::ToolInvocationResult::error(
            err.message().to_string(),
            err.code().to_string(),
            err.details().cloned(),
            err.retryable(),
            err.signals()
                .iter()
                .cloned()
                .map(|signal| klaw_agent::ToolInvocationSignal {
                    kind: signal.kind,
                    payload: signal.payload,
                })
                .collect(),
        )
        .to_tool_message_content(tool_name),
    }
}

async fn replay_approved_tool(
    runtime: &RuntimeBundle,
    approval_id: &str,
    session_key: &str,
    tool_name: &str,
    command_text: &str,
) -> Result<Option<ApprovedToolReplay>, Box<dyn Error>> {
    let audit = find_approved_tool_audit(runtime, session_key, tool_name, approval_id).await;
    let arguments = audit
        .as_ref()
        .and_then(|record| serde_json::from_str::<Value>(&record.arguments_json).ok())
        .or_else(|| (tool_name == "shell").then(|| json!({ "command": command_text })));
    let Some(arguments) = arguments else {
        return Ok(None);
    };
    let tool_call_id = audit
        .as_ref()
        .and_then(|record| parse_tool_call_id(record.metadata_json.as_deref()))
        .unwrap_or_else(|| format!("approved-{tool_name}-{approval_id}"));
    let Some(tool) = runtime.runtime.tools.get(tool_name) else {
        return Ok(Some(ApprovedToolReplay {
            tool_name: tool_name.to_string(),
            tool_call_id,
            arguments,
            tool_message_content: klaw_agent::ToolInvocationResult::error(
                format!("tool `{tool_name}` not found"),
                "tool_not_found".to_string(),
                None,
                false,
                Vec::new(),
            )
            .to_tool_message_content(tool_name),
        }));
    };
    let mut metadata = BTreeMap::new();
    metadata.insert("approval.id".to_string(), Value::String(approval_id.to_string()));
    metadata.insert(
        "approval.tool_name".to_string(),
        Value::String(tool_name.to_string()),
    );
    metadata.insert("approval.approved".to_string(), Value::Bool(true));
    if tool_name == "shell" {
        metadata.insert(
            "shell.approval_id".to_string(),
            Value::String(approval_id.to_string()),
        );
    }
    let tool_message_content = replay_tool_result_content(
        tool_name,
        tool.execute(
            arguments.clone(),
            &ToolContext {
                session_key: session_key.to_string(),
                metadata,
            },
        )
        .await,
    );
    Ok(Some(ApprovedToolReplay {
        tool_name: tool_name.to_string(),
        tool_call_id,
        arguments,
        tool_message_content,
    }))
}

fn build_approved_tool_resume_history(
    mut full_history: Vec<ChatRecord>,
    approval_id: &str,
    replay: &ApprovedToolReplay,
) -> Vec<ConversationMessage> {
    if full_history
        .last()
        .is_some_and(|record| is_approval_prompt_record(record, approval_id))
    {
        full_history.pop();
    }
    let mut conversation_history = to_conversation_messages(&full_history);
    conversation_history.push(ConversationMessage {
        role: "assistant".to_string(),
        content: String::new(),
        tool_calls: Some(vec![klaw_llm::ToolCall {
            id: Some(replay.tool_call_id.clone()),
            name: replay.tool_name.clone(),
            arguments: replay.arguments.clone(),
        }]),
        tool_call_id: None,
    });
    conversation_history.push(ConversationMessage {
        role: "tool".to_string(),
        content: replay.tool_message_content.clone(),
        tool_calls: None,
        tool_call_id: Some(replay.tool_call_id.clone()),
    });
    conversation_history
}

async fn submit_approved_tool_resume(
    runtime: &RuntimeBundle,
    followup_channel: String,
    followup_chat_id: String,
    session_key: String,
    model_provider: String,
    model: String,
    request_metadata: BTreeMap<String, Value>,
    approval_id: &str,
    tool_name: &str,
    command_text: &str,
) -> Result<Option<AssistantOutput>, Box<dyn Error>> {
    let Some(replay) =
        replay_approved_tool(runtime, approval_id, &session_key, tool_name, command_text).await?
    else {
        return Ok(None);
    };
    let full_history = session_manager(runtime).read_chat_records(&session_key).await?;
    let conversation_history = build_approved_tool_resume_history(full_history, approval_id, &replay);
    let outcome = submit_history_only_turn_outcome(
        runtime,
        followup_channel,
        session_key,
        followup_chat_id,
        "local-user".to_string(),
        model_provider,
        model,
        Vec::new(),
        inherited_channel_runtime_metadata(&request_metadata),
        conversation_history,
    )
    .await?;
    Ok(outcome.output)
}

fn approval_link_metadata_matches_route(
    metadata: &BTreeMap<String, Value>,
    route: &SessionRoute,
    base_session_key: &str,
) -> bool {
    let linked_base = metadata
        .get("channel.base_session_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let linked_delivery = metadata
        .get("channel.delivery_session_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if linked_base.is_none() && linked_delivery.is_none() {
        return false;
    }
    let base_matches = linked_base.is_some_and(|value| value == base_session_key);
    let delivery_matches =
        linked_delivery.is_some_and(|value| value == route.active_session_key.as_str());
    let base_conflict = linked_base.is_some_and(|value| value != base_session_key);
    let delivery_conflict =
        linked_delivery.is_some_and(|value| value != route.active_session_key.as_str());
    (base_matches || delivery_matches) && !base_conflict && !delivery_conflict
}

async fn approval_belongs_to_route(
    runtime: &RuntimeBundle,
    approval_session_key: &str,
    route: &SessionRoute,
    base_session_key: &str,
) -> bool {
    if approval_session_key == route.active_session_key || approval_session_key == base_session_key
    {
        return true;
    }
    let sessions = session_manager(runtime);
    let Ok(session) = sessions.get_session(approval_session_key).await else {
        return false;
    };
    let metadata = session
        .delivery_metadata_json
        .as_deref()
        .and_then(parse_delivery_metadata_json)
        .unwrap_or_default();
    approval_link_metadata_matches_route(&metadata, route, base_session_key)
}

async fn approval_followup_context(
    runtime: &RuntimeBundle,
    approval_session_key: &str,
    fallback_channel: &str,
    fallback_chat_id: &str,
) -> (String, String, BTreeMap<String, Value>) {
    let sessions = session_manager(runtime);
    let Ok(session) = sessions.get_session(approval_session_key).await else {
        return (
            fallback_channel.to_string(),
            fallback_chat_id.to_string(),
            BTreeMap::new(),
        );
    };
    let metadata = session
        .delivery_metadata_json
        .as_deref()
        .and_then(parse_delivery_metadata_json)
        .unwrap_or_default();
    (session.channel, session.chat_id, metadata)
}

pub(super) async fn try_handle(
    runtime: &RuntimeBundle,
    channel: String,
    base_session_key: String,
    chat_id: String,
    input: String,
    request_metadata: BTreeMap<String, Value>,
) -> Result<Option<ChannelResponse>, Box<dyn Error>> {
    if parse_im_command(&input).is_none() {
        return Ok(None);
    }

    handle_im_command(
        runtime,
        channel,
        base_session_key,
        chat_id,
        input,
        request_metadata,
    )
    .await
}

pub(super) async fn handle_im_command(
    runtime: &RuntimeBundle,
    channel: String,
    base_session_key: String,
    chat_id: String,
    input: String,
    request_metadata: BTreeMap<String, Value>,
) -> Result<Option<ChannelResponse>, Box<dyn Error>> {
    let Some((command, arg)) = parse_im_command(&input) else {
        return Ok(None);
    };
    let route = resolve_session_route(runtime, &channel, &base_session_key, &chat_id).await?;
    let response = match command {
        "help" => channel_response(render_help_text(runtime), None, BTreeMap::new()),
        "stop" => channel_response(
            "Current turn stopped manually. No further tool calls were made.".to_string(),
            None,
            stopped_turn_metadata("manual stop command", "im_command"),
        ),
        "new" | "start" => {
            let new_session_key = format!("{base_session_key}:{}", Uuid::new_v4().simple());
            let (new_session_provider, new_session_model) = resolve_new_session_target(runtime);
            let sessions = session_manager(runtime);
            sessions
                .get_or_create_session_state(
                    &new_session_key,
                    &chat_id,
                    &channel,
                    &new_session_provider,
                    &new_session_model,
                )
                .await?;
            sessions
                .set_active_session(&base_session_key, &chat_id, &channel, &new_session_key)
                .await?;
            let bootstrap_input = build_new_session_bootstrap_user_message();
            match submit_and_get_output(
                runtime,
                channel.clone(),
                bootstrap_input,
                new_session_key.clone(),
                chat_id.clone(),
                "local-user".to_string(),
                new_session_provider.clone(),
                new_session_model.clone(),
                Vec::new(),
                build_new_session_bootstrap_request_metadata(&request_metadata),
            )
            .await
            {
                Ok(Some(output)) => channel_response(
                    format_new_session_started_message(
                        &new_session_key,
                        &new_session_provider,
                        &new_session_model,
                        Some(&output.content),
                    ),
                    output.reasoning,
                    output.metadata,
                ),
                Ok(None) => channel_response(
                    format_new_session_started_message(
                        &new_session_key,
                        &new_session_provider,
                        &new_session_model,
                        None,
                    ),
                    None,
                    BTreeMap::new(),
                ),
                Err(err) => channel_response(
                    format!(
                        "{}\n\n⚠️ Session bootstrap reply failed: {}",
                        format_new_session_started_message(
                            &new_session_key,
                            &new_session_provider,
                            &new_session_model,
                            None,
                        ),
                        err
                    ),
                    None,
                    BTreeMap::new(),
                ),
            }
        }
        "usage" => usage_response(runtime, &route.active_session_key).await?,
        "shell" => {
            let Some(command_text) = arg.map(str::trim).filter(|value| !value.is_empty()) else {
                return Ok(Some(channel_response(
                    "❌ Usage: `/shell <command>`".to_string(),
                    None,
                    BTreeMap::new(),
                )));
            };
            let output = execute_im_shell(runtime, &route.active_session_key, command_text).await?;
            channel_response(output, None, BTreeMap::new())
        }
        "model_provider" => {
            let provider_runtime = provider_runtime_snapshot(runtime);
            if provider_runtime.provider_default_models.len() <= 1 && arg.is_none() {
                return Ok(Some(channel_response(
                    "ℹ️ Only one provider is configured, so switching is not required.".to_string(),
                    None,
                    BTreeMap::new(),
                )));
            }
            if let Some(provider_id) = first_arg_token(arg) {
                let Some(default_model) = provider_runtime.provider_default_models.get(provider_id)
                else {
                    let all = provider_runtime
                        .provider_default_models
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Ok(Some(channel_response(
                        format!("❌ Unknown provider: `{provider_id}`\n🧩 Available: {all}"),
                        None,
                        BTreeMap::new(),
                    )));
                };
                let sessions = session_manager(runtime);
                let (global_provider, global_model) = runtime_default_route(runtime);
                if provider_id == global_provider && default_model == &global_model {
                    sessions
                        .clear_model_routing_override(&route.active_session_key, &chat_id, &channel)
                        .await?;
                } else {
                    sessions
                        .set_model_provider(
                            &route.active_session_key,
                            &chat_id,
                            &channel,
                            provider_id,
                            default_model,
                        )
                        .await?;
                }
                channel_response(
                    format!(
                        "✅ **Provider switched**\n\n🧩 Provider: `{provider_id}`\n🤖 Model: `{default_model}`"
                    ),
                    None,
                    BTreeMap::new(),
                )
            } else {
                let lines = provider_runtime
                    .provider_default_models
                    .iter()
                    .map(|(id, model)| {
                        if id == &route.model_provider {
                            format!("• `{id}`  ← current (default: `{model}`)")
                        } else {
                            format!("• `{id}`  (default: `{model}`)")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                channel_response(
                    format!("🧩 **Providers**\n\n{lines}"),
                    None,
                    BTreeMap::new(),
                )
            }
        }
        "model" => {
            if let Some(model) = arg.map(str::trim).filter(|value| !value.is_empty()) {
                if model.trim().is_empty() {
                    return Ok(Some(channel_response(
                        "❌ Model name cannot be empty.".to_string(),
                        None,
                        BTreeMap::new(),
                    )));
                }
                let sessions = session_manager(runtime);
                let (global_provider, global_model) = runtime_default_route(runtime);
                if route.model_provider == global_provider && model == global_model {
                    sessions
                        .clear_model_routing_override(&route.active_session_key, &chat_id, &channel)
                        .await?;
                } else {
                    sessions
                        .set_model_provider(
                            &route.active_session_key,
                            &chat_id,
                            &channel,
                            &route.model_provider,
                            model,
                        )
                        .await?;
                }
                channel_response(
                    format!(
                        "✅ **Model updated**\n\n🧩 Provider: `{}`\n🤖 Model: `{model}`",
                        route.model_provider
                    ),
                    None,
                    BTreeMap::new(),
                )
            } else {
                channel_response(
                    format!(
                        "🤖 **Current model**\n\n🧩 Provider: `{}`\n🤖 Model: `{}`",
                        route.model_provider, route.model
                    ),
                    None,
                    BTreeMap::new(),
                )
            }
        }
        "approve" => {
            let Some(approval_id) = first_arg_token(arg) else {
                return Ok(Some(channel_response(
                    "❌ Usage: `/approve <approval_id>`".to_string(),
                    None,
                    BTreeMap::new(),
                )));
            };
            let manager = approval_manager(runtime);
            let approval = match manager.get_approval(approval_id).await {
                Ok(approval) => approval,
                Err(_) => {
                    return Ok(Some(channel_response(
                        format!("❌ Approval not found: `{approval_id}`"),
                        None,
                        BTreeMap::new(),
                    )));
                }
            };
            if !approval_belongs_to_route(runtime, &approval.session_key, &route, &base_session_key)
                .await
            {
                return Ok(Some(channel_response(
                    format!("❌ Approval `{approval_id}` does not belong to current session."),
                    None,
                    BTreeMap::new(),
                )));
            }
            match approval.status {
                ApprovalStatus::Pending => {
                    if approval.expires_at_ms < now_ms() {
                        let _ = manager
                            .resolve_approval(
                                approval_id,
                                ApprovalResolveDecision::Approve,
                                Some("channel-user"),
                                now_ms(),
                            )
                            .await?;
                        return Ok(Some(channel_response(
                            format!("⌛ Approval expired: `{approval_id}`"),
                            None,
                            BTreeMap::new(),
                        )));
                    }
                    let approved = manager
                        .resolve_approval(
                            approval_id,
                            ApprovalResolveDecision::Approve,
                            Some("channel-user"),
                            now_ms(),
                        )
                        .await?
                        .approval;
                    let (followup_channel, followup_chat_id, stored_delivery_metadata) =
                        approval_followup_context(
                            runtime,
                            &approved.session_key,
                            &channel,
                            &chat_id,
                        )
                        .await;
                    let mut followup_request_metadata =
                        inherited_channel_runtime_metadata(&request_metadata);
                    for (key, value) in stored_delivery_metadata {
                        if key.starts_with("channel.") || key.starts_with("webhook.") {
                            followup_request_metadata.entry(key).or_insert(value);
                        }
                    }
                    let maybe_output = submit_approved_tool_resume(
                        runtime,
                        followup_channel,
                        followup_chat_id,
                        approved.session_key.clone(),
                        route.model_provider.clone(),
                        route.model.clone(),
                        followup_request_metadata,
                        &approved.id,
                        &approved.tool_name,
                        &approved.command_text,
                    )
                    .await?;
                    match maybe_output {
                        Some(output) => {
                            channel_response(output.content, output.reasoning, output.metadata)
                        }
                        None => channel_response(
                            format!(
                                "✅ Approval granted: `{}` (`{}`).\n\n找不到原始工具调用审计，无法自动恢复；请重新触发该操作。",
                                approved.id, approved.tool_name
                            ),
                            None,
                            BTreeMap::new(),
                        ),
                    }
                }
                other => channel_response(
                    format_approve_already_handled_message(approval_id, other),
                    None,
                    BTreeMap::new(),
                ),
            }
        }
        "reject" => {
            let Some(approval_id) = first_arg_token(arg) else {
                return Ok(Some(channel_response(
                    "❌ Usage: `/reject <approval_id>`".to_string(),
                    None,
                    BTreeMap::new(),
                )));
            };
            let manager = approval_manager(runtime);
            let approval = match manager.get_approval(approval_id).await {
                Ok(approval) => approval,
                Err(_) => {
                    return Ok(Some(channel_response(
                        format!("❌ Approval not found: `{approval_id}`"),
                        None,
                        BTreeMap::new(),
                    )));
                }
            };
            if !approval_belongs_to_route(runtime, &approval.session_key, &route, &base_session_key)
                .await
            {
                return Ok(Some(channel_response(
                    format!("❌ Approval `{approval_id}` does not belong to current session."),
                    None,
                    BTreeMap::new(),
                )));
            }
            match approval.status {
                ApprovalStatus::Pending => {
                    if approval.expires_at_ms < now_ms() {
                        let _ = manager
                            .resolve_approval(
                                approval_id,
                                ApprovalResolveDecision::Reject,
                                Some("channel-user"),
                                now_ms(),
                            )
                            .await?;
                        return Ok(Some(channel_response(
                            format!("⌛ Approval expired: `{approval_id}`"),
                            None,
                            BTreeMap::new(),
                        )));
                    }
                    manager
                        .resolve_approval(
                            approval_id,
                            ApprovalResolveDecision::Reject,
                            Some("channel-user"),
                            now_ms(),
                        )
                        .await?;
                    channel_response(
                        format!(
                            "🛑 Approval rejected: `{approval_id}` (`{}`).",
                            approval.tool_name
                        ),
                        None,
                        BTreeMap::new(),
                    )
                }
                other => channel_response(
                    format!(
                        "ℹ️ Approval `{approval_id}` is already `{}`.",
                        other.as_str()
                    ),
                    None,
                    BTreeMap::new(),
                ),
            }
        }
        "card_answer" => {
            let Some(question_id) = first_arg_token(arg) else {
                return Ok(Some(channel_response(
                    "❌ Usage: `/card_answer <question_id> <option_id>`".to_string(),
                    None,
                    BTreeMap::new(),
                )));
            };
            let Some(option_id) = second_arg_token(arg) else {
                return Ok(Some(channel_response(
                    "❌ Usage: `/card_answer <question_id> <option_id>`".to_string(),
                    None,
                    BTreeMap::new(),
                )));
            };
            let manager = ask_question_manager(runtime);
            let question = match manager.get_question(question_id).await {
                Ok(question) => question,
                Err(_) => {
                    return Ok(Some(channel_response(
                        format!("❌ Question not found: `{question_id}`"),
                        None,
                        BTreeMap::new(),
                    )));
                }
            };
            if question.session_key != route.active_session_key
                && question.session_key != base_session_key
            {
                return Ok(Some(channel_response(
                    format!("❌ Question `{question_id}` does not belong to current session."),
                    None,
                    BTreeMap::new(),
                )));
            }
            let outcome = manager
                .answer_question(question_id, option_id, Some("channel-user"), now_ms())
                .await;
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(err) => {
                    return Ok(Some(channel_response(
                        format!("❌ Failed to record answer: {err}"),
                        None,
                        BTreeMap::new(),
                    )));
                }
            };
            if !outcome.updated {
                let response = match outcome.question.status {
                    klaw_storage::PendingQuestionStatus::Answered => {
                        let selected_label = outcome
                            .question
                            .selected_option()
                            .map(|option| option.label.as_str())
                            .unwrap_or("unknown");
                        channel_response(
                            format!(
                                "ℹ️ Question `{question_id}` was already answered with `{selected_label}`."
                            ),
                            None,
                            BTreeMap::new(),
                        )
                    }
                    klaw_storage::PendingQuestionStatus::Expired => channel_response(
                        format!("⌛ Question expired: `{question_id}`"),
                        None,
                        BTreeMap::new(),
                    ),
                    klaw_storage::PendingQuestionStatus::Pending => channel_response(
                        format!("ℹ️ Question `{question_id}` is still pending."),
                        None,
                        BTreeMap::new(),
                    ),
                };
                return Ok(Some(response));
            }
            let Some(selected_option) = outcome.question.selected_option() else {
                return Ok(Some(channel_response(
                    format!("❌ Question `{question_id}` was answered without a valid option."),
                    None,
                    BTreeMap::new(),
                )));
            };
            let followup_input = format!(
                "The user answered a pending ask_question prompt.\nQuestion ID: {}\nQuestion: {}\nSelected option ID: {}\nSelected option label: {}",
                outcome.question.id,
                outcome.question.question_text,
                selected_option.id,
                selected_option.label
            );
            let maybe_output = submit_and_get_output(
                runtime,
                channel.clone(),
                followup_input,
                outcome.question.session_key.clone(),
                question.chat_id.clone(),
                "channel-user".to_string(),
                route.model_provider.clone(),
                route.model.clone(),
                Vec::new(),
                build_ask_question_followup_request_metadata(
                    &request_metadata,
                    &outcome.question.id,
                    &outcome.question.question_text,
                    &selected_option.id,
                    &selected_option.label,
                ),
            )
            .await?;
            match maybe_output {
                Some(output) => channel_response(output.content, output.reasoning, output.metadata),
                None => channel_response(
                    format!("✅ Answer recorded: `{}`", selected_option.label),
                    None,
                    BTreeMap::new(),
                ),
            }
        }
        other => {
            let help = render_help_text(runtime);
            channel_response(
                format!("❌ Unknown command: `/{other}`\n\n{help}"),
                None,
                BTreeMap::new(),
            )
        }
    };
    Ok(Some(response))
}

fn render_help_text(runtime: &RuntimeBundle) -> String {
    let provider_runtime = provider_runtime_snapshot(runtime);
    let mut lines = vec![
        "📘 **Command Center**".to_string(),
        String::new(),
        "```text".to_string(),
    ];
    lines.push(format!("{:<24}{}", "/new", "Start a new session context"));
    lines.push(format!(
        "{:<24}{}",
        "/start", "Alias of /new for a fresh session"
    ));
    lines.push(format!("{:<24}{}", "/help", "Show this help"));
    lines.push(format!(
        "{:<24}{}",
        "/stop", "Stop the current turn without calling the agent"
    ));
    lines.push(format!(
        "{:<24}{}",
        "/usage", "Show latest turn and current session token usage"
    ));
    lines.push(format!(
        "{:<24}{}",
        "/shell <command>", "Run a shell command from the shell workspace"
    ));
    if provider_runtime.provider_default_models.len() > 1 {
        lines.push(format!(
            "{:<24}{}",
            "/model_provider", "List available providers"
        ));
        lines.push(format!(
            "{:<24}{}",
            "/model_provider <id>", "Switch provider for current session"
        ));
    }
    lines.push(format!("{:<24}{}", "/model", "Show current model"));
    lines.push(format!(
        "{:<24}{}",
        "/model <model_name>", "Update current model for current session"
    ));
    lines.push(format!(
        "{:<24}{}",
        "/approve <approval_id>", "Approve a pending tool action"
    ));
    lines.push(format!(
        "{:<24}{}",
        "/reject <approval_id>", "Reject a pending tool action"
    ));
    lines.push("```".to_string());
    lines.join("\n")
}
