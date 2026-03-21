//! Notification channels fired on run completion.

use serde_json::json;
use tracing::warn;

use crate::config::NotificationsConfig;
use crate::state::{RunRecord, RunStatus};

/// Fire all configured notification channels for a completed run.
///
/// Errors are logged as warnings and never propagate — notifications are
/// best-effort and must never abort a run.
pub async fn notify_run_complete(config: &NotificationsConfig, record: &RunRecord) {
    if config.desktop {
        notify_desktop(record).await;
    }

    if let Some(url) = config.slack_webhook.as_deref() {
        notify_slack(url, record).await;
    }

    if let Some(url) = config.webhook_url.as_deref() {
        notify_webhook(url, record).await;
    }
}

fn status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::Canceled => "canceled",
        RunStatus::Abandoned => "abandoned",
        _ => "unknown",
    }
}

async fn notify_desktop(record: &RunRecord) {
    let title = format!(
        "forza: {} #{}",
        status_label(record.status),
        record.issue_number
    );
    let body = format!("{} — {}", record.repo, record.workflow);

    #[cfg(target_os = "macos")]
    {
        let script = format!("display notification {:?} with title {:?}", body, title);
        if let Err(e) = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await
        {
            warn!(error = %e, "desktop notification failed (osascript)");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Err(e) = tokio::process::Command::new("notify-send")
            .args([&title, &body])
            .output()
            .await
        {
            warn!(error = %e, "desktop notification failed (notify-send)");
        }
    }
}

async fn notify_slack(url: &str, record: &RunRecord) {
    let status = status_label(record.status);
    let cost = record
        .total_cost_usd
        .map(|c| format!(" | cost: ${c:.4}"))
        .unwrap_or_default();
    let text = format!(
        "forza run {} — {} #{} *{}*{cost}",
        status, record.repo, record.issue_number, record.workflow
    );

    let payload = json!({
        "text": text,
        "blocks": [
            {
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": text,
                }
            }
        ]
    });

    let client = reqwest::Client::new();
    if let Err(e) = client.post(url).json(&payload).send().await {
        warn!(error = %e, "slack notification failed");
    }
}

async fn notify_webhook(url: &str, record: &RunRecord) {
    let payload = json!({
        "run_id": record.run_id,
        "repo": record.repo,
        "issue_number": record.issue_number,
        "status": status_label(record.status),
        "workflow": record.workflow,
        "branch": record.branch,
        "pr_number": record.pr_number,
        "total_cost_usd": record.total_cost_usd,
        "started_at": record.started_at,
        "completed_at": record.completed_at,
    });

    let client = reqwest::Client::new();
    if let Err(e) = client.post(url).json(&payload).send().await {
        warn!(error = %e, "webhook notification failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationsConfig;

    fn make_record(status: RunStatus) -> RunRecord {
        RunRecord {
            run_id: "test-run-1".into(),
            repo: "owner/repo".into(),
            issue_number: 42,
            status,
            workflow: "feature".into(),
            branch: "automation/42-test".into(),
            pr_number: Some(99),
            stages: vec![],
            started_at: chrono::Utc::now(),
            completed_at: Some(chrono::Utc::now()),
            total_cost_usd: Some(0.12),
            subject_kind: crate::state::SubjectKind::Issue,
        }
    }

    #[test]
    fn status_label_mapping() {
        assert_eq!(status_label(RunStatus::Succeeded), "succeeded");
        assert_eq!(status_label(RunStatus::Failed), "failed");
        assert_eq!(status_label(RunStatus::Canceled), "canceled");
        assert_eq!(status_label(RunStatus::Abandoned), "abandoned");
    }

    #[test]
    fn notifications_config_defaults() {
        let config: crate::config::RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[global.notifications]
slack_webhook = "https://hooks.slack.com/services/xxx"
"#,
        )
        .unwrap();

        let notif = config.global.notifications.as_ref().unwrap();
        assert!(!notif.desktop);
        assert_eq!(
            notif.slack_webhook.as_deref(),
            Some("https://hooks.slack.com/services/xxx")
        );
        assert!(notif.webhook_url.is_none());
    }

    #[test]
    fn notifications_config_all_fields() {
        let config: crate::config::RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"

[global.notifications]
desktop = true
slack_webhook = "https://hooks.slack.com/services/xxx"
webhook_url = "https://example.com/hook"
"#,
        )
        .unwrap();

        let notif = config.global.notifications.as_ref().unwrap();
        assert!(notif.desktop);
        assert!(notif.slack_webhook.is_some());
        assert!(notif.webhook_url.is_some());
    }

    #[test]
    fn notifications_absent_when_section_missing() {
        let config: crate::config::RunnerConfig = toml::from_str(
            r#"
[global]
repo = "owner/repo"
"#,
        )
        .unwrap();
        assert!(config.global.notifications.is_none());
    }

    #[tokio::test]
    async fn notify_run_complete_noop_when_no_channels() {
        let config = NotificationsConfig {
            desktop: false,
            slack_webhook: None,
            webhook_url: None,
        };
        let record = make_record(RunStatus::Succeeded);
        // Should complete without panicking.
        notify_run_complete(&config, &record).await;
    }
}
