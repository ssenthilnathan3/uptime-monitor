mod checker;
mod db;
mod notifier;

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use sqlx::SqlitePool;
use tokio::sync::Semaphore;
use tracing::{error, info};

use checker::CheckStatus;

#[derive(Debug, Deserialize)]
struct Config {
    settings: Settings,
    slack: Option<SlackConfig>,
    services: Vec<ServiceConfig>,
}

#[derive(Debug, Deserialize)]
struct Settings {
    interval_secs: u64,
    timeout_secs: u64,
    concurrency: usize,
    #[serde(default = "default_db_path")]
    db_path: String,
}

fn default_db_path() -> String {
    "uptime.db".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SlackConfig {
    pub webhook_url: String,
}

async fn run_round(
    client: Arc<Client>,
    services: Arc<Vec<ServiceConfig>>,
    semaphore: Arc<Semaphore>,
    timeout: Duration,
    pool: Arc<SqlitePool>,
    slack: Option<Arc<SlackConfig>>,
) {
    let handles: Vec<_> = services
        .iter()
        .cloned()
        .map(|svc| {
            let client = client.clone();
            let sem = semaphore.clone();
            let pool = pool.clone();
            let slack = slack.clone();
            tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let result = checker::check_service(&client, &svc, timeout).await;
                checker::log_result(&result);

                // Persist full check history
                if let Err(e) = db::save_check(&pool, &result).await {
                    error!(name = %result.name, "Failed to save check: {e}");
                }

                // Alert only on state transitions (up→down or down→up)
                let prev = db::get_previous_status(&pool, &result.name)
                    .await
                    .unwrap_or(None);
                let current = result.status.as_str();

                let should_alert = match &prev {
                    None => result.status == CheckStatus::Down,
                    Some(p) => p.as_str() != current,
                };

                if should_alert {
                    if let Some(ref slack) = slack {
                        let msg = notifier::format_alert(&result);
                        if let Err(e) =
                            notifier::send_slack_alert(&client, &slack.webhook_url, &msg).await
                        {
                            error!("Slack alert failed: {e}");
                        }
                    }
                }

                // Update state on first check or transition
                if prev.as_deref() != Some(current) {
                    if let Err(e) = db::upsert_service_state(&pool, &result).await {
                        error!(name = %result.name, "Failed to update service state: {e}");
                    }
                }
            })
        })
        .collect();

    for handle in handles {
        if let Err(e) = handle.await {
            error!("Task panicked: {e}");
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config_str =
        std::fs::read_to_string("config.toml").expect("config.toml not found in current directory");
    let config: Config = toml::from_str(&config_str).expect("Invalid config.toml");

    let timeout = Duration::from_secs(config.settings.timeout_secs);
    let interval = Duration::from_secs(config.settings.interval_secs);

    let pool = Arc::new(db::init(&config.settings.db_path).await?);
    let client = Arc::new(Client::builder().timeout(timeout).build()?);
    let semaphore = Arc::new(Semaphore::new(config.settings.concurrency));
    let services = Arc::new(config.services);
    let slack = config.slack.map(Arc::new);

    info!(
        services = services.len(),
        interval_secs = config.settings.interval_secs,
        concurrency = config.settings.concurrency,
        db = %config.settings.db_path,
        slack = slack.is_some(),
        "Starting uptime monitor"
    );

    loop {
        run_round(
            client.clone(),
            services.clone(),
            semaphore.clone(),
            timeout,
            pool.clone(),
            slack.clone(),
        )
        .await;
        tokio::time::sleep(interval).await;
    }
}
