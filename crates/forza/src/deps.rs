//! Dependency validation — checks that required external tools are available on startup.

use tokio::process::Command;

use crate::error::{Error, Result};
use crate::git::GitClient;

/// Validate that all required external tools are present and usable.
///
/// Checks `git`, `gh` (including auth), and the configured agent binary in order.
/// Returns the first error encountered.
pub async fn validate_dependencies(agent: &str, git: &dyn GitClient) -> Result<()> {
    check_git(git).await?;
    check_gh().await?;
    check_agent(agent).await?;
    Ok(())
}

async fn check_git(git: &dyn GitClient) -> Result<()> {
    match git.version().await {
        Ok(_) => Ok(()),
        Err(_) => Err(Error::Dependency(
            "git not available; install git from https://git-scm.com/downloads".into(),
        )),
    }
}

async fn check_gh() -> Result<()> {
    let status = Command::new("gh")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    match status {
        Ok(s) if s.success() => {}
        Ok(_) => {
            return Err(Error::Dependency(
                "gh --version failed; install gh from https://cli.github.com".into(),
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::Dependency(
                "gh not found; install gh from https://cli.github.com".into(),
            ));
        }
        Err(e) => {
            return Err(Error::Dependency(format!("gh check failed: {e}")));
        }
    }

    let auth = Command::new("gh")
        .args(["auth", "status"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    match auth {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(Error::Dependency(
            "gh is not authenticated; run `gh auth login` to authenticate".into(),
        )),
        Err(e) => Err(Error::Dependency(format!("gh auth status failed: {e}"))),
    }
}

async fn check_agent(agent: &str) -> Result<()> {
    let status = Command::new(agent)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(Error::Dependency(format!(
            "{agent} --version failed; {}",
            install_hint(agent)
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::Dependency(format!(
            "{agent} not found; {}",
            install_hint(agent)
        ))),
        Err(e) => Err(Error::Dependency(format!("{agent} check failed: {e}"))),
    }
}

fn install_hint(agent: &str) -> &'static str {
    match agent {
        "claude" => "install claude from https://docs.anthropic.com/en/docs/claude-code",
        "codex" => "install codex from https://openai.com/index/openai-codex",
        _ => "ensure the agent binary is installed and on your PATH",
    }
}

// ---------------------------------------------------------------------------
// Doctor checks — structured results for `forza doctor`
// ---------------------------------------------------------------------------

/// Result of a single doctor check.
pub struct CheckResult {
    /// Display name for the check (left column).
    pub name: &'static str,
    /// Detail string shown in the middle column.
    pub detail: String,
    /// Whether the check passed.
    pub ok: bool,
}

/// Run all doctor checks and return individual results.
///
/// Config-dependent checks (repo access, labels) are skipped when `repo` is `None`.
pub async fn doctor_checks(
    agent: &str,
    git: &dyn GitClient,
    repo: Option<&str>,
) -> Vec<CheckResult> {
    let mut results = Vec::new();

    results.push(doctor_agent(agent).await);
    results.push(doctor_gh_cli().await);
    results.push(doctor_gh_auth().await);
    results.push(doctor_git(git).await);
    results.push(doctor_api_key(agent));
    results.push(doctor_config());

    if let Some(repo) = repo {
        results.push(doctor_repo_access(repo).await);
        results.push(doctor_labels(repo).await);
    }

    results
}

async fn doctor_agent(agent: &str) -> CheckResult {
    let output = Command::new(agent)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let version = String::from_utf8_lossy(&o.stdout)
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            CheckResult {
                name: "Agent",
                detail: if version.is_empty() {
                    agent.to_string()
                } else {
                    version
                },
                ok: true,
            }
        }
        Ok(_) => CheckResult {
            name: "Agent",
            detail: format!("{agent} --version failed"),
            ok: false,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CheckResult {
            name: "Agent",
            detail: format!("{agent} not found"),
            ok: false,
        },
        Err(e) => CheckResult {
            name: "Agent",
            detail: format!("{agent} error: {e}"),
            ok: false,
        },
    }
}

async fn doctor_gh_cli() -> CheckResult {
    let output = Command::new("gh")
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let version = String::from_utf8_lossy(&o.stdout)
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            CheckResult {
                name: "gh CLI",
                detail: if version.is_empty() {
                    "gh".to_string()
                } else {
                    version
                },
                ok: true,
            }
        }
        Ok(_) => CheckResult {
            name: "gh CLI",
            detail: "gh --version failed".to_string(),
            ok: false,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CheckResult {
            name: "gh CLI",
            detail: "gh not found".to_string(),
            ok: false,
        },
        Err(e) => CheckResult {
            name: "gh CLI",
            detail: format!("gh error: {e}"),
            ok: false,
        },
    }
}

async fn doctor_gh_auth() -> CheckResult {
    let output = Command::new("gh")
        .args(["auth", "status"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            // Try to extract the username from gh auth status output.
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let username = combined
                .lines()
                .find_map(|line| {
                    let line = line.trim();
                    if line.starts_with("Logged in to") {
                        line.split("account").nth(1).and_then(|s| {
                            s.split_whitespace()
                                .next()
                                .map(|u| u.trim_end_matches(')').to_string())
                        })
                    } else if line.contains("account") && line.contains("github.com") {
                        // Newer gh format: "account joshrotenberg (github.com/...)"
                        line.split("account")
                            .nth(1)
                            .and_then(|s| s.split_whitespace().next().map(|u| u.to_string()))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "authenticated".to_string());
            CheckResult {
                name: "gh auth",
                detail: username,
                ok: true,
            }
        }
        Ok(_) => CheckResult {
            name: "gh auth",
            detail: "not authenticated".to_string(),
            ok: false,
        },
        Err(_) => CheckResult {
            name: "gh auth",
            detail: "gh not available".to_string(),
            ok: false,
        },
    }
}

async fn doctor_git(git: &dyn GitClient) -> CheckResult {
    match git.version().await {
        Ok(version) => CheckResult {
            name: "git",
            detail: version.trim().to_string(),
            ok: true,
        },
        Err(_) => CheckResult {
            name: "git",
            detail: "not available".to_string(),
            ok: false,
        },
    }
}

fn doctor_api_key(agent: &str) -> CheckResult {
    match agent {
        "claude" => {
            if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                CheckResult {
                    name: "API key",
                    detail: "ANTHROPIC_API_KEY set".to_string(),
                    ok: true,
                }
            } else {
                CheckResult {
                    name: "API key",
                    detail: "ANTHROPIC_API_KEY not set".to_string(),
                    ok: false,
                }
            }
        }
        "codex" => {
            if std::env::var("OPENAI_API_KEY").is_ok() {
                CheckResult {
                    name: "API key",
                    detail: "OPENAI_API_KEY set".to_string(),
                    ok: true,
                }
            } else {
                CheckResult {
                    name: "API key",
                    detail: "OPENAI_API_KEY not set".to_string(),
                    ok: false,
                }
            }
        }
        _ => CheckResult {
            name: "API key",
            detail: "unknown agent; skipped".to_string(),
            ok: true,
        },
    }
}

fn doctor_config() -> CheckResult {
    let path = std::path::Path::new("forza.toml");
    if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(contents) => match toml::from_str::<toml::Value>(&contents) {
                Ok(_) => CheckResult {
                    name: "Config",
                    detail: "forza.toml".to_string(),
                    ok: true,
                },
                Err(e) => CheckResult {
                    name: "Config",
                    detail: format!("parse error: {e}"),
                    ok: false,
                },
            },
            Err(e) => CheckResult {
                name: "Config",
                detail: format!("read error: {e}"),
                ok: false,
            },
        }
    } else {
        CheckResult {
            name: "Config",
            detail: "forza.toml not found".to_string(),
            ok: false,
        }
    }
}

