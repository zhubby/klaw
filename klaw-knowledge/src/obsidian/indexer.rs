use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use ignore::WalkBuilder;
use klaw_storage::{DatabaseExecutor, DbRow, DbValue};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    KnowledgeError, KnowledgeSyncProgress, KnowledgeSyncProgressStage, models::EmbeddingModel,
};

use super::{
    chunker::{Chunk, ParsedMarkdown, chunk_markdown},
    links::{
        DiscoveredLink, LinkMatchType, NameEntry, NoteLinkTarget, build_name_index, discover_links,
    },
    parser::{ParsedNote, parse_note},
};

pub(crate) const KNOWLEDGE_VECTOR_INDEX_NAME: &str = "idx_knowledge_chunks_embedding";
const KNOWLEDGE_METADATA_TABLE: &str = "knowledge_metadata";
const EMBEDDING_DIMENSIONS_KEY: &str = "knowledge_embedding_dimensions";
const VECTOR_INDEX_ENABLED_KEY: &str = "knowledge_vector_index_enabled";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexedNote {
    pub path: String,
    pub parsed: ParsedNote,
    pub markdown: ParsedMarkdown,
}

pub async fn init_schema(db: &Arc<dyn DatabaseExecutor>) -> Result<(), KnowledgeError> {
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
            target_title TEXT NOT NULL,
            target_entry_id TEXT,
            matched_text TEXT,
            match_type TEXT,
            confidence_bp INTEGER
        );
        CREATE TABLE IF NOT EXISTS knowledge_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
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
            ensure_link_columns(db).await?;
            return Ok(());
        }
        return Err(KnowledgeError::Provider(message));
    }
    ensure_link_columns(db).await?;
    Ok(())
}

async fn ensure_link_columns(db: &Arc<dyn DatabaseExecutor>) -> Result<(), KnowledgeError> {
    for (name, definition) in [
        ("target_entry_id", "target_entry_id TEXT"),
        ("matched_text", "matched_text TEXT"),
        ("match_type", "match_type TEXT"),
        ("confidence_bp", "confidence_bp INTEGER"),
    ] {
        if !table_has_column(db, "knowledge_links", name).await? {
            db.execute_batch(&format!(
                "ALTER TABLE knowledge_links ADD COLUMN {definition}"
            ))
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        }
    }
    Ok(())
}

pub(crate) async fn ensure_vector_index(
    db: &Arc<dyn DatabaseExecutor>,
    dimensions: usize,
) -> Result<bool, KnowledgeError> {
    if dimensions == 0 {
        return Err(KnowledgeError::Provider(
            "knowledge embedding vectors cannot be empty".to_string(),
        ));
    }

    init_schema(db).await?;
    if let Some(existing_dimensions) = embedding_dimensions(db).await?
        && existing_dimensions != dimensions
    {
        return Err(KnowledgeError::Provider(format!(
            "knowledge embedding dimension changed from {existing_dimensions} to {dimensions}; delete knowledge.db and re-sync"
        )));
    }

    ensure_vector_column(db, dimensions).await?;
    set_metadata_value(db, EMBEDDING_DIMENSIONS_KEY, &dimensions.to_string()).await?;

    let create_index = format!(
        "CREATE INDEX IF NOT EXISTS {KNOWLEDGE_VECTOR_INDEX_NAME}
         ON knowledge_chunks(libsql_vector_idx(embedding))"
    );
    match db.execute_batch(&create_index).await {
        Ok(()) => {
            set_metadata_value(db, VECTOR_INDEX_ENABLED_KEY, "true").await?;
            Ok(true)
        }
        Err(err) if is_vector_capability_error(&err.to_string()) => {
            set_metadata_value(db, VECTOR_INDEX_ENABLED_KEY, "false").await?;
            Ok(false)
        }
        Err(err) => Err(KnowledgeError::Provider(err.to_string())),
    }
}

pub(crate) async fn has_vector_index(
    db: &Arc<dyn DatabaseExecutor>,
) -> Result<bool, KnowledgeError> {
    let rows = db
        .query(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'index' AND name = ?1",
            &[DbValue::Text(KNOWLEDGE_VECTOR_INDEX_NAME.to_string())],
        )
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    Ok(rows.first().and_then(|row| integer_at(row, 0)).unwrap_or(0) > 0)
}

