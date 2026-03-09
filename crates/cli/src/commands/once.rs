use clap::Args;

use crate::commands::runtime::{build_runtime_bundle, submit_and_get_output};

#[derive(Debug, Args)]
pub struct OnceCommand {
    /// Input text for a single request.
    #[arg(long)]
    pub input: String,
    /// Session key used for this one-shot request.
    #[arg(long, default_value = "stdio:once")]
    pub session_key: String,
}

impl OnceCommand {
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = build_runtime_bundle();
        let chat_id = self
            .session_key
            .split(':')
            .nth(1)
            .unwrap_or("once")
            .to_string();

        let maybe_output =
            submit_and_get_output(&runtime, self.input, self.session_key, chat_id).await?;
        match maybe_output {
            Some(content) => println!("{content}"),
            None => println!("[no response]"),
        }
        Ok(())
    }
}
