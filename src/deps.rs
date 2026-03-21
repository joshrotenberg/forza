//! Dependency validation — checks that required external tools are available on startup.

use tokio::process::Command;

use crate::error::{Error, Result};

/// Validate that all required external tools are present and usable.
///
/// Checks `git`, `gh` (including auth), and the configured agent binary in order.
/// Returns the first error encountered.
pub async fn validate_dependencies(agent: &str) -> Result<()> {
    check_git().await?;
    check_gh().await?;
    check_agent(agent).await?;
    Ok(())
}

async fn check_git() -> Result<()> {
    let status = Command::new("git")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(Error::Dependency(
            "git --version failed; install git from https://git-scm.com/downloads".into(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::Dependency(
            "git not found; install git from https://git-scm.com/downloads".into(),
        )),
        Err(e) => Err(Error::Dependency(format!("git check failed: {e}"))),
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
