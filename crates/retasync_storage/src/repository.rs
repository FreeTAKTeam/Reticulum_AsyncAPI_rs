use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

const SCHEMA_SQL: &str = include_str!("sql/schema.sql");

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub sqlite_path: String,
}

#[derive(Debug, Clone)]
pub struct RetasyncStorage {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct JobRecord {
    pub job_id: String,
    pub operation: String,
    pub status: String,
    pub payload_json: String,
    pub submitted_at: String,
    pub updated_at: String,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct JobResultRecord {
    pub job_id: String,
    pub result_json: String,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TransferRecord {
    pub transfer_id: String,
    pub status: String,
    pub metadata_json: String,
    pub submitted_at: String,
    pub updated_at: String,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct NodeConfigRevision {
    pub revision_id: i64,
    pub config_json: String,
    pub created_at: String,
}

impl RetasyncStorage {
    pub async fn connect(config: &StorageConfig) -> Result<Self> {
        let uri = normalize_sqlite_uri(&config.sqlite_path);
        let options = SqliteConnectOptions::from_str(&uri)
            .with_context(|| format!("invalid sqlite URI: {}", uri))?
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("failed to connect sqlite pool")?;

        let storage = Self { pool };
        storage.migrate().await?;
        Ok(storage)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<()> {
        for statement in SCHEMA_SQL.split(';') {
            let sql = statement.trim();
            if sql.is_empty() {
                continue;
            }
            sqlx::query(sql)
                .execute(&self.pool)
                .await
                .with_context(|| format!("migration failed for statement: {sql}"))?;
        }
        info!("retasync sqlite schema ready");
        Ok(())
    }

    pub async fn create_job(&self, operation: &str, payload: Value) -> Result<JobRecord> {
        let now = Utc::now().to_rfc3339();
        let job_id = Uuid::now_v7().to_string();
        let payload_json = serde_json::to_string(&payload).context("serialize job payload")?;

        sqlx::query(
            "INSERT INTO jobs(job_id, operation, status, payload_json, submitted_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&job_id)
        .bind(operation)
        .bind("queued")
        .bind(&payload_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .context("insert job")?;

        self.get_job(&job_id)
            .await?
            .context("job missing after insert")
    }

    pub async fn update_job_status(
        &self,
        job_id: &str,
        status: &str,
        failure_reason: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE jobs SET status = ?, updated_at = ?, failure_reason = ? WHERE job_id = ?")
            .bind(status)
            .bind(now)
            .bind(failure_reason)
            .bind(job_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("update job status for {job_id}"))?;

        Ok(())
    }

    pub async fn insert_job_result(&self, job_id: &str, result: Value) -> Result<()> {
        let completed_at = Utc::now().to_rfc3339();
        let result_json = serde_json::to_string(&result).context("serialize job result")?;

        sqlx::query(
            "INSERT INTO job_results(job_id, result_json, completed_at) VALUES (?, ?, ?) ON CONFLICT(job_id) DO UPDATE SET result_json = excluded.result_json, completed_at = excluded.completed_at",
        )
        .bind(job_id)
        .bind(result_json)
        .bind(completed_at)
        .execute(&self.pool)
        .await
        .with_context(|| format!("insert job result for {job_id}"))?;

        Ok(())
    }

    pub async fn get_job(&self, job_id: &str) -> Result<Option<JobRecord>> {
        sqlx::query_as::<_, JobRecord>(
            "SELECT job_id, operation, status, payload_json, submitted_at, updated_at, failure_reason FROM jobs WHERE job_id = ?",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("query job {job_id}"))
    }

    pub async fn get_job_result(&self, job_id: &str) -> Result<Option<JobResultRecord>> {
        sqlx::query_as::<_, JobResultRecord>(
            "SELECT job_id, result_json, completed_at FROM job_results WHERE job_id = ?",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("query job result {job_id}"))
    }

    pub async fn list_cached_events(&self, limit: i64) -> Result<Vec<Value>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM cached_events ORDER BY received_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("query cached events")?;

        rows.into_iter()
            .map(|row| serde_json::from_str::<Value>(&row).context("parse cached event payload"))
            .collect()
    }

    pub async fn list_cached_messages(&self, limit: i64) -> Result<Vec<Value>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM cached_messages ORDER BY received_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("query cached messages")?;

        rows.into_iter()
            .map(|row| serde_json::from_str::<Value>(&row).context("parse cached message payload"))
            .collect()
    }

    pub async fn list_allowlist(&self) -> Result<Vec<String>> {
        sqlx::query_scalar::<_, String>(
            "SELECT identity_hash FROM acl_allowlist ORDER BY identity_hash ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("query allowlist")
    }

    pub async fn add_allowlist(&self, identity_hash: &str, note: Option<&str>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO acl_allowlist(identity_hash, note, created_at) VALUES (?, ?, ?) ON CONFLICT(identity_hash) DO UPDATE SET note = excluded.note",
        )
        .bind(identity_hash)
        .bind(note)
        .bind(now)
        .execute(&self.pool)
        .await
        .with_context(|| format!("insert allowlist identity {identity_hash}"))?;
        Ok(())
    }

    pub async fn delete_allowlist(&self, identity_hash: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM acl_allowlist WHERE identity_hash = ?")
            .bind(identity_hash)
            .execute(&self.pool)
            .await
            .with_context(|| format!("delete allowlist identity {identity_hash}"))?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn append_node_config_revision(&self, config_json: &str) -> Result<NodeConfigRevision> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO node_config_revisions(config_json, created_at) VALUES (?, ?)")
            .bind(config_json)
            .bind(&now)
            .execute(&self.pool)
            .await
            .context("insert node config revision")?;

        sqlx::query_as::<_, NodeConfigRevision>(
            "SELECT revision_id, config_json, created_at FROM node_config_revisions ORDER BY revision_id DESC LIMIT 1",
        )
        .fetch_one(&self.pool)
        .await
        .context("query latest node config revision")
    }

    pub async fn get_transfer(&self, transfer_id: &str) -> Result<Option<TransferRecord>> {
        sqlx::query_as::<_, TransferRecord>(
            "SELECT transfer_id, status, metadata_json, submitted_at, updated_at, failure_reason FROM transfers WHERE transfer_id = ?",
        )
        .bind(transfer_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("query transfer {transfer_id}"))
    }

    pub async fn create_transfer(&self, metadata: Value) -> Result<TransferRecord> {
        let transfer_id = Uuid::now_v7().to_string();
        let now = Utc::now().to_rfc3339();
        let metadata_json = serde_json::to_string(&metadata).context("serialize transfer metadata")?;

        sqlx::query(
            "INSERT INTO transfers(transfer_id, status, metadata_json, submitted_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&transfer_id)
        .bind("queued")
        .bind(&metadata_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .context("insert transfer")?;

        self.get_transfer(&transfer_id)
            .await?
            .context("transfer missing after insert")
    }

    pub async fn update_transfer_status(
        &self,
        transfer_id: &str,
        status: &str,
        failure_reason: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE transfers SET status = ?, updated_at = ?, failure_reason = ? WHERE transfer_id = ?")
            .bind(status)
            .bind(now)
            .bind(failure_reason)
            .bind(transfer_id)
            .execute(&self.pool)
            .await
        .with_context(|| format!("update transfer {transfer_id}"))?;
        Ok(())
    }

    pub async fn purge_expired(
        &self,
        job_retention_hours: i64,
        cache_retention_hours: i64,
        transfer_retention_days: i64,
    ) -> Result<()> {
        sqlx::query(
            "DELETE FROM job_results WHERE completed_at < datetime('now', '-' || ? || ' hours')",
        )
        .bind(job_retention_hours)
        .execute(&self.pool)
        .await
        .context("purge expired job_results")?;

        sqlx::query(
            "DELETE FROM jobs WHERE updated_at < datetime('now', '-' || ? || ' hours')",
        )
        .bind(job_retention_hours)
        .execute(&self.pool)
        .await
        .context("purge expired jobs")?;

        sqlx::query(
            "DELETE FROM cached_events WHERE received_at < datetime('now', '-' || ? || ' hours')",
        )
        .bind(cache_retention_hours)
        .execute(&self.pool)
        .await
        .context("purge expired cached_events")?;

        sqlx::query(
            "DELETE FROM cached_messages WHERE received_at < datetime('now', '-' || ? || ' hours')",
        )
        .bind(cache_retention_hours)
        .execute(&self.pool)
        .await
        .context("purge expired cached_messages")?;

        sqlx::query(
            "DELETE FROM transfers WHERE updated_at < datetime('now', '-' || ? || ' days')",
        )
        .bind(transfer_retention_days)
        .execute(&self.pool)
        .await
        .context("purge expired transfers")?;

        Ok(())
    }
}

fn normalize_sqlite_uri(raw: &str) -> String {
    if raw.starts_with("sqlite:") {
        raw.to_string()
    } else {
        format!("sqlite://{raw}")
    }
}
