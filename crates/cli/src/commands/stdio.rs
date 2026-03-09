use clap::Args;
use std::{
    io::{self, Write},
    sync::Arc,
};
use tracing::info;

use crate::commands::runtime::{build_runtime_bundle, submit_and_get_output};
use klaw_config::AppConfig;

#[derive(Debug, Args)]
pub struct StdioCommand {
    /// Session key used for local conversation.
    #[arg(long, default_value = "stdio:local-chat")]
    pub session_key: String,
}

impl StdioCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = build_runtime_bundle(config.as_ref())?;
        let chat_id = self
            .session_key
            .split(':')
            .nth(1)
            .unwrap_or("local-chat")
            .to_string();

        println!("Agent stdio mode started.");
        println!("Type your message and press Enter.");
        println!("Use /exit to quit.\n");
        info!(session_key = self.session_key, "cli started");

        let stdin = io::stdin();
        let mut line = String::new();
        loop {
            print!("you> ");
            io::stdout().flush()?;

            line.clear();
            let bytes = stdin.read_line(&mut line)?;
            if bytes == 0 {
                println!("\nEOF received. Bye.");
                break;
            }

            let input = line.trim();
            if input.is_empty() {
                continue;
            }
            if input == "/exit" {
                println!("Bye.");
                break;
            }

            let maybe_output = submit_and_get_output(
                &runtime,
                input.to_string(),
                self.session_key.clone(),
                chat_id.clone(),
            )
            .await?;

            match maybe_output {
                Some(content) => println!("agent> {}\n", content),
                None => println!("agent> [no response]\n"),
            }
        }
        Ok(())
    }
}
