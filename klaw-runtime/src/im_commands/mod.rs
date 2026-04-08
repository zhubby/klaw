mod parser;
mod shell;
mod usage;

use super::*;

use parser::now_ms;
use shell::{execute_approved_shell, execute_im_shell};
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
            if approval.session_key != route.active_session_key
                && approval.session_key != base_session_key
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
                    if approved.tool_name != "shell" {
                        return Ok(Some(channel_response(
                            format!(
                                "✅ Approval granted: `{}` (`{}`).\n\n请重试之前触发审批的操作。",
                                approved.id, approved.tool_name
                            ),
                            None,
                            BTreeMap::new(),
                        )));
                    }
                    let execution_result = execute_approved_shell(
                        runtime,
                        &approved.id,
                        &approved.session_key,
                        &approved.command_text,
                    )
                    .await?;
                    let model_followup_input = format!(
                        "审批已通过并已执行命令。请基于以下执行结果继续处理本轮任务。\n\
                        要求：\n\
                        1) 先说明成功/失败\n\
                        2) 如果失败且属于明显可修复的命令/参数问题，你可以再调用一次工具进行修复重试\n\
                        3) 最多只允许一次额外工具调用；若不需要重试，直接给出最终回复\n\
                        4) 若失败原因不是一次重试可解决的问题，直接总结最关键原因和下一步建议\n\n\
                        approval_id: {}\n\
                        command: {}\n\
                        shell_result:\n{}",
                        approved.id, approved.command_preview, execution_result
                    );
                    let maybe_output = submit_and_get_output(
                        runtime,
                        channel.clone(),
                        model_followup_input,
                        approved.session_key.clone(),
                        chat_id.clone(),
                        "local-user".to_string(),
                        route.model_provider.clone(),
                        route.model.clone(),
                        Vec::new(),
                        build_approved_shell_followup_request_metadata(&request_metadata),
                    )
                    .await?;
                    match maybe_output {
                        Some(output) => {
                            channel_response(output.content, output.reasoning, output.metadata)
                        }
                        None => channel_response(
                            format!(
                                "✅ **Approval granted and command executed**\n\n{}",
                                execution_result
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
            if approval.session_key != route.active_session_key
                && approval.session_key != base_session_key
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
