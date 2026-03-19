use std::collections::BTreeMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum HealthStatus {
    Ready,
    Live,
    Degraded,
    Unavailable,
}

impl HealthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Live => "Live",
            Self::Degraded => "Degraded",
            Self::Unavailable => "Unavailable",
        }
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Ready | Self::Live)
    }
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ComponentHealth {
    pub name: String,
    pub status: HealthStatus,
    pub message: Option<String>,
}

pub struct HealthRegistry {
    components: RwLock<BTreeMap<String, ComponentHealth>>,
}

impl Default for HealthRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthRegistry {
    pub fn new() -> Self {
        Self {
            components: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn register(&self, name: impl Into<String>) {
        let name = name.into();
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        components.insert(
            name.clone(),
            ComponentHealth {
                name,
                status: HealthStatus::Unavailable,
                message: None,
            },
        );
    }

    pub fn set_status(&self, name: &str, status: HealthStatus) {
        self.set_status_with_message(name, status, None);
    }

    pub fn set_status_with_message(
        &self,
        name: &str,
        status: HealthStatus,
        message: Option<String>,
    ) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(component) = components.get_mut(name) {
            component.status = status;
            component.message = message;
        }
    }

    pub fn get_component(&self, name: &str) -> Option<ComponentHealth> {
        let components = self
            .components
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        components.get(name).cloned()
    }

    pub fn liveness(&self) -> HealthStatus {
        let components = self
            .components
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if components.is_empty() {
            return HealthStatus::Live;
        }
        if components
            .values()
            .any(|c| c.status == HealthStatus::Unavailable)
        {
            HealthStatus::Unavailable
        } else {
            HealthStatus::Live
        }
    }

    pub fn readiness(&self) -> HealthStatus {
        let components = self
            .components
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if components.is_empty() {
            return HealthStatus::Ready;
        }
        let mut has_degraded = false;
        for component in components.values() {
            match component.status {
                HealthStatus::Ready => continue,
                HealthStatus::Live => continue,
                HealthStatus::Degraded => has_degraded = true,
                HealthStatus::Unavailable => return HealthStatus::Unavailable,
            }
        }
        if has_degraded {
            HealthStatus::Degraded
        } else {
            HealthStatus::Ready
        }
    }

    pub fn overall_status(&self) -> HealthStatus {
        let liveness = self.liveness();
        let readiness = self.readiness();
        match (liveness, readiness) {
            (HealthStatus::Unavailable, _) => HealthStatus::Unavailable,
            (_, HealthStatus::Unavailable) => HealthStatus::Unavailable,
            (_, HealthStatus::Degraded) => HealthStatus::Degraded,
            (HealthStatus::Live, HealthStatus::Ready) => HealthStatus::Ready,
            (HealthStatus::Live, HealthStatus::Live) => HealthStatus::Live,
            _ => HealthStatus::Degraded,
        }
    }

    pub fn all_components(&self) -> Vec<ComponentHealth> {
        let components = self
            .components
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        components.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_status_str_matches() {
        assert_eq!(HealthStatus::Ready.as_str(), "Ready");
        assert_eq!(HealthStatus::Live.as_str(), "Live");
        assert_eq!(HealthStatus::Degraded.as_str(), "Degraded");
        assert_eq!(HealthStatus::Unavailable.as_str(), "Unavailable");
    }

    #[test]
    fn health_registry_liveness_returns_live_when_empty() {
        let registry = HealthRegistry::new();
        assert_eq!(registry.liveness(), HealthStatus::Live);
    }

    #[test]
    fn health_registry_readiness_returns_ready_when_empty() {
        let registry = HealthRegistry::new();
        assert_eq!(registry.readiness(), HealthStatus::Ready);
    }

    #[test]
    fn health_registry_tracks_component_status() {
        let registry = HealthRegistry::new();
        registry.register("provider");
        registry.set_status("provider", HealthStatus::Ready);

        let component = registry
            .get_component("provider")
            .expect("component should exist");
        assert_eq!(component.status, HealthStatus::Ready);
    }

    #[test]
    fn health_registry_overall_status_unavailable_when_any_unavailable() {
        let registry = HealthRegistry::new();
        registry.register("provider");
        registry.register("transport");
        registry.set_status("provider", HealthStatus::Ready);
        registry.set_status("transport", HealthStatus::Unavailable);

        assert_eq!(registry.readiness(), HealthStatus::Unavailable);
        assert_eq!(registry.overall_status(), HealthStatus::Unavailable);
    }

    #[test]
    fn health_registry_overall_status_degraded_when_any_degraded() {
        let registry = HealthRegistry::new();
        registry.register("provider");
        registry.register("transport");
        registry.set_status("provider", HealthStatus::Ready);
        registry.set_status("transport", HealthStatus::Degraded);

        assert_eq!(registry.readiness(), HealthStatus::Degraded);
        assert_eq!(registry.overall_status(), HealthStatus::Degraded);
    }
}
