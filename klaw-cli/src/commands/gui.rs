use clap::Args;
use std::fmt;

#[derive(Debug, Args)]
pub struct GuiCommand {}

impl GuiCommand {
    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        klaw_gui::run().map_err(|err| Box::new(GuiCliError(err.to_string())) as _)
    }
}

struct GuiCliError(String);

impl fmt::Display for GuiCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for GuiCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for GuiCliError {}
