use async_trait::async_trait;

use crate::{SkillError, SkillRecord, SkillSummary};

#[async_trait]
pub trait SkillStore: Send + Sync {
    async fn download(&self, skill_name: &str) -> Result<SkillRecord, SkillError>;
    async fn download_with_source(
        &self,
        skill_name: &str,
        source_name: &str,
        download_url_template: &str,
    ) -> Result<SkillRecord, SkillError>;
    async fn delete(&self, skill_name: &str) -> Result<(), SkillError>;
    async fn list(&self) -> Result<Vec<SkillSummary>, SkillError>;
    async fn get(&self, skill_name: &str) -> Result<SkillRecord, SkillError>;
    async fn update(&self, skill_name: &str) -> Result<SkillRecord, SkillError>;
    async fn update_with_source(
        &self,
        skill_name: &str,
        source_name: &str,
        download_url_template: &str,
    ) -> Result<SkillRecord, SkillError>;
    async fn load_all_skill_markdowns(&self) -> Result<Vec<SkillRecord>, SkillError>;
}