async fn ensure_vector_column(
    db: &Arc<dyn DatabaseExecutor>,
    dimensions: usize,
) -> Result<(), KnowledgeError> {
    let expected_type = vector_column_type(dimensions);
    if embedding_column_type(db)
        .await?
        .is_some_and(|column_type| is_vector_column_type(&column_type, dimensions))
    {
        return Ok(());
    }

    let rebuild = format!(
        "DROP INDEX IF EXISTS {KNOWLEDGE_VECTOR_INDEX_NAME};
         DROP TABLE IF EXISTS knowledge_chunks_vector_new;
         CREATE TABLE knowledge_chunks_vector_new (
            id TEXT PRIMARY KEY,
            entry_id TEXT NOT NULL,
            heading TEXT,
            content TEXT NOT NULL,
            snippet TEXT NOT NULL,
            embedding {expected_type}
         );
         INSERT INTO knowledge_chunks_vector_new (id, entry_id, heading, content, snippet, embedding)
         SELECT id, entry_id, heading, content, snippet, embedding
         FROM knowledge_chunks;
         DROP TABLE knowledge_chunks;
         ALTER TABLE knowledge_chunks_vector_new RENAME TO knowledge_chunks;"
    );
    db.execute_batch(&rebuild)
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))
}

async fn embedding_dimensions(
    db: &Arc<dyn DatabaseExecutor>,
) -> Result<Option<usize>, KnowledgeError> {
    let rows = db
        .query(
            &format!("SELECT value FROM {KNOWLEDGE_METADATA_TABLE} WHERE key = ?1 LIMIT 1"),
            &[DbValue::Text(EMBEDDING_DIMENSIONS_KEY.to_string())],
        )
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    rows.first()
        .and_then(|row| text_at(row, 0))
        .map(|value| {
            value.parse::<usize>().map_err(|err| {
                KnowledgeError::Provider(format!("invalid embedding dimension metadata: {err}"))
            })
        })
        .transpose()
}

async fn embedding_column_type(
    db: &Arc<dyn DatabaseExecutor>,
) -> Result<Option<String>, KnowledgeError> {
    let rows = db
        .query("PRAGMA table_info(knowledge_chunks)", &[])
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    Ok(rows
        .iter()
        .find(|row| text_at(row, 1).as_deref() == Some("embedding"))
        .and_then(|row| text_at(row, 2)))
}

async fn table_has_column(
    db: &Arc<dyn DatabaseExecutor>,
    table: &str,
    column: &str,
) -> Result<bool, KnowledgeError> {
    let rows = db
        .query(&format!("PRAGMA table_info({table})"), &[])
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    Ok(rows
        .iter()
        .any(|row| text_at(row, 1).as_deref() == Some(column)))
}

async fn set_metadata_value(
    db: &Arc<dyn DatabaseExecutor>,
    key: &str,
    value: &str,
) -> Result<(), KnowledgeError> {
    db.execute(
        &format!(
            "INSERT INTO {KNOWLEDGE_METADATA_TABLE} (key, value)
             VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value"
        ),
        &[
            DbValue::Text(key.to_string()),
            DbValue::Text(value.to_string()),
        ],
    )
    .await
    .map(|_| ())
    .map_err(|err| KnowledgeError::Provider(err.to_string()))
}

fn vector_column_type(dimensions: usize) -> String {
    format!("F32_BLOB({dimensions})")
}

fn is_vector_column_type(column_type: &str, dimensions: usize) -> bool {
    let normalized = column_type.to_ascii_uppercase();
    normalized == "F32_BLOB" || normalized == vector_column_type(dimensions)
}

fn is_vector_capability_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("no such function")
        || normalized.contains("no such module")
        || normalized.contains("unknown function")
        || normalized.contains("invalid expression in create index")
}

pub async fn index_vault(
    db: Arc<dyn DatabaseExecutor>,
    vault_root: &Path,
    exclude_folders: &[String],
    max_excerpt_length: usize,
    embedder: Option<&dyn EmbeddingModel>,
) -> Result<usize, KnowledgeError> {
    index_vault_with_progress(
        db,
        vault_root,
        exclude_folders,
        max_excerpt_length,
        embedder,
        |_| {},
    )
    .await
}

