use super::*;

#[derive(Debug, Clone)]
pub(super) struct ApprovedShellExecution {
    pub raw_output: String,
    pub parsed: Option<ApprovedShellExecutionPayload>,
}

#[derive(Debug, Clone)]
pub(super) struct ApprovedShellExecutionPayload {
    pub success: bool,
    pub timed_out: bool,
}

impl ApprovedShellExecution {
    pub(super) fn needs_recovery_followup(&self) -> bool {
        self.parsed
            .as_ref()
            .is_some_and(|payload| payload.timed_out || !payload.success)
    }
}

pub(super) async fn execute_approved_shell(
    runtime: &RuntimeBundle,
    approval_id: &str,
    session_key: &str,
    command_text: &str,
) -> Result<ApprovedShellExecution, Box<dyn Error>> {
    let Some(shell_tool) = runtime.runtime.tools.get("shell") else {
        let raw_output =
            "⚠️ shell tool unavailable; approval has been recorded but command was not executed."
                .to_string();
        return Ok(ApprovedShellExecution {
            parsed: parse_shell_execution_payload(&raw_output),
            raw_output,
        });
    };
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "shell.approval_id".to_string(),
        serde_json::Value::String(approval_id.to_string()),
    );
    let output = shell_tool
        .execute(
            json!({ "command": command_text }),
            &ToolContext {
                session_key: session_key.to_string(),
                metadata,
            },
        )
        .await;
    match output {
        Ok(output) => {
            let raw_output = output
                .content_for_user
                .unwrap_or_else(|| output.content_for_model);
            Ok(ApprovedShellExecution {
                parsed: parse_shell_execution_payload(&raw_output),
                raw_output,
            })
        }
        Err(err) if err.code() == "approval_required" => {
            let raw_output = err.message().to_string();
            Ok(ApprovedShellExecution {
                parsed: parse_shell_execution_payload(&raw_output),
                raw_output,
            })
        }
        Err(err) => {
            let raw_output = format!("tool `shell` failed: {err}");
            Ok(ApprovedShellExecution {
                parsed: parse_shell_execution_payload(&raw_output),
                raw_output,
            })
        }
    }
}

pub(super) async fn execute_im_shell(
    runtime: &RuntimeBundle,
    session_key: &str,
    command_text: &str,
) -> Result<String, Box<dyn Error>> {
    let Some(shell_tool) = runtime.runtime.tools.get("shell") else {
        return Ok("⚠️ shell tool unavailable.".to_string());
    };

    let mut metadata = BTreeMap::new();
    metadata.insert("shell.approved".to_string(), Value::Bool(true));
    metadata.insert(
        "shell.source".to_string(),
        Value::String("im_command".to_string()),
    );

    let output = shell_tool
        .execute(
            json!({ "command": command_text }),
            &ToolContext {
                session_key: session_key.to_string(),
                metadata,
            },
        )
        .await;

    match output {
        Ok(output) => Ok(format_shell_output_for_im(
            &output
                .content_for_user
                .unwrap_or_else(|| output.content_for_model),
        )),
        Err(err) => Ok(format!("tool `shell` failed: {err}")),
    }
}

fn format_shell_output_for_im(raw: &str) -> String {
    let Ok(payload) = serde_json::from_str::<Value>(raw) else {
        return raw.to_string();
    };

    let success = payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let timed_out = payload
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let title = if timed_out {
        "⏱️ **Shell command timed out**"
    } else if success {
        "✅ **Shell command succeeded**"
    } else {
        "❌ **Shell command failed**"
    };

    let mut lines = vec![title.to_string(), String::new()];

    if let Some(command) = payload.get("command").and_then(Value::as_str) {
        lines.push(format!("- Command: `{command}`"));
    }
    if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
        lines.push(format!("- CWD: `{cwd}`"));
    }
    if let Some(exit_code) = payload.get("exit_code") {
        if !exit_code.is_null() {
            lines.push(format!("- Exit code: `{exit_code}`"));
        }
    }
    if let Some(duration_ms) = payload.get("duration_ms").and_then(Value::as_u64) {
        lines.push(format!("- Duration: `{duration_ms}ms`"));
    }

    append_stream_block(
        &mut lines,
        "stdout",
        payload.get("stdout").and_then(Value::as_str),
        payload
            .get("stdout_truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    );
    append_stream_block(
        &mut lines,
        "stderr",
        payload.get("stderr").and_then(Value::as_str),
        payload
            .get("stderr_truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    );

    lines.join("\n")
}

fn append_stream_block(
    lines: &mut Vec<String>,
    label: &str,
    content: Option<&str>,
    truncated: bool,
) {
    let Some(content) = content.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    lines.push(String::new());
    lines.push(format!("**{label}**"));
    lines.push("````text".to_string());
    lines.push(content.to_string());
    lines.push("````".to_string());

    if truncated {
        lines.push(format!("_{} was truncated_", label));
    }
}

fn parse_shell_execution_payload(raw: &str) -> Option<ApprovedShellExecutionPayload> {
    let payload = serde_json::from_str::<Value>(raw).ok()?;
    Some(ApprovedShellExecutionPayload {
        success: payload
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        timed_out: payload
            .get("timed_out")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}
