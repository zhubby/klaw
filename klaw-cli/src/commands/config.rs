use clap::{Args, Subcommand};
use klaw_config::ConfigError;
use std::{fmt, path::Path};

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
    /// Validate that config file can be parsed and passes schema checks.
    Validate(ConfigValidateCommand),
    /// Print a pretty TOML config template.
    Example(ConfigExampleCommand),
}

#[derive(Debug, Args)]
pub struct ConfigMigrateCommand {}

#[derive(Debug, Args)]
pub struct ConfigResetCommand {}

#[derive(Debug, Args)]
pub struct ConfigValidateCommand {}

#[derive(Debug, Args)]
pub struct ConfigExampleCommand {}

impl ConfigCommand {
    pub fn run(self, config_path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
        let result = match self.command {
            ConfigSubcommands::Migrate(cmd) => cmd.run(config_path),
            ConfigSubcommands::Reset(cmd) => cmd.run(config_path),
            ConfigSubcommands::Validate(cmd) => cmd.run(config_path),
            ConfigSubcommands::Example(cmd) => {
                cmd.run();
                Ok(())
            }
        };

        result.map_err(|err| Box::new(ConfigCliError(format_config_error(&err))) as _)
    }
}

impl ConfigMigrateCommand {
    fn run(self, config_path: Option<&Path>) -> Result<(), ConfigError> {
        let migrated = klaw_config::migrate_with_defaults(config_path)?;
        print_result(
            "Config Migrated",
            migrated.path.as_path(),
            Some(if migrated.created_file {
                "created"
            } else {
                "updated"
            }),
        );
        Ok(())
    }
}

impl ConfigResetCommand {
    fn run(self, config_path: Option<&Path>) -> Result<(), ConfigError> {
        let migrated = klaw_config::reset_to_defaults(config_path)?;
        print_result(
            "Config Reset",
            migrated.path.as_path(),
            Some(if migrated.created_file {
                "created"
            } else {
                "overwritten"
            }),
        );
        Ok(())
    }
}

impl ConfigExampleCommand {
    fn run(self) {
        let template = klaw_config::default_config_template();
        println!("{template}");
    }
}

impl ConfigValidateCommand {
    fn run(self, config_path: Option<&Path>) -> Result<(), ConfigError> {
        let path = klaw_config::validate_config_file(config_path)?;
        print_result("Config Valid", path.as_path(), None);
        Ok(())
    }
}

fn print_result(title: &str, path: &Path, status: Option<&str>) {
    println!("{title}");
    println!("  path: {}", path.display());
    if let Some(status) = status {
        println!("  status: {status}");
    }
}

fn format_config_error(err: &ConfigError) -> String {
    match err {
        ConfigError::ConfigNotFound(path) => format!(
            "Config Invalid\n  path: {}\n  reason: config file not found",
            path.display()
        ),
        ConfigError::ParseConfig { path, source } => {
            let mut rendered = format!(
                "Config Invalid\n  path: {}\n  reason: {}",
                path.display(),
                source.message()
            );
            if let Some(span) = source.span() {
                rendered.push_str(&format!("\n  span: {}..{}", span.start, span.end));
            }
            rendered
        }
        ConfigError::InvalidConfig(message) => {
            format!("Config Invalid\n  reason: {message}")
        }
        ConfigError::ReadConfig { path, source } => format!(
            "Config Invalid\n  path: {}\n  reason: failed to read config ({source})",
            path.display()
        ),
        ConfigError::CreateDir(source) => {
            format!("Config Error\n  reason: failed to create config directory ({source})")
        }
        ConfigError::WriteConfig(source) => {
            format!("Config Error\n  reason: failed to write config ({source})")
        }
        ConfigError::HomeDirUnavailable => {
            "Config Error\n  reason: cannot resolve home directory for default config path"
                .to_string()
        }
    }
}

struct ConfigCliError(String);

impl fmt::Display for ConfigCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for ConfigCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConfigCliError {}
