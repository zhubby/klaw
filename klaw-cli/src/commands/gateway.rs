use clap::Args;
use klaw_config::AppConfig;
use std::sync::Arc;

#[derive(Debug, Args)]
pub struct GatewayCommand {}

impl GatewayCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        klaw_gateway::run_gateway(&config.gateway).await?;
        Ok(())
    }
}
