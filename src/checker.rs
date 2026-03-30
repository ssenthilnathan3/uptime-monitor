use std::time::{Duration, Instant};

use chrono::Utc;
use reqwest::Client;
use serde::Serialize;
use tracing::{info, warn};

use crate::ServiceConfig;

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub url: String,
    pub timestamp: String,
    pub status: CheckStatus,
    pub latency_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Up,
    Down,
}

impl CheckStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckStatus::Up => "up",
            CheckStatus::Down => "down",
        }
    }
}

pub async fn check_service(client: &Client, svc: &ServiceConfig, timeout: Duration) -> CheckResult {
    let start = Instant::now();
    let timestamp = Utc::now().to_rfc3339();

    let result = client.get(&svc.url).timeout(timeout).send().await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => {
            let http_status = resp.status().as_u16();
            if resp.status().is_success() {
                CheckResult {
                    name: svc.name.clone(),
                    url: svc.url.clone(),
                    timestamp,
                    status: CheckStatus::Up,
                    latency_ms: Some(latency_ms),
                    http_status: Some(http_status),
                    error: None,
                }
            } else {
                CheckResult {
                    name: svc.name.clone(),
                    url: svc.url.clone(),
                    timestamp,
                    status: CheckStatus::Down,
                    latency_ms: Some(latency_ms),
                    http_status: Some(http_status),
                    error: Some(format!("HTTP {http_status}")),
                }
            }
        }
        Err(e) => CheckResult {
            name: svc.name.clone(),
            url: svc.url.clone(),
            timestamp,
            status: CheckStatus::Down,
            latency_ms: None,
            http_status: None,
            error: Some(e.to_string()),
        },
    }
}

pub fn log_result(r: &CheckResult) {
    match r.status {
        CheckStatus::Up => info!(
            name = %r.name,
            latency_ms = ?r.latency_ms,
            http_status = ?r.http_status,
            "UP"
        ),
        CheckStatus::Down => warn!(
            name = %r.name,
            error = ?r.error,
            http_status = ?r.http_status,
            "DOWN"
        ),
    }
}