pub async fn index_vault_with_progress<F>(
    db: Arc<dyn DatabaseExecutor>,
    vault_root: &Path,
    exclude_folders: &[String],
    max_excerpt_length: usize,
    embedder: Option<&dyn EmbeddingModel>,
    mut progress: F,
) -> Result<usize, KnowledgeError>
where
    F: FnMut(KnowledgeSyncProgress),
{
    init_schema(&db).await?;
    let files = collect_markdown_files(vault_root, exclude_folders)?;
    let name_index = build_vault_name_index(vault_root, &files)?;
    remove_missing_entries(db.clone(), vault_root, &files).await?;
    let total = files.len();
    let mut indexed = 0usize;
    for (index, path) in files.into_iter().enumerate() {
        indexed += usize::from(
            index_note_path_with_links(
                db.clone(),
                vault_root,
                &path,
                max_excerpt_length,
                embedder,
                &name_index,
            )
            .await?,
        );
        progress(KnowledgeSyncProgress {
            stage: KnowledgeSyncProgressStage::IndexingNotes,
            completed: index + 1,
            total: Some(total),
            current_item: Some(relative_display_path(vault_root, &path)?),
        });
    }
    Ok(indexed)
}

pub async fn embed_missing_chunks(
    db: Arc<dyn DatabaseExecutor>,
    embedder: &dyn EmbeddingModel,
) -> Result<usize, KnowledgeError> {
    embed_missing_chunks_with_progress(db, embedder, |_| {}).await
}

pub async fn embed_missing_chunks_with_progress<F>(
    db: Arc<dyn DatabaseExecutor>,
    embedder: &dyn EmbeddingModel,
    mut progress: F,
) -> Result<usize, KnowledgeError>
where
    F: FnMut(KnowledgeSyncProgress),
{
    let rows = db
        .query(
            "SELECT c.id, c.heading, c.content, e.title, e.uri
             FROM knowledge_chunks c
             JOIN knowledge_entries e ON e.id = c.entry_id
             WHERE c.embedding IS NULL",
            &[],
        )
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;

    let total = rows.len();
    let mut embedded = 0usize;
    for row in rows {
        let Some(chunk_id) = text_at(&row, 0) else {
            continue;
        };
        let heading = text_at(&row, 1);
        let Some(content) = text_at(&row, 2) else {
            continue;
        };
        let title = text_at(&row, 3).unwrap_or_else(|| "untitled".to_string());
        let current_item = text_at(&row, 4).or_else(|| Some(chunk_id.clone()));
        let mut text = String::new();
        text.push_str(heading.as_deref().unwrap_or(title.as_str()));
        text.push_str("\n\n");
        text.push_str(&content);
        let vector = embedder.embed(&text).await?;
        ensure_vector_index(&db, vector.len()).await?;
        db.execute(
            "UPDATE knowledge_chunks SET embedding = ?1 WHERE id = ?2",
            &[
                DbValue::Blob(serialize_embedding(&vector)),
                DbValue::Text(chunk_id),
            ],
        )
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        embedded += 1;
        progress(KnowledgeSyncProgress {
            stage: KnowledgeSyncProgressStage::EmbeddingChunks,
            completed: embedded,
            total: Some(total),
            current_item,
        });
    }

    Ok(embedded)
}

pub async fn index_note_path(
    db: Arc<dyn DatabaseExecutor>,
    vault_root: &Path,
    absolute_path: &Path,
    max_excerpt_length: usize,
    embedder: Option<&dyn EmbeddingModel>,
) -> Result<bool, KnowledgeError> {
    let files = collect_markdown_files(vault_root, &[])?;
    let name_index = build_vault_name_index(vault_root, &files)?;
    index_note_path_with_links(
        db,
        vault_root,
        absolute_path,
        max_excerpt_length,
        embedder,
        &name_index,
    )
    .await
}

async fn index_note_path_with_links(
    db: Arc<dyn DatabaseExecutor>,
    vault_root: &Path,
    absolute_path: &Path,
    max_excerpt_length: usize,
    embedder: Option<&dyn EmbeddingModel>,
    name_index: &[NameEntry],
) -> Result<bool, KnowledgeError> {
    init_schema(&db).await?;
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
    for link in links_for_note(&content, &parsed, &entry_id, name_index) {
        insert_link(db.clone(), &entry_id, &link).await?;
    }

    Ok(true)
}

