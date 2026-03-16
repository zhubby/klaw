mod error;
mod fetcher;
mod fs_store;
mod model;
mod store;

pub use error::SkillError;
pub use fetcher::{ReqwestSkillFetcher, SkillFetcher};
pub use fs_store::{
    open_default_skill_store, FileSystemSkillStore, InstalledSkill, RegistrySource,
    RegistrySyncReport, SkillUninstallResult,
};
pub use model::{RegistrySkillSummary, SkillRecord, SkillSource, SkillSourceKind, SkillSummary};
pub use store::SkillStore;
