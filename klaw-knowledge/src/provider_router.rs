use std::{collections::BTreeMap, sync::Arc};

use crate::{
    KnowledgeEntry, KnowledgeError, KnowledgeHit, KnowledgeProvider, KnowledgeSearchQuery,
    KnowledgeSourceInfo,
};

#[derive(Default, Clone)]
pub struct KnowledgeProviderRouter {
    providers: BTreeMap<String, Arc<dyn KnowledgeProvider>>,
}

impl KnowledgeProviderRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<P>(&mut self, provider: P)
    where
        P: KnowledgeProvider + 'static,
    {
        self.providers
            .insert(provider.provider_name().to_string(), Arc::new(provider));
    }

    pub fn get(&self, provider_name: &str) -> Option<Arc<dyn KnowledgeProvider>> {
        self.providers.get(provider_name).cloned()
    }

    pub async fn search(
        &self,
        provider_name: &str,
        query: KnowledgeSearchQuery,
    ) -> Result<Vec<KnowledgeHit>, KnowledgeError> {
        let provider = self
            .get(provider_name)
            .ok_or_else(|| KnowledgeError::SourceUnavailable(provider_name.to_string()))?;
        provider.search(query).await
    }

    pub async fn get_entry(
        &self,
        provider_name: &str,
        id: &str,
    ) -> Result<Option<KnowledgeEntry>, KnowledgeError> {
        let provider = self
            .get(provider_name)
            .ok_or_else(|| KnowledgeError::SourceUnavailable(provider_name.to_string()))?;
        provider.get(id).await
    }

    pub async fn list_sources(
        &self,
        provider_name: &str,
    ) -> Result<Vec<KnowledgeSourceInfo>, KnowledgeError> {
        let provider = self
            .get(provider_name)
            .ok_or_else(|| KnowledgeError::SourceUnavailable(provider_name.to_string()))?;
        provider.list_sources().await
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::json;

    use super::*;

    #[derive(Clone)]
    struct MockProvider;

    #[async_trait]
    impl KnowledgeProvider for MockProvider {
        fn provider_name(&self) -> &str {
            "mock"
        }

        async fn search(
            &self,
            query: KnowledgeSearchQuery,
        ) -> Result<Vec<KnowledgeHit>, KnowledgeError> {
            Ok(vec![KnowledgeHit {
                id: query.text.clone(),
                title: "Match".to_string(),
                excerpt: "excerpt".to_string(),
                score: 1.0,
                tags: vec![],
                uri: "mock://entry".to_string(),
                source: "mock".to_string(),
                metadata: json!({}),
            }])
        }

        async fn get(&self, id: &str) -> Result<Option<KnowledgeEntry>, KnowledgeError> {
            Ok(Some(KnowledgeEntry {
                id: id.to_string(),
                title: "Match".to_string(),
                content: "full content".to_string(),
                tags: vec![],
                uri: "mock://entry".to_string(),
                source: "mock".to_string(),
                metadata: json!({}),
                created_at_ms: 1,
                updated_at_ms: 1,
            }))
        }

        async fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>, KnowledgeError> {
            Ok(vec![KnowledgeSourceInfo {
                provider: "mock".to_string(),
                name: "Mock".to_string(),
                description: "mock source".to_string(),
                entry_count: 1,
            }])
        }
    }

    #[tokio::test]
    async fn routes_calls_to_registered_provider() {
        let mut router = KnowledgeProviderRouter::new();
        router.register(MockProvider);

        let hits = router
            .search(
                "mock",
                KnowledgeSearchQuery {
                    text: "auth".to_string(),
                    ..Default::default()
                },
            )
            .await
            .expect("search should succeed");
        assert_eq!(hits[0].id, "auth");

        let entry = router
            .get_entry("mock", "auth")
            .await
            .expect("get should succeed");
        assert_eq!(entry.expect("entry").content, "full content");
    }

    #[tokio::test]
    async fn rejects_unknown_provider() {
        let router = KnowledgeProviderRouter::new();
        let err = router
            .list_sources("missing")
            .await
            .expect_err("missing provider should fail");
        assert!(matches!(err, KnowledgeError::SourceUnavailable(_)));
    }
}