fn build_vault_name_index(
    vault_root: &Path,
    files: &[PathBuf],
) -> Result<Vec<NameEntry>, KnowledgeError> {
    let targets = files
        .iter()
        .map(|path| {
            let content = std::fs::read_to_string(path)
                .map_err(|err| KnowledgeError::Provider(format!("read note failed: {err}")))?;
            let parsed = parse_note(&content);
            let relative_path = path
                .strip_prefix(vault_root)
                .map_err(|err| KnowledgeError::Provider(format!("strip prefix failed: {err}")))?
                .to_string_lossy()
                .replace('\\', "/");
            let title = parsed
                .title
                .clone()
                .unwrap_or_else(|| default_title_from_path(path));
            Ok(NoteLinkTarget {
                path: relative_path,
                title,
                aliases: parsed.aliases,
            })
        })
        .collect::<Result<Vec<_>, KnowledgeError>>()?;
    Ok(build_name_index(targets))
}

fn links_for_note(
    content: &str,
    parsed: &ParsedNote,
    entry_id: &str,
    name_index: &[NameEntry],
) -> Vec<IndexedLink> {
    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for wikilink in &parsed.wikilinks {
        let (target_entry_id, target_title) = resolve_wikilink_target(wikilink, name_index)
            .map(|entry| (Some(entry.path.clone()), entry.title.clone()))
            .unwrap_or_else(|| (None, wikilink.clone()));
        push_indexed_link(
            &mut links,
            &mut seen,
            IndexedLink {
                target_entry_id,
                target_title,
                matched_text: Some(wikilink.clone()),
                match_type: "wikilink",
                confidence_bp: None,
            },
            entry_id,
        );
    }

    for discovered in discover_links(content, name_index, Some("People")) {
        push_discovered_link(&mut links, &mut seen, discovered, entry_id);
    }

    links
}

fn push_discovered_link(
    links: &mut Vec<IndexedLink>,
    seen: &mut std::collections::HashSet<(Option<String>, String, String, &'static str)>,
    discovered: DiscoveredLink,
    entry_id: &str,
) {
    push_indexed_link(
        links,
        seen,
        IndexedLink {
            target_entry_id: Some(discovered.target_path),
            target_title: discovered.target_title,
            matched_text: Some(discovered.matched_text),
            match_type: discovered.match_type.as_str(),
            confidence_bp: discovered.match_type.confidence_bp().map(i64::from),
        },
        entry_id,
    );
}

fn push_indexed_link(
    links: &mut Vec<IndexedLink>,
    seen: &mut std::collections::HashSet<(Option<String>, String, String, &'static str)>,
    link: IndexedLink,
    entry_id: &str,
) {
    if link.target_entry_id.as_deref() == Some(entry_id) {
        return;
    }
    let key = (
        link.target_entry_id.clone(),
        link.target_title.clone(),
        link.matched_text.clone().unwrap_or_default(),
        link.match_type,
    );
    if seen.insert(key) {
        links.push(link);
    }
}

fn resolve_wikilink_target<'a>(
    wikilink: &str,
    name_index: &'a [NameEntry],
) -> Option<&'a NameEntry> {
    let normalized = wikilink.trim().trim_end_matches(".md").to_ascii_lowercase();
    name_index.iter().find(|entry| {
        matches!(
            entry.match_type,
            LinkMatchType::ExactName | LinkMatchType::Alias
        ) && (entry.name_lower == normalized
            || entry
                .path
                .trim_end_matches(".md")
                .eq_ignore_ascii_case(wikilink))
    })
}

async fn insert_link(
    db: Arc<dyn DatabaseExecutor>,
    source_entry_id: &str,
    link: &IndexedLink,
) -> Result<(), KnowledgeError> {
    db.execute(
        "INSERT INTO knowledge_links (
            source_entry_id, target_title, target_entry_id, matched_text, match_type, confidence_bp
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        &[
            DbValue::Text(source_entry_id.to_string()),
            DbValue::Text(link.target_title.clone()),
            link.target_entry_id
                .clone()
                .map(DbValue::Text)
                .unwrap_or(DbValue::Null),
            link.matched_text
                .clone()
                .map(DbValue::Text)
                .unwrap_or(DbValue::Null),
            DbValue::Text(link.match_type.to_string()),
            link.confidence_bp
                .map(DbValue::Integer)
                .unwrap_or(DbValue::Null),
        ],
    )
    .await
    .map(|_| ())
    .map_err(|err| KnowledgeError::Provider(err.to_string()))
}

#[derive(Debug, Clone)]
struct IndexedLink {
    target_entry_id: Option<String>,
    target_title: String,
    matched_text: Option<String>,
    match_type: &'static str,
    confidence_bp: Option<i64>,
}

pub async fn remove_note_path(
    db: Arc<dyn DatabaseExecutor>,
    relative_path: &str,
) -> Result<(), KnowledgeError> {
    delete_entry(db, relative_path).await
}

