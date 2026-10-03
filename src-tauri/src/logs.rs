use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::pool::SshPool;
use crate::ssh::{ExecEvent, SshTarget};

#[derive(Debug, Default, Deserialize)]
pub struct LogOptions {
    pub lines: Option<u32>,
    /// docker --since value, e.g. "10m", "2h", "2026-09-25T10:00:00".
    pub since: Option<String>,
    pub grep: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogEvent {
    Line { text: String },
    End { code: Option<u32> },
}

/// POSIX single-quote escaping for the remote shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'\''"#))
}

fn valid_token(s: &str, extra: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

pub fn logs_command(container: &str, opts: &LogOptions) -> Result<String> {
    if !valid_token(container, "-_.") {
        bail!("invalid container name: {container:?}");
    }
    let mut cmd = format!("docker logs -f -t --tail {} ", opts.lines.unwrap_or(500));
    if let Some(since) = opts.since.as_deref().filter(|s| !s.is_empty()) {
        if !valid_token(since, "-:.+TZ") {
            bail!("invalid --since value: {since:?}");
        }
        cmd += &format!("--since {} ", shell_quote(since));
    }
    cmd += &format!("{container} 2>&1");
    if let Some(grep) = opts.grep.as_deref().filter(|g| !g.is_empty()) {
        cmd += &format!(" | grep --line-buffered -e {}", shell_quote(grep));
    }
    Ok(cmd)
}

/// Active followers (container logs, proxy requests); each is one exec channel on the host's pooled
/// connection. Aborting the forwarder drops the receiver, which closes the
/// channel and ends the remote `docker logs -f`.
#[derive(Default)]
pub struct LogStreams {
    next_id: AtomicU64,
    subs: Arc<Mutex<HashMap<u64, JoinHandle<()>>>>,
}

impl LogStreams {
    pub async fn subscribe(
        &self,
        pool: &SshPool,
        target: &SshTarget,
        container: &str,
        opts: &LogOptions,
        emit: impl Fn(LogEvent) + Send + Sync + 'static,
    ) -> Result<u64> {
        let cmd = logs_command(container, opts)?;
        let emit = Arc::new(emit);
        let on_end = emit.clone();
        self.follow(pool, target, &cmd, move |text| emit(LogEvent::Line { text }), move |code| on_end(LogEvent::End { code }))
            .await
    }

    /// Runs a long-lived remote command, calling `on_line` per output line
    /// (stdout and stderr) and `on_end` once when it exits.
    pub async fn follow(
        &self,
        pool: &SshPool,
        target: &SshTarget,
        cmd: &str,
        on_line: impl Fn(String) + Send + 'static,
        on_end: impl FnOnce(Option<u32>) + Send + 'static,
    ) -> Result<u64> {
        let mut rx = match pool.get(target).await?.exec(cmd).await {
            Ok(rx) => rx,
            Err(e) => {
                pool.evict(target).await;
                return Err(e);
            }
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let subs = self.subs.clone();
        let task = tokio::spawn(async move {
            let mut code = None;
            while let Some(event) = rx.recv().await {
                match event {
                    ExecEvent::Stdout(text) | ExecEvent::Stderr(text) => on_line(text),
                    ExecEvent::Exit(c) => code = Some(c),
                }
            }
            on_end(code);
            subs.lock().unwrap().remove(&id);
        });
        self.subs.lock().unwrap().insert(id, task);
        Ok(id)
    }

    pub fn unsubscribe(&self, id: u64) {
        if let Some(task) = self.subs.lock().unwrap().remove(&id) {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_logs_command() {
        let opts = LogOptions { lines: Some(100), since: Some("10m".into()), grep: Some("it's 500".into()) };
        assert_eq!(
            logs_command("app-web-abc", &opts).unwrap(),
            r#"docker logs -f -t --tail 100 --since '10m' app-web-abc 2>&1 | grep --line-buffered -e 'it'\''s 500'"#
        );
        assert_eq!(logs_command("app-web", &LogOptions::default()).unwrap(), "docker logs -f -t --tail 500 app-web 2>&1");
    }

    #[test]
    fn rejects_unsafe_input() {
        assert!(logs_command("app; rm -rf /", &LogOptions::default()).is_err());
        let since = LogOptions { since: Some("1m; reboot".into()), ..Default::default() };
        assert!(logs_command("app", &since).is_err());
    }
}
