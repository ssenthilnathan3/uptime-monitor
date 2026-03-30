use anyhow::Result;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::str::FromStr;

use crate::checker::CheckResult;

pub async fn init(path: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{path}"))?
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS checks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT    NOT NULL,
            url         TEXT    NOT NULL,
            timestamp   TEXT    NOT NULL,
            status      TEXT    NOT NULL,
            latency_ms  INTEGER,
            http_status INTEGER,
            error       TEXT
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS service_state (
            name       TEXT PRIMARY KEY,
            status     TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub async fn save_check(pool: &SqlitePool, r: &CheckResult) -> Result<()> {
    sqlx::query(
        "INSERT INTO checks (name, url, timestamp, status, latency_ms, http_status, error)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&r.name)
    .bind(&r.url)
    .bind(&r.timestamp)
    .bind(r.status.as_str())
    .bind(r.latency_ms.map(|v| v as i64))
    .bind(r.http_status.map(|v| v as i64))
    .bind(&r.error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_previous_status(pool: &SqlitePool, name: &str) -> Result<Option<String>> {
    let row = sqlx::query_scalar::<_, String>(
        "SELECT status FROM service_state WHERE name = ?",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn upsert_service_state(
    pool: &SqlitePool,
    result: &CheckResult,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO service_state (name, status, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(name) DO UPDATE SET status = excluded.status, updated_at = excluded.updated_at",
    )
    .bind(&result.name)
    .bind(result.status.as_str())
    .bind(&result.timestamp)
    .execute(pool)
    .await?;
    Ok(())
}
