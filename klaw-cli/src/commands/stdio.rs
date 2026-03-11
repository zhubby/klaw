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
    /// Print model reasoning when provider returns it.
    #[arg(long, default_value_t = false)]
    pub show_reasoning: bool,
}

impl StdioCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = build_runtime_bundle(config.as_ref()).await?;
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
                Some(output) => {
                    println!(
                        "agent>\n{}\n",
                        render_agent_output(
                            &output.content,
                            output.reasoning.as_deref(),
                            self.show_reasoning
                        )
                    )
                }
                None => println!("agent> [no response]\n"),
            }
        }
        Ok(())
    }
}

fn render_agent_output(content: &str, reasoning: Option<&str>, show_reasoning: bool) -> String {
    let mut lines = vec![
        "--------------------".to_string(),
        "[answer]".to_string(),
        content.trim().to_string(),
    ];
    if show_reasoning {
        if let Some(reasoning_text) = reasoning {
            lines.push(String::new());
            lines.push("[reasoning]".to_string());
            lines.extend(reasoning_text.lines().map(|line| format!("> {line}")));
        }
    }
    lines.push("--------------------".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::render_agent_output;

    #[test]
    fn hides_reasoning_when_flag_disabled() {
        let view = render_agent_output("done", Some("step1\nstep2"), false);
        assert!(view.contains("[answer]"));
        assert!(!view.contains("[reasoning]"));
    }

    #[test]
    fn renders_reasoning_block_when_enabled() {
        let view = render_agent_output("done", Some("step1\nstep2"), true);
        assert!(view.contains("[reasoning]"));
        assert!(view.contains("> step1"));
        assert!(view.contains("> step2"));
    }
}
