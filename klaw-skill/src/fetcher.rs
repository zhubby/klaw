use async_trait::async_trait;

use crate::{SkillError, SkillSource};

#[async_trait]
pub trait SkillFetcher: Send + Sync {
    async fn fetch_markdown(&self, source: &SkillSource) -> Result<String, SkillError>;
}

#[derive(Clone, Debug)]
pub struct ReqwestSkillFetcher {
    client: reqwest::Client,
}

impl Default for ReqwestSkillFetcher {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl SkillFetcher for ReqwestSkillFetcher {
    async fn fetch_markdown(&self, source: &SkillSource) -> Result<String, SkillError> {
        let url = source.remote_markdown_url();
        let response =
            self.client
                .get(&url)
                .send()
                .await
                .map_err(|source| SkillError::Network {
                    url: url.clone(),
                    source,
                })?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(SkillError::SkillNotFound(source.skill_name().to_string()));
        }
        if !status.is_success() {
            return Err(SkillError::RemoteStatus {
                url,
                status: status.as_u16(),
            });
        }

        response.text().await.map_err(|source| SkillError::Network {
            url: url.clone(),
            source,
        })
    }
}
