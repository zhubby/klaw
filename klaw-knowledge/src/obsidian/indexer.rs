use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use ignore::WalkBuilder;
use klaw_storage::{DbRow, DbValue, MemoryDb};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{KnowledgeError, models::EmbeddingModel};

use super::{
    chunker::{Chunk, ParsedMarkdown, chunk_markdown},
    parser::{ParsedNote, parse_note},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexedNote {
    pub path: String,
    pub parsed: ParsedNote,
    pub markdown: ParsedMarkdown,
}

pub async fn init_schema(db: &Arc<dyn MemoryDb>) -> Result<(), KnowledgeError> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS knowledge_entries (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            uri TEXT NOT NULL UNIQUE,
            tags_json TEXT NOT NULL,
            aliases_json TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            content TEXT NOT NULL,
            note_date TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS knowledge_chunks (
            id TEXT PRIMARY KEY,
            entry_id TEXT NOT NULL,
            heading TEXT,
            content TEXT NOT NULL,
            snippet TEXT NOT NULL,
            embedding BLOB
        );
        CREATE TABLE IF NOT EXISTS knowledge_links (
            source_entry_id TEXT NOT NULL,
            target_title TEXT NOT NULL
        );",
    )
    .await
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;

    let fts_result = db
        .execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(
                chunk_id UNINDEXED,
                entry_id UNINDEXED,
                title,
                aliases,
                tags,
                content
            );",
        )
        .await;
    if let Err(err) = fts_result {
        let message = err.to_string();
        if message.contains("no such module: fts5") {
            db.execute_batch(
                "CREATE TABLE IF NOT EXISTS knowledge_fts (
                    chunk_id TEXT NOT NULL,
                    entry_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    aliases TEXT NOT NULL,
                    tags TEXT NOT NULL,
                    content TEXT NOT NULL
                );",
            )
            .await
            .map_err(|fallback_err| KnowledgeError::Provider(fallback_err.to_string()))?;
            return Ok(());
        }
        return Err(KnowledgeError::Provider(message));
    }
    Ok(())
}

pub async fn index_vault(
    db: Arc<dyn MemoryDb>,
    vault_root: &Path,
    exclude_folders: &[String],
    max_excerpt_length: usize,
    embedder: Option<&dyn EmbeddingModel>,
) -> Result<usize, KnowledgeError> {
    init_schema(&db).await?;
    let files = collect_markdown_files(vault_root, exclude_folders)?;
    let mut indexed = 0usize;
    for path in files {
        indexed += usize::from(
            index_note_path(db.clone(), vault_root, &path, max_excerpt_length, embedder).await?,
        );
    }
    Ok(indexed)
}

pub async fn index_note_path(
    db: Arc<dyn MemoryDb>,
    vault_root: &Path,
    absolute_path: &Path,
    max_excerpt_length: usize,
    embedder: Option<&dyn EmbeddingModel>,
) -> Result<bool, KnowledgeError> {
    let content = std::fs::read_to_string(absolute_path)
        .map_err(|err| KnowledgeError::Provider(format!("read note failed: {err}")))?;
    let relative_path = absolute_path
        .strip_prefix(vault_root)
        .map_err(|err| KnowledgeError::Provider(format!("strip prefix failed: {err}")))?
        .to_string_lossy()
        .replace('\\', "/");
    let updated_at_ms = file_mtime_ms(absolute_path)?;

    if is_entry_up_to_date(&db, &relative_path, updated_at_ms).await? {
        return Ok(false);
    }

    let parsed = parse_note(&content);
    let markdown = chunk_markdown(&content);
    let title = parsed
        .title
        .clone()
        .unwrap_or_else(|| default_title_from_path(absolute_path));
    let entry_id = relative_path.clone();
    let tags_json = serde_json::to_string(&parsed.tags)
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    let aliases_json = serde_json::to_string(&parsed.aliases)
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    let metadata_json = serde_json::to_string(&json!({
        "inline_tags": parsed.inline_tags,
        "wikilinks": parsed.wikilinks,
        "note_date": parsed.note_date,
    }))
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;

    delete_entry(db.clone(), &entry_id).await?;
    db.execute(
        "INSERT INTO knowledge_entries (
            id, title, uri, tags_json, aliases_json, metadata_json, content, note_date, created_at_ms, updated_at_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        &[
            DbValue::Text(entry_id.clone()),
            DbValue::Text(title.clone()),
            DbValue::Text(relative_path.clone()),
            DbValue::Text(tags_json),
            DbValue::Text(aliases_json.clone()),
            DbValue::Text(metadata_json),
            DbValue::Text(content.clone()),
            parsed
                .note_date
                .clone()
                .map(DbValue::Text)
                .unwrap_or(DbValue::Null),
            DbValue::Integer(updated_at_ms),
            DbValue::Integer(updated_at_ms),
        ],
    )
    .await
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;

    for (index, chunk) in markdown.chunks.iter().enumerate() {
        insert_chunk(
            db.clone(),
            &entry_id,
            &title,
            &parsed.aliases,
            &parsed.tags,
            index,
            chunk,
            max_excerpt_length,
            embedder,
        )
        .await?;
    }
    for wikilink in &parsed.wikilinks {
        db.execute(
            "INSERT INTO knowledge_links (source_entry_id, target_title) VALUES (?1, ?2)",
            &[
                DbValue::Text(entry_id.clone()),
                DbValue::Text(wikilink.clone()),
            ],
        )
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    }

    Ok(true)
}

