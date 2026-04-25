use crate::StorageError;
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq)]
pub enum DbValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbRow {
    pub values: Vec<DbValue>,
}

impl DbRow {
    pub fn get(&self, index: usize) -> Option<&DbValue> {
        self.values.get(index)
    }
}

#[async_trait]
pub trait DatabaseExecutor: Send + Sync {
    async fn execute_batch(&self, sql: &str) -> Result<(), StorageError>;
    async fn execute(&self, sql: &str, params: &[DbValue]) -> Result<u64, StorageError>;
    async fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, StorageError>;
}
