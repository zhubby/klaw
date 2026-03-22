use clap::{Args, Subcommand};
use klaw_session::{SessionListQuery, SessionManager, SqliteSessionManager};

#[derive(Debug, Args)]
pub struct SessionCommand {
    #[command(subcommand)]
    pub command: SessionSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum SessionSubcommands {
    /// List indexed sessions from klaw.db.
    List(SessionListCommand),
    /// Get one indexed session by session key.
    Get(SessionGetCommand),
}

#[derive(Debug, Args)]
pub struct SessionListCommand {
    /// Max rows to return.
    #[arg(long, default_value_t = 20)]
    pub limit: i64,
    /// Row offset for pagination.
    #[arg(long, default_value_t = 0)]
    pub offset: i64,
}

#[derive(Debug, Args)]
pub struct SessionGetCommand {
    /// Exact session key, e.g. stdio:local-chat.
    #[arg(long)]
    pub session_key: String,
}

impl SessionCommand {
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let manager = SqliteSessionManager::open_default().await?;
        match self.command {
            SessionSubcommands::List(cmd) => cmd.run(&manager).await?,
            SessionSubcommands::Get(cmd) => cmd.run(&manager).await?,
        }
        Ok(())
    }
}

impl SessionListCommand {
    async fn run(self, manager: &impl SessionManager) -> Result<(), Box<dyn std::error::Error>> {
        let sessions = manager
            .list_sessions(SessionListQuery {
                limit: self.limit,
                offset: self.offset,
                updated_from_ms: None,
                updated_to_ms: None,
            })
            .await?;
        if sessions.is_empty() {
            println!("No sessions.");
            return Ok(());
        }
        for s in sessions {
            let usage = manager.sum_llm_usage_by_session(&s.session_key).await?;
            println!(
                "{} chat_id={} channel={} turn_count={} total_tokens={} input_tokens={} output_tokens={} updated_at_ms={} jsonl_path={}",
                s.session_key,
                s.chat_id,
                s.channel,
                s.turn_count,
                usage.total_tokens,
                usage.input_tokens,
                usage.output_tokens,
                s.updated_at_ms,
                s.jsonl_path
            );
        }
        Ok(())
    }
}

impl SessionGetCommand {
    async fn run(self, manager: &impl SessionManager) -> Result<(), Box<dyn std::error::Error>> {
        let s = manager.get_session(&self.session_key).await?;
        let usage = manager.sum_llm_usage_by_session(&self.session_key).await?;
        println!("session_key={}", s.session_key);
        println!("chat_id={}", s.chat_id);
        println!("channel={}", s.channel);
        println!("turn_count={}", s.turn_count);
        println!("request_count={}", usage.request_count);
        println!("input_tokens={}", usage.input_tokens);
        println!("output_tokens={}", usage.output_tokens);
        println!("total_tokens={}", usage.total_tokens);
        println!("created_at_ms={}", s.created_at_ms);
        println!("updated_at_ms={}", s.updated_at_ms);
        println!("last_message_at_ms={}", s.last_message_at_ms);
        println!("jsonl_path={}", s.jsonl_path);
        Ok(())
    }
}
