mod error;
mod fetcher;
mod fs_store;
mod model;
mod store;

pub use error::SkillError;
pub use fetcher::{ReqwestSkillFetcher, SkillFetcher};
pub use fs_store::{
    FileSystemSkillStore, InstalledSkill, RegistryDeleteReport, RegistrySource, RegistrySyncReport,
    RegistrySyncStatus, SkillUninstallResult, open_default_skill_registry,
    open_default_skills_manager,
};
pub use model::{
    RegistrySkillMatch, RegistrySkillSummary, SkillRecord, SkillSource, SkillSourceKind,
    SkillSummary,
};
pub use store::{SkillsManager, SkillsRegistry};
