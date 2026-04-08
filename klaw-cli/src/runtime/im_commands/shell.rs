use super::*;

pub(super) async fn execute_approved_shell(
    runtime: &RuntimeBundle,
    approval_id: &str,
    session_key: &str,
    command_text: &str,
) -> Result<String, Box<dyn Error>> {
    let Some(shell_tool) = runtime.runtime.tools.get("shell") else {
        return Ok(
            "⚠️ shell tool unavailable; approval has been recorded but command was not executed."
                .to_string(),
        );
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
        Ok(output) => Ok(output
            .content_for_user
            .unwrap_or_else(|| output.content_for_model)),
        Err(err) if err.code() == "approval_required" => Ok(err.message().to_string()),
        Err(err) => Ok(format!("tool `shell` failed: {err}")),
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
        Ok(output) => Ok(output
            .content_for_user
            .unwrap_or_else(|| output.content_for_model)),
        Err(err) => Ok(format!("tool `shell` failed: {err}")),
    }
}