async fn doctor_repo_access(repo: &str) -> CheckResult {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{repo}"), "--jq", ".full_name"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            CheckResult {
                name: "Repo",
                detail: if name.is_empty() {
                    repo.to_string()
                } else {
                    name
                },
                ok: true,
            }
        }
        Ok(_) => CheckResult {
            name: "Repo",
            detail: format!("{repo} not accessible"),
            ok: false,
        },
        Err(_) => CheckResult {
            name: "Repo",
            detail: "gh not available".to_string(),
            ok: false,
        },
    }
}

async fn doctor_labels(repo: &str) -> CheckResult {
    let required = [
        "forza:ready",
        "forza:in-progress",
        "forza:complete",
        "forza:failed",
        "forza:blocked",
        "forza:plan",
    ];

    let output = Command::new("gh")
        .args([
            "label", "list", "--repo", repo, "--limit", "100", "--json", "name",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            let labels: Vec<String> = serde_json::from_str::<Vec<serde_json::Value>>(&body)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| v.get("name")?.as_str().map(String::from))
                .collect();
            let present = required
                .iter()
                .filter(|r| labels.contains(&(**r).to_string()))
                .count();
            let total = required.len();
            if present == total {
                CheckResult {
                    name: "Labels",
                    detail: format!("{present}/{total} present"),
                    ok: true,
                }
            } else {
                let missing: Vec<&str> = required
                    .iter()
                    .filter(|r| !labels.contains(&(**r).to_string()))
                    .copied()
                    .collect();
                CheckResult {
                    name: "Labels",
                    detail: format!(
                        "{present}/{total} present (missing: {})",
                        missing.join(", ")
                    ),
                    ok: false,
                }
            }
        }
        Ok(_) => CheckResult {
            name: "Labels",
            detail: "could not list labels".to_string(),
            ok: false,
        },
        Err(_) => CheckResult {
            name: "Labels",
            detail: "gh not available".to_string(),
            ok: false,
        },
    }
}
