use std::{env, io, path::PathBuf};
use thiserror::Error;
use tokio::fs;

pub const SYSTEM_PROMPT_FILE_NAME: &str = "SYSTEM.md";
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful AI assistant";

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("home directory is unavailable")]
    HomeDirUnavailable,
    #[error("failed to create data directory `{path}`: {source}")]
    CreateDataDir { path: String, source: io::Error },
    #[error("failed to read system prompt file `{path}`: {source}")]
    ReadSystemPrompt { path: String, source: io::Error },
    #[error("failed to write system prompt file `{path}`: {source}")]
    WriteSystemPrompt { path: String, source: io::Error },
}

pub async fn load_or_create_system_prompt() -> Result<String, PromptError> {
    let data_dir = default_data_dir()?;
    load_or_create_system_prompt_in_dir(data_dir).await
}

pub async fn load_or_create_system_prompt_in_dir(data_dir: PathBuf) -> Result<String, PromptError> {
    fs::create_dir_all(&data_dir)
        .await
        .map_err(|source| PromptError::CreateDataDir {
            path: data_dir.display().to_string(),
            source,
        })?;

    let prompt_path = data_dir.join(SYSTEM_PROMPT_FILE_NAME);
    match fs::read_to_string(&prompt_path).await {
        Ok(content) => Ok(content),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            fs::write(&prompt_path, DEFAULT_SYSTEM_PROMPT)
                .await
                .map_err(|source| PromptError::WriteSystemPrompt {
                    path: prompt_path.display().to_string(),
                    source,
                })?;
            Ok(DEFAULT_SYSTEM_PROMPT.to_string())
        }
        Err(source) => Err(PromptError::ReadSystemPrompt {
            path: prompt_path.display().to_string(),
            source,
        }),
    }
}

fn default_data_dir() -> Result<PathBuf, PromptError> {
    let home = env::var_os("HOME").ok_or(PromptError::HomeDirUnavailable)?;
    Ok(PathBuf::from(home).join(".klaw"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::fs;
    use uuid::Uuid;

    #[tokio::test(flavor = "current_thread")]
    async fn load_or_create_writes_default_when_missing() {
        let data_dir = std::env::temp_dir().join(format!("klaw-prompt-test-{}", Uuid::new_v4()));

        let content = load_or_create_system_prompt_in_dir(data_dir.clone())
            .await
            .expect("should create default prompt");
        assert_eq!(content, DEFAULT_SYSTEM_PROMPT);

        let written = fs::read_to_string(data_dir.join(SYSTEM_PROMPT_FILE_NAME))
            .await
            .expect("SYSTEM.md should exist");
        assert_eq!(written, DEFAULT_SYSTEM_PROMPT);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn load_or_create_reads_existing_file() {
        let data_dir = std::env::temp_dir().join(format!("klaw-prompt-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&data_dir)
            .await
            .expect("temp data dir should be created");
        let expected = "custom system prompt";
        fs::write(data_dir.join(SYSTEM_PROMPT_FILE_NAME), expected)
            .await
            .expect("SYSTEM.md should be written");

        let content = load_or_create_system_prompt_in_dir(data_dir)
            .await
            .expect("should load existing prompt");
        assert_eq!(content, expected);
    }
}
