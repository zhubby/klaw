use crate::ModelError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HuggingFaceModelRef {
    pub repo_id: String,
    pub revision: String,
}

impl HuggingFaceModelRef {
    pub fn new(
        repo_id: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, ModelError> {
        let repo_id = repo_id.into().trim().to_string();
        if repo_id.is_empty() || !repo_id.contains('/') {
            return Err(ModelError::Config(
                "huggingface repo_id must look like owner/name".to_string(),
            ));
        }
        let revision = revision.into().trim().to_string();
        if revision.is_empty() {
            return Err(ModelError::Config(
                "huggingface revision cannot be empty".to_string(),
            ));
        }
        Ok(Self { repo_id, revision })
    }
}

pub fn normalize_model_id(repo_id: &str, revision: &str) -> String {
    format!(
        "{}--{}",
        repo_id.replace('/', "__"),
        revision.replace('/', "__")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_model_id_from_repo_and_revision() {
        assert_eq!(
            normalize_model_id("Qwen/Qwen3-Embedding-0.6B-GGUF", "main"),
            "Qwen__Qwen3-Embedding-0.6B-GGUF--main"
        );
    }

    #[test]
    fn rejects_invalid_huggingface_repo_shape() {
        let err = HuggingFaceModelRef::new("qwen", "main").expect_err("repo should fail");
        assert!(matches!(err, ModelError::Config(_)));
    }
}
