use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tracing::{error, info, warn};


#[derive(Debug, Deserialize)]
struct Config {
    settings: Settings,
    services: Vec<ServiceConfig>,
}

#[derive(Debug, Deserialize)]
struct Settings {
    interval_secs: u64,
    timeout_secs: u64,
    concurrency: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct ServiceConfig {
    name: String,
    url: String,
}


#[derive(Debug, Serialize)]
struct CheckResult {
    name: String,
    url: String,
    timestamp: String,
    status: CheckStatus,
    latency_ms: Option<u64>,
    http_status: Option<u16>,
    error: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum CheckStatus {
    Up,
    Down,
}


async fn check_service(client: &Client, svc: &ServiceConfig, timeout: Duration) -> CheckResult {
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


async fn run_round(
    client: Arc<Client>,
    services: Arc<Vec<ServiceConfig>>,
    semaphore: Arc<Semaphore>,
    timeout: Duration,
) {
    let handles: Vec<_> = services
        .iter()
        .cloned()
        .map(|svc| {
            let client = client.clone();
            let sem = semaphore.clone();
            tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                check_service(&client, &svc, timeout).await
            })
        })
        .collect();

    for handle in handles {
        match handle.await {
            Ok(result) => log_result(&result),
            Err(e) => error!("Task panicked: {e}"),
        }
    }
}

fn log_result(r: &CheckResult) {
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


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config_str = std::fs::read_to_string("config.toml")
        .expect("config.toml not found in current directory");
    let config: Config = toml::from_str(&config_str).expect("Invalid config.toml");

    let timeout = Duration::from_secs(config.settings.timeout_secs);
    let interval = Duration::from_secs(config.settings.interval_secs);

    let client = Arc::new(Client::builder().timeout(timeout).build()?);
    let services = Arc::new(config.services);
    let semaphore = Arc::new(Semaphore::new(config.settings.concurrency));

    info!(
        services = services.len(),
        interval_secs = config.settings.interval_secs,
        concurrency = config.settings.concurrency,
        "Starting uptime monitor"
    );

    loop {
        run_round(client.clone(), services.clone(), semaphore.clone(), timeout).await;
        tokio::time::sleep(interval).await;
    }
}
