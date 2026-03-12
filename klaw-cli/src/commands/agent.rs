use clap::Args;
use std::sync::Arc;
use uuid::Uuid;

use crate::runtime::{build_runtime_bundle, submit_and_get_output};
use klaw_config::AppConfig;

#[derive(Debug, Args)]
pub struct AgentCommand {
    /// Input text for a single request.
    #[arg(long)]
    pub input: String,
    /// Session key used for this one-shot request. Auto-generated as `stdio:<uuid>` when omitted.
    #[arg(long)]
    pub session_key: Option<String>,
}

impl AgentCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = build_runtime_bundle(config.as_ref()).await?;
        let session_key = self
            .session_key
            .unwrap_or_else(|| format!("stdio:{}", Uuid::new_v4()));
        let chat_id = session_key.split(':').nth(1).unwrap_or("chat").to_string();

        let maybe_output =
            submit_and_get_output(&runtime, self.input, session_key, chat_id).await?;
        match maybe_output {
            Some(output) => println!("{}", output.content),
            None => println!("[no response]"),
        }
        Ok(())
    }
}
