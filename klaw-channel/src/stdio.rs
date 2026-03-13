use crate::{Channel, ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime};
use std::io::{self, Write};
use tokio::io::AsyncBufReadExt;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StdioChannel {
    session_key: String,
    chat_id: String,
    show_reasoning: bool,
}

impl StdioChannel {
    pub fn new(session_key: Option<String>, show_reasoning: bool) -> Self {
        let session_key = session_key.unwrap_or_else(|| format!("stdio:{}", Uuid::new_v4()));
        let chat_id = session_key.split(':').nth(1).unwrap_or("chat").to_string();
        Self {
            session_key,
            chat_id,
            show_reasoning,
        }
    }

    pub fn session_key(&self) -> &str {
        &self.session_key
    }
}

#[async_trait::async_trait(?Send)]
impl Channel for StdioChannel {
    fn name(&self) -> &'static str {
        "stdio"
    }

    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
        println!("Agent stdio mode started.");
        println!("Type your message and press Enter.");
        println!("Use /exit to quit.\n");
        info!(session_key = %self.session_key, "stdio channel started");

        let stdin = tokio::io::BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();
        let mut cron_tick = tokio::time::interval(runtime.cron_tick_interval());
        let mut runtime_tick = tokio::time::interval(runtime.runtime_tick_interval());

        print!("you> ");
        io::stdout().flush()?;

        loop {
            tokio::select! {
                _ = cron_tick.tick() => {
                    runtime.on_cron_tick().await;
                }
                _ = runtime_tick.tick() => {
                    runtime.on_runtime_tick().await;
                }
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    println!("\nCtrl+C received. Bye.");
                    break;
                }
                line = lines.next_line() => {
                    let maybe_line: Option<String> = line?;
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

                    let maybe_output = runtime
                        .submit(ChannelRequest {
                            channel: self.name().to_string(),
                            input: input.to_string(),
                            session_key: self.session_key.clone(),
                            chat_id: self.chat_id.clone(),
                        })
                        .await?;

                    match maybe_output {
                        Some(output) => {
                            println!(
                                "agent>\n{}\n",
                                render_agent_output(
                                    &output,
                                    self.show_reasoning,
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

fn render_agent_output(output: &ChannelResponse, show_reasoning: bool) -> String {
    let mut lines = vec![
        "--------------------".to_string(),
        "[answer]".to_string(),
        output.content.trim().to_string(),
    ];
    if show_reasoning {
        if let Some(reasoning_text) = &output.reasoning {
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
    use super::{render_agent_output, StdioChannel};
    use crate::ChannelResponse;

    #[test]
    fn keeps_explicit_session_key() {
        let channel = StdioChannel::new(Some("stdio:explicit".to_string()), false);
        assert_eq!(channel.session_key(), "stdio:explicit");
    }

    #[test]
    fn hides_reasoning_when_flag_disabled() {
        let view = render_agent_output(
            &ChannelResponse {
                content: "done".to_string(),
                reasoning: Some("step1\nstep2".to_string()),
            },
            false,
        );
        assert!(view.contains("[answer]"));
        assert!(!view.contains("[reasoning]"));
    }

    #[test]
    fn renders_reasoning_block_when_enabled() {
        let view = render_agent_output(
            &ChannelResponse {
                content: "done".to_string(),
                reasoning: Some("step1\nstep2".to_string()),
            },
            true,
        );
        assert!(view.contains("[reasoning]"));
        assert!(view.contains("> step1"));
        assert!(view.contains("> step2"));
    }
}
