use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

#[derive(Debug, Clone, Default)]
pub struct AcpAgentHub {
    entries: Arc<RwLock<BTreeMap<String, String>>>,
}

impl AcpAgentHub {
    pub fn insert(&self, agent_id: impl Into<String>, command: impl Into<String>) {
        self.entries
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .insert(agent_id.into(), command.into());
    }

    #[must_use]
    pub fn get(&self, agent_id: &str) -> Option<String> {
        self.entries
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(agent_id)
            .cloned()
    }

    pub fn remove(&self, agent_id: &str) {
        self.entries
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .remove(agent_id);
    }
}
