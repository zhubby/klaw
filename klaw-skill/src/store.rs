use async_trait::async_trait;

use crate::{
    RegistrySkillMatch, RegistrySkillSummary, SkillError, SkillRecord, SkillSummary,
    SkillUninstallResult,
};

#[async_trait]
pub trait SkillsRegistry: Send + Sync {
    async fn list_source_skills(
        &self,
        source_name: &str,
    ) -> Result<Vec<RegistrySkillSummary>, SkillError>;
    async fn get_source_skill(
        &self,
        source_name: &str,
        skill_name: &str,
    ) -> Result<SkillRecord, SkillError>;
    async fn search_source_skills(
        &self,
        source_name: &str,
        query: &str,
    ) -> Result<Vec<RegistrySkillMatch>, SkillError>;
}

#[async_trait]
pub trait SkillsManager: Send + Sync {
    async fn install_from_registry(
        &self,
        source_name: &str,
        skill_name: &str,
    ) -> Result<(SkillRecord, bool), SkillError>;
    async fn uninstall_from_registry(
        &self,
        source_name: &str,
        skill_name: &str,
    ) -> Result<(), SkillError>;
    async fn uninstall(&self, skill_name: &str) -> Result<SkillUninstallResult, SkillError>;
    async fn list_installed(&self) -> Result<Vec<SkillSummary>, SkillError>;
    async fn get_installed(&self, skill_name: &str) -> Result<SkillRecord, SkillError>;
    async fn load_all_installed_skill_markdowns(&self) -> Result<Vec<SkillRecord>, SkillError>;
}
