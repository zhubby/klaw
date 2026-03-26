use serde::{Deserialize, Serialize};

pub const UTC_TIMEZONE_NAME: &str = "UTC";

pub fn system_timezone_name() -> String {
    iana_time_zone::get_timezone()
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| UTC_TIMEZONE_NAME.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentCheckReport {
    pub checks: Vec<DependencyStatus>,
    pub checked_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyStatus {
    pub name: String,
    pub description: String,
    pub project_url: Option<String>,
    pub available: bool,
    pub version: Option<String>,
    pub required: bool,
    pub category: DependencyCategory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DependencyCategory {
    Required,
    Preferred,
    OptionalWithFallback,
}

impl EnvironmentCheckReport {
    pub fn all_required_available(&self) -> bool {
        self.checks
            .iter()
            .filter(|c| c.required)
            .all(|c| c.available)
    }

    pub fn terminal_multiplexer_available(&self) -> bool {
        self.checks
            .iter()
            .filter(|c| c.name == "zellij" || c.name == "tmux")
            .any(|c| c.available)
    }

    pub fn all_preferred_available(&self) -> bool {
        self.checks
            .iter()
            .filter(|c| matches!(c.category, DependencyCategory::Preferred))
            .all(|c| c.available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_timezone_name_is_non_empty() {
        assert!(!system_timezone_name().trim().is_empty());
    }
}
