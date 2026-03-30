use anyhow::Result;
use reqwest::Client;

use crate::checker::{CheckResult, CheckStatus};

pub fn format_alert(r: &CheckResult) -> String {
    match r.status {
        CheckStatus::Up => format!(
            ":white_check_mark: *{}* is back UP `{}`",
            r.name, r.url
        ),
        CheckStatus::Down => format!(
            ":red_circle: *{}* is DOWN — {} `{}`",
            r.name,
            r.error.as_deref().unwrap_or("no response"),
            r.url
        ),
    }
}

pub async fn send_slack_alert(client: &Client, webhook_url: &str, text: &str) -> Result<()> {
    let payload = serde_json::json!({ "text": text });
    let resp = client.post(webhook_url).json(&payload).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Slack webhook returned {}", resp.status());
    }
    Ok(())
}
