use clap::Args;
use std::sync::Arc;

use crate::commands::runtime::{build_runtime_bundle, submit_and_get_output};
use klaw_config::AppConfig;

#[derive(Debug, Args)]
pub struct AgentCommand {
    /// Input text for a single request.
    #[arg(long)]
    pub input: String,
    /// Session key used for this one-shot request.
    #[arg(long, default_value = "stdio:agent")]
    pub session_key: String,
}

impl AgentCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = build_runtime_bundle(config.as_ref()).await?;
        let chat_id = self
            .session_key
            .split(':')
            .nth(1)
            .unwrap_or("agent")
            .to_string();

        let maybe_output =
            submit_and_get_output(&runtime, self.input, self.session_key, chat_id).await?;
        match maybe_output {
            Some(output) => println!("{}", output.content),
            None => println!("[no response]"),
        }
        Ok(())
    }
}
