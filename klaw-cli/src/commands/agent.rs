use clap::Args;
use klaw_channel::terminal::{TERMINAL_CHANNEL_NAME, resolve_session_key};
use klaw_runtime::{build_runtime_bundle, submit_and_get_output};
use std::collections::BTreeMap;
use std::sync::Arc;

use klaw_config::AppConfig;

fn resolve_channel_name() -> &'static str {
    TERMINAL_CHANNEL_NAME
}

#[derive(Debug, Args)]
pub struct AgentCommand {
    /// Input text for a single request.
    #[arg(long)]
    pub input: String,
    /// Session key used for this one-shot request. Auto-generated as `terminal:<uuid>` when omitted.
    #[arg(long)]
    pub session_key: Option<String>,
}

impl AgentCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = build_runtime_bundle(config.as_ref()).await?;
        let provider_runtime = runtime.runtime.provider_runtime_snapshot();
        let session_key = resolve_session_key(self.session_key);
        let chat_id = session_key.split(':').nth(1).unwrap_or("chat").to_string();

        let maybe_output = submit_and_get_output(
            &runtime,
            resolve_channel_name().to_string(),
            self.input,
            session_key,
            chat_id,
            "local-user".to_string(),
            provider_runtime.default_provider_id,
            provider_runtime.default_model,
            Vec::new(),
            BTreeMap::new(),
        )
        .await?;
        match maybe_output {
            Some(output) => println!("{}", output.content),
            None => println!("[no response]"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_channel_name, resolve_session_key};

    #[test]
    fn generated_session_key_uses_terminal_prefix() {
        let session_key = resolve_session_key(None);
        assert!(session_key.starts_with("terminal:"));
    }

    #[test]
    fn explicit_session_key_is_preserved() {
        let session_key = resolve_session_key(Some("terminal:manual".to_string()));
        assert_eq!(session_key, "terminal:manual");
    }

    #[test]
    fn agent_channel_name_is_terminal() {
        assert_eq!(resolve_channel_name(), "terminal");
    }
}
