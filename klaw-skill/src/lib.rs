mod error;
mod fetcher;
mod fs_store;
mod model;
mod store;

pub use error::SkillError;
pub use fetcher::{ReqwestSkillFetcher, SkillFetcher};
pub use fs_store::{
    open_default_skill_manager, open_default_skill_registry, FileSystemSkillStore, InstalledSkill,
    RegistrySource, RegistrySyncReport, SkillUninstallResult,
};
pub use model::{
    RegistrySkillMatch, RegistrySkillSummary, SkillRecord, SkillSource, SkillSourceKind,
    SkillSummary,
};
pub use store::{SkillManager, SkillsRegistry};