pub async fn has_indexed_entries(db: Arc<dyn DatabaseExecutor>) -> Result<bool, KnowledgeError> {
    let rows = db
        .query("SELECT COUNT(*) FROM knowledge_entries", &[])
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    Ok(integer_at(
        rows.first()
            .ok_or_else(|| KnowledgeError::Provider("entry count query returned no rows".into()))?,
        0,
    )
    .unwrap_or_default()
        > 0)
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

fn relative_display_path(
    vault_root: &Path,
    absolute_path: &Path,
) -> Result<String, KnowledgeError> {
    Ok(absolute_path
        .strip_prefix(vault_root)
        .map_err(|err| KnowledgeError::Provider(format!("strip prefix failed: {err}")))?
        .to_string_lossy()
        .replace('\\', "/"))
}

async fn is_entry_up_to_date(
    db: &Arc<dyn DatabaseExecutor>,
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

async fn remove_missing_entries(
    db: Arc<dyn DatabaseExecutor>,
    vault_root: &Path,
    files: &[PathBuf],
) -> Result<(), KnowledgeError> {
    let existing_files = files
        .iter()
        .filter_map(|path| relative_display_path(vault_root, path).ok())
        .collect::<std::collections::BTreeSet<_>>();
    let rows = db
        .query("SELECT id FROM knowledge_entries", &[])
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    for row in rows {
        let Some(entry_id) = text_at(&row, 0) else {
            continue;
        };
        if !existing_files.contains(&entry_id) {
            delete_entry(db.clone(), &entry_id).await?;
        }
    }
    Ok(())
}

async fn delete_entry(db: Arc<dyn DatabaseExecutor>, entry_id: &str) -> Result<(), KnowledgeError> {
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
    db: Arc<dyn DatabaseExecutor>,
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
            let vector = embedder.embed(&text).await?;
            ensure_vector_index(&db, vector.len()).await?;
            Some(vector)
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

pub(crate) fn serialize_embedding(vector: &[f32]) -> Vec<u8> {
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

fn text_at(row: &DbRow, index: usize) -> Option<String> {
    match row.get(index)? {
        DbValue::Text(value) => Some(value.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use async_trait::async_trait;
    use klaw_storage::{DefaultKnowledgeDb, StoragePaths};

    use crate::{KnowledgeSyncProgressStage, models::EmbeddingModel};

    use super::*;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn open_test_db(name: &str) -> Arc<dyn DatabaseExecutor> {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("klaw-knowledge-{name}-{suffix}"));
        let paths = StoragePaths::from_root(base);
        Arc::new(
            DefaultKnowledgeDb::open_knowledge(paths)
                .await
                .expect("knowledge db should open"),
        )
    }

    #[derive(Default)]
    struct MockEmbeddingModel;

    #[async_trait]
    impl EmbeddingModel for MockEmbeddingModel {
        async fn embed(&self, text: &str) -> Result<Vec<f32>, KnowledgeError> {
            Ok(vec![text.len() as f32, 1.0, 0.5])
        }
    }

    #[tokio::test]
    async fn embedding_indexing_initializes_native_turso_vector_schema() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let vault = std::env::temp_dir().join(format!("klaw-knowledge-vector-vault-{suffix}"));
        std::fs::create_dir_all(&vault).expect("vault dir");
        std::fs::write(
            vault.join("cookies.md"),
            "# Cookies\nCookie storage details.",
        )
        .expect("write note");

        let db = open_test_db("vector-schema").await;
        index_vault(db.clone(), &vault, &[], 400, Some(&MockEmbeddingModel))
            .await
            .expect("index with embeddings should succeed");

        let column_rows = db
            .query("PRAGMA table_info(knowledge_chunks)", &[])
            .await
            .expect("table info should load");
        let embedding_type = column_rows
            .iter()
            .find(|row| text_at(row, 1).as_deref() == Some("embedding"))
            .and_then(|row| text_at(row, 2))
            .expect("embedding column should exist");
        assert_eq!(embedding_type, "F32_BLOB");

        let metadata_rows = db
            .query(
                "SELECT value FROM knowledge_metadata WHERE key = 'knowledge_embedding_dimensions'",
                &[],
            )
            .await
            .expect("metadata query should load");
        assert_eq!(text_at(&metadata_rows[0], 0).as_deref(), Some("3"));

        let index_rows = db
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_knowledge_chunks_embedding'",
                &[],
            )
            .await
            .expect("index query should load");
        let vector_enabled_rows = db
            .query(
                "SELECT value FROM knowledge_metadata WHERE key = 'knowledge_vector_index_enabled'",
                &[],
            )
            .await
            .expect("vector metadata should load");
        let vector_index_enabled = text_at(&vector_enabled_rows[0], 0).as_deref() == Some("true");
        assert_eq!(
            integer_at(&index_rows[0], 0),
            Some(i64::from(vector_index_enabled))
        );
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

    #[tokio::test]
    async fn indexes_discovered_links_with_resolved_targets() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let vault = std::env::temp_dir().join(format!("klaw-knowledge-links-vault-{suffix}"));
        std::fs::create_dir_all(&vault).expect("vault dir");
        std::fs::write(
            vault.join("auth.md"),
            "# Auth\nCookies and RRF keep browser state.",
        )
        .expect("write auth");
        std::fs::write(vault.join("Cookies.md"), "# Cookies\nCookie storage.").expect("cookies");
        std::fs::write(
            vault.join("Reciprocal Rank Fusion.md"),
            "---\naliases: [RRF]\n---\n# Reciprocal Rank Fusion\nSearch fusion.",
        )
        .expect("rrf");

        let db = open_test_db("discovered-links").await;
        index_vault(db.clone(), &vault, &[], 400, None)
            .await
            .expect("index should succeed");

        let rows = db
            .query(
                "SELECT target_entry_id, matched_text, match_type
                 FROM knowledge_links
                 WHERE source_entry_id = 'auth.md'
                 ORDER BY matched_text",
                &[],
            )
            .await
            .expect("query links");
        let links: Vec<(String, String, String)> = rows
            .iter()
            .filter_map(|row| Some((text_at(row, 0)?, text_at(row, 1)?, text_at(row, 2)?)))
            .collect();

        assert!(links.contains(&(
            "Cookies.md".to_string(),
            "Cookies".to_string(),
            "exact_name".to_string()
        )));
        assert!(links.contains(&(
            "Reciprocal Rank Fusion.md".to_string(),
            "RRF".to_string(),
            "alias".to_string()
        )));
    }

    #[tokio::test]
    async fn index_vault_reports_file_progress() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let vault = std::env::temp_dir().join(format!("klaw-knowledge-progress-vault-{suffix}"));
        std::fs::create_dir_all(&vault).expect("vault dir");
        std::fs::write(vault.join("auth.md"), "# Auth").expect("write auth");
        std::fs::write(vault.join("runtime.md"), "# Runtime").expect("write runtime");

        let db = open_test_db("progress").await;
        let mut progress_events = Vec::new();
        let indexed = index_vault_with_progress(db, &vault, &[], 400, None, |progress| {
            progress_events.push(progress);
        })
        .await
        .expect("index should succeed");

        assert_eq!(indexed, 2);
        assert_eq!(progress_events.len(), 2);
        assert!(progress_events.iter().all(|progress| {
            progress.stage == KnowledgeSyncProgressStage::IndexingNotes
                && progress.total == Some(2)
                && progress.completed > 0
                && progress
                    .current_item
                    .as_deref()
                    .is_some_and(|item| item.ends_with(".md"))
        }));
    }

    #[tokio::test]
    async fn remove_note_path_clears_entry_chunks_fts_and_links() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let vault = std::env::temp_dir().join(format!("klaw-knowledge-remove-vault-{suffix}"));
        std::fs::create_dir_all(&vault).expect("vault dir");
        let auth_path = vault.join("auth.md");
        std::fs::write(&auth_path, "# Auth\nSee [[Cookies]]").expect("write auth");

        let db = open_test_db("remove").await;
        index_note_path(db.clone(), &vault, &auth_path, 400, None)
            .await
            .expect("index should succeed");

        assert!(has_indexed_entries(db.clone()).await.expect("has entries"));
        remove_note_path(db.clone(), "auth.md")
            .await
            .expect("remove should succeed");

        for table in [
            "knowledge_entries",
            "knowledge_chunks",
            "knowledge_fts",
            "knowledge_links",
        ] {
            let rows = db
                .query(&format!("SELECT COUNT(*) FROM {table}"), &[])
                .await
                .expect("count should load");
            assert_eq!(integer_at(&rows[0], 0), Some(0), "{table} should be empty");
        }
        assert!(!has_indexed_entries(db).await.expect("has entries"));
    }
}
