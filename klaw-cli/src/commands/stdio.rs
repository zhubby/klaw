use clap::Args;
use std::{
    io::{self, Write},
    sync::Arc,
};
use tokio::io::AsyncBufReadExt;
use tracing::info;
use uuid::Uuid;

use crate::commands::runtime::{build_runtime_bundle, submit_and_get_output};
use crate::commands::service_loop::{BackgroundServiceConfig, BackgroundServices};
use klaw_config::AppConfig;

#[derive(Debug, Args)]
pub struct StdioCommand {
    /// Session key used for local conversation. Auto-generated as `stdio:<uuid>` when omitted.
    #[arg(long)]
    pub session_key: Option<String>,
    /// Print model reasoning when provider returns it.
    #[arg(long, default_value_t = false)]
    pub show_reasoning: bool,
}

impl StdioCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = build_runtime_bundle(config.as_ref()).await?;
        let session_key = self
            .session_key
            .unwrap_or_else(|| format!("stdio:{}", Uuid::new_v4()));
        let chat_id = session_key
            .split(':')
            .nth(1)
            .unwrap_or("chat")
            .to_string();
        let background = BackgroundServices::new(
            &runtime,
            BackgroundServiceConfig::from_app_config(config.as_ref()),
        );

        println!("Agent stdio mode started.");
        println!("Type your message and press Enter.");
        println!("Use /exit to quit.\n");
        info!(session_key, "cli started");

        let stdin = tokio::io::BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();
        let mut cron_tick = tokio::time::interval(background.cron_tick_interval());
        let mut runtime_tick = tokio::time::interval(background.runtime_tick_interval());
        print!("you> ");
        io::stdout().flush()?;

        loop {
            tokio::select! {
                _ = cron_tick.tick() => {
                    background.on_cron_tick().await;
                }
                _ = runtime_tick.tick() => {
                    background.on_runtime_tick(&runtime).await;
                }
                line = lines.next_line() => {
                    let maybe_line = line?;
                    let Some(line) = maybe_line else {
                        println!("\nEOF received. Bye.");
                        break;
                    };

                    let input = line.trim();
                    if input.is_empty() {
                        print!("you> ");
                        io::stdout().flush()?;
                        continue;
                    }
                    if input == "/exit" {
                        println!("Bye.");
                        break;
                    }

                    let maybe_output = submit_and_get_output(
                        &runtime,
                        input.to_string(),
                        session_key.clone(),
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
                    print!("you> ");
                    io::stdout().flush()?;
                }
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
    use super::StdioCommand;

    #[test]
    fn keeps_explicit_session_key() {
        let cmd = StdioCommand {
            session_key: Some("stdio:explicit".to_string()),
            show_reasoning: false,
        };
        let key = cmd
            .session_key
            .unwrap_or_else(|| format!("stdio:{}", uuid::Uuid::new_v4()));
        assert_eq!(key, "stdio:explicit");
    }

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