pub fn collect_markdown_files(
    vault_root: &Path,
    exclude_folders: &[String],
) -> Result<Vec<PathBuf>, KnowledgeError> {
    let walker = WalkBuilder::new(vault_root)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .build();
    let mut files = Vec::new();
    for result in walker {
        let entry = result.map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        let path = entry.path();
        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
            || path.extension().and_then(|ext| ext.to_str()) != Some("md")
        {
            continue;
        }
        let relative = path
            .strip_prefix(vault_root)
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        if is_excluded(relative, exclude_folders) {
            continue;
        }
        files.push(path.to_path_buf());
    }
    Ok(files)
}

async fn is_entry_up_to_date(
    db: &Arc<dyn MemoryDb>,
    uri: &str,
    updated_at_ms: i64,
) -> Result<bool, KnowledgeError> {
    let rows = db
        .query(
            "SELECT updated_at_ms FROM knowledge_entries WHERE uri = ?1",
            &[DbValue::Text(uri.to_string())],
        )
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    Ok(rows.first().and_then(|row| integer_at(row, 0)) == Some(updated_at_ms))
}

async fn delete_entry(db: Arc<dyn MemoryDb>, entry_id: &str) -> Result<(), KnowledgeError> {
    db.execute(
        "DELETE FROM knowledge_fts WHERE entry_id = ?1",
        &[DbValue::Text(entry_id.to_string())],
    )
    .await
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    db.execute(
        "DELETE FROM knowledge_links WHERE source_entry_id = ?1",
        &[DbValue::Text(entry_id.to_string())],
    )
    .await
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    db.execute(
        "DELETE FROM knowledge_chunks WHERE entry_id = ?1",
        &[DbValue::Text(entry_id.to_string())],
    )
    .await
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    db.execute(
        "DELETE FROM knowledge_entries WHERE id = ?1",
        &[DbValue::Text(entry_id.to_string())],
    )
    .await
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    Ok(())
}

async fn insert_chunk(
    db: Arc<dyn MemoryDb>,
    entry_id: &str,
    title: &str,
    aliases: &[String],
    tags: &[String],
    index: usize,
    chunk: &Chunk,
    max_excerpt_length: usize,
    embedder: Option<&dyn EmbeddingModel>,
) -> Result<(), KnowledgeError> {
    let chunk_id = format!("{entry_id}#{index}");
    let snippet = trim_chars(&chunk.snippet, max_excerpt_length);
    let embedding = match embedder {
        Some(embedder) => {
            let mut text = String::new();
            if let Some(heading) = &chunk.heading {
                text.push_str(heading);
                text.push_str("\n\n");
            } else {
                text.push_str(title);
                text.push_str("\n\n");
            }
            text.push_str(&chunk.text);
            Some(embedder.embed(&text).await?)
        }
        None => None,
    };
    db.execute(
        "INSERT INTO knowledge_chunks (id, entry_id, heading, content, snippet, embedding)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        &[
            DbValue::Text(chunk_id.clone()),
            DbValue::Text(entry_id.to_string()),
            chunk
                .heading
                .clone()
                .map(DbValue::Text)
                .unwrap_or(DbValue::Null),
            DbValue::Text(chunk.text.clone()),
            DbValue::Text(snippet.clone()),
            embedding
                .map(|vector| DbValue::Blob(serialize_embedding(&vector)))
                .unwrap_or(DbValue::Null),
        ],
    )
    .await
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    db.execute(
        "INSERT INTO knowledge_fts (chunk_id, entry_id, title, aliases, tags, content)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        &[
            DbValue::Text(chunk_id),
            DbValue::Text(entry_id.to_string()),
            DbValue::Text(title.to_string()),
            DbValue::Text(aliases.join(" ")),
            DbValue::Text(tags.join(" ")),
            DbValue::Text(chunk.text.clone()),
        ],
    )
    .await
    .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    Ok(())
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars.max(1)).collect()
}

fn serialize_embedding(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>()
}

fn default_title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("untitled")
        .to_string()
}

fn file_mtime_ms(path: &Path) -> Result<i64, KnowledgeError> {
    let modified = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    Ok(modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64)
}

fn is_excluded(path: &Path, exclude_folders: &[String]) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        exclude_folders
            .iter()
            .any(|exclude| exclude.trim_matches('/') == name)
    })
}

fn integer_at(row: &DbRow, index: usize) -> Option<i64> {
    match row.get(index)? {
        DbValue::Integer(value) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use klaw_storage::{DefaultKnowledgeDb, StoragePaths};

    use super::*;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn open_test_db(name: &str) -> Arc<dyn MemoryDb> {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("klaw-knowledge-{name}-{suffix}"));
        let paths = StoragePaths::from_root(base);
        Arc::new(
            DefaultKnowledgeDb::open_knowledge(paths)
                .await
                .expect("knowledge db should open"),
        )
    }

    #[tokio::test]
    async fn indexes_markdown_files_and_respects_excludes() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let vault = std::env::temp_dir().join(format!("klaw-knowledge-vault-{suffix}"));
        std::fs::create_dir_all(vault.join(".obsidian")).expect("vault dir");
        std::fs::write(
            vault.join("auth.md"),
            "---\ntags: [auth]\n---\n# Auth\nSee [[Cookies]]",
        )
        .expect("write note");
        std::fs::write(vault.join(".obsidian/ignored.md"), "# Ignored").expect("write ignored");

        let db = open_test_db("index").await;
        let indexed = index_vault(db.clone(), &vault, &[String::from(".obsidian")], 400, None)
            .await
            .expect("index should succeed");
        assert_eq!(indexed, 1);

        let rows = db
            .query("SELECT COUNT(*) FROM knowledge_entries", &[])
            .await
            .expect("query entries");
        assert_eq!(integer_at(&rows[0], 0), Some(1));
    }
}
