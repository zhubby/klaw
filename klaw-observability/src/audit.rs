use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::RwLock;
use time::OffsetDateTime;
use uuid::Uuid;

pub const EVENT_INBOUND_RECEIVED: &str = "inbound_received";
pub const EVENT_VALIDATION_FAILED: &str = "validation_failed";
pub const EVENT_SESSION_QUEUED: &str = "session_queued";
pub const EVENT_TOOL_CALLED: &str = "tool_called";
pub const EVENT_TOOL_FAILED: &str = "tool_failed";
pub const EVENT_PROVIDER_FALLBACK: &str = "provider_fallback";
pub const EVENT_FINAL_RESPONSE_PUBLISHED: &str = "final_response_published";
pub const EVENT_MESSAGE_SENT_DLQ: &str = "message_sent_dlq";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_name: String,
    pub trace_id: Uuid,
    pub timestamp: String,
    pub session_key: Option<String>,
    pub error_code: Option<String>,
    pub payload: serde_json::Value,
}

impl AuditEvent {
    pub fn new(
        event_name: impl Into<String>,
        trace_id: Uuid,
        session_key: Option<String>,
        error_code: Option<String>,
        payload: serde_json::Value,
    ) -> Self {
        let timestamp = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string());
        Self {
            event_name: event_name.into(),
            trace_id,
            timestamp,
            session_key,
            error_code,
            payload,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

pub struct AuditLogger {
    output_path: Option<PathBuf>,
    buffer: RwLock<Vec<AuditEvent>>,
    max_buffer_size: usize,
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new(None, 1000)
    }
}

impl AuditLogger {
    pub fn new(output_path: Option<PathBuf>, max_buffer_size: usize) -> Self {
        Self {
            output_path,
            buffer: RwLock::new(Vec::new()),
            max_buffer_size,
        }
    }

    pub fn emit(&self, event: AuditEvent) {
        if let Some(path) = &self.output_path {
            if let Err(err) = self.write_to_file(path, &event) {
                tracing::warn!(error = %err, "failed to write audit event to file");
            }
        }

        let mut buffer = self
            .buffer
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if buffer.len() >= self.max_buffer_size {
            buffer.remove(0);
        }
        buffer.push(event);
    }

    fn write_to_file(&self, path: &PathBuf, event: &AuditEvent) -> std::io::Result<()> {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        let json = event.to_json()?;
        writeln!(file, "{}", json)?;
        Ok(())
    }

    pub fn drain_events(&self) -> Vec<AuditEvent> {
        let mut buffer = self
            .buffer
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *buffer)
    }

    pub fn recent_events(&self, limit: usize) -> Vec<AuditEvent> {
        let buffer = self
            .buffer
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        buffer.iter().rev().take(limit).cloned().collect()
    }

    pub fn event_count(&self) -> usize {
        let buffer = self
            .buffer
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audit_event_new_creates_valid_event() {
        let trace_id = Uuid::new_v4();
        let event = AuditEvent::new(
            EVENT_INBOUND_RECEIVED,
            trace_id,
            Some("session:123".to_string()),
            None,
            json!({"key": "value"}),
        );

        assert_eq!(event.event_name, EVENT_INBOUND_RECEIVED);
        assert_eq!(event.trace_id, trace_id);
        assert_eq!(event.session_key, Some("session:123".to_string()));
        assert!(event.timestamp.contains('T'));
    }

    #[test]
    fn audit_event_to_json_produces_valid_json() {
        let trace_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let event = AuditEvent::new(
            EVENT_TOOL_CALLED,
            trace_id,
            Some("session:abc".to_string()),
            None,
            json!({"tool": "shell"}),
        );

        let json = event.to_json().expect("should serialize");
        assert!(json.contains("tool_called"));
        assert!(json.contains("session:abc"));
    }

    #[test]
    fn audit_logger_buffer_stores_events() {
        let logger = AuditLogger::new(None, 100);
        let trace_id = Uuid::new_v4();

        logger.emit(AuditEvent::new(
            EVENT_INBOUND_RECEIVED,
            trace_id,
            None,
            None,
            json!({}),
        ));

        assert_eq!(logger.event_count(), 1);
    }

    #[test]
    fn audit_logger_buffer_respects_max_size() {
        let logger = AuditLogger::new(None, 3);

        for i in 0..5 {
            logger.emit(AuditEvent::new(
                EVENT_INBOUND_RECEIVED,
                Uuid::new_v4(),
                Some(format!("session:{}", i)),
                None,
                json!({}),
            ));
        }

        assert_eq!(logger.event_count(), 3);
        let events = logger.drain_events();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn audit_logger_recent_events_returns_newest_first() {
        let logger = AuditLogger::new(None, 100);

        for i in 0..5 {
            logger.emit(AuditEvent::new(
                EVENT_INBOUND_RECEIVED,
                Uuid::new_v4(),
                Some(format!("session:{}", i)),
                None,
                json!({}),
            ));
        }

        let recent = logger.recent_events(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].session_key, Some("session:4".to_string()));
        assert_eq!(recent[1].session_key, Some("session:3".to_string()));
    }
}
