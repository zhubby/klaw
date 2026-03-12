use clap::{Args, Subcommand};
use std::path::Path;

#[derive(Debug, Args)]
pub struct ConfigCommand {
    #[command(subcommand)]
    pub command: ConfigSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommands {
    /// Merge defaults with current config and rewrite the config file.
    Migrate(ConfigMigrateCommand),
    /// Overwrite config file with default template.
    Reset(ConfigResetCommand),
    /// Print a pretty TOML config template.
    Example(ConfigExampleCommand),
}

#[derive(Debug, Args)]
pub struct ConfigMigrateCommand {}

#[derive(Debug, Args)]
pub struct ConfigResetCommand {}

#[derive(Debug, Args)]
pub struct ConfigExampleCommand {}

impl ConfigCommand {
    pub fn run(self, config_path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
        match self.command {
            ConfigSubcommands::Migrate(cmd) => cmd.run(config_path)?,
            ConfigSubcommands::Reset(cmd) => cmd.run(config_path)?,
            ConfigSubcommands::Example(cmd) => cmd.run()?,
        }
        Ok(())
    }
}

impl ConfigMigrateCommand {
    fn run(self, config_path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
        let migrated = klaw_config::migrate_with_defaults(config_path)?;
        println!(
            "config migrated: {}{}",
            migrated.path.display(),
            if migrated.created_file {
                " (created)"
            } else {
                ""
            }
        );
        Ok(())
    }
}

impl ConfigResetCommand {
    fn run(self, config_path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
        let migrated = klaw_config::reset_to_defaults(config_path)?;
        println!(
            "config reset: {}{}",
            migrated.path.display(),
            if migrated.created_file {
                " (created)"
            } else {
                ""
            }
        );
        Ok(())
    }
}

impl ConfigExampleCommand {
    fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let template = klaw_config::default_config_template();
        println!("{template}");
        Ok(())
    }
}
