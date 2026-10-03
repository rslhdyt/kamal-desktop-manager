use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use russh::client::{self, Handle};
use russh::keys::agent::client::AgentClient;
use russh::keys::{check_known_hosts, load_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::ChannelMsg;
use serde::Deserialize;
use tokio::sync::mpsc;

#[derive(Clone, Debug, Deserialize)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
}

#[derive(Debug)]
pub enum ExecEvent {
    Stdout(String),
    Stderr(String),
    Exit(u32),
}

pub struct ExecOutput {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub exit: Option<u32>,
}

/// Runs remote commands. Kept as a trait so a system-`ssh` ControlMaster
/// fallback can be added for setups russh can't handle (ProxyJump, 2FA, …).
#[async_trait]
pub trait SshTransport: Send + Sync {
    /// Runs `cmd` and streams its output line by line. `cmd` must already be
    /// safe for the remote shell — never pass unescaped user input.
    async fn exec(&self, cmd: &str) -> Result<mpsc::Receiver<ExecEvent>>;

    async fn exec_collect(&self, cmd: &str) -> Result<ExecOutput> {
        let mut rx = self.exec(cmd).await?;
        let mut out = ExecOutput { stdout: vec![], stderr: vec![], exit: None };
        while let Some(event) = rx.recv().await {
            match event {
                ExecEvent::Stdout(line) => out.stdout.push(line),
                ExecEvent::Stderr(line) => out.stderr.push(line),
                ExecEvent::Exit(code) => out.exit = Some(code),
            }
        }
        Ok(out)
    }
}

struct KnownHostsCheck {
    host: String,
    port: u16,
}

impl client::Handler for KnownHostsCheck {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        match key {
            // Unknown host returns false (connection refused), a changed key
            // returns KeyChanged. Never auto-accept.
            PublicKeyOrCertificate::PublicKey { key, .. } => {
                Ok(check_known_hosts(&self.host, self.port, key)?)
            }
            PublicKeyOrCertificate::Certificate(_) => Ok(false),
        }
    }
}

pub struct RusshTransport {
    handle: Arc<Handle<KnownHostsCheck>>,
}

impl RusshTransport {
    pub async fn connect(target: &SshTarget) -> Result<Self> {
        let config = Arc::new(client::Config {
            // Pooled connections sit idle between polls; keepalives detect dead peers.
            inactivity_timeout: None,
            keepalive_interval: Some(Duration::from_secs(15)),
            keepalive_max: 3,
            ..Default::default()
        });
        let resolved = resolve_ssh_config(target).await;
        let host = resolved.hostname.as_deref().unwrap_or(&target.host);
        let check = KnownHostsCheck { host: host.to_string(), port: target.port };
        let connect = client::connect(config, (host, target.port), check);
        let mut handle = tokio::time::timeout(Duration::from_secs(15), connect)
            .await
            .map_err(|_| anyhow!("timed out connecting to {}:{}", target.host, target.port))?
            .map_err(|e| match e {
                russh::Error::UnknownKey => anyhow!(
                    "{} is not in ~/.ssh/known_hosts; connect once with `ssh -p {} {}@{}` first",
                    target.host, target.port, target.user, target.host
                ),
                e => anyhow!(e).context(format!("connecting to {}:{}", target.host, target.port)),
            })?;

        if !authenticate(&mut handle, &target.user, &resolved.identity_files).await? {
            bail!("no ssh-agent identity or unencrypted IdentityFile accepted by {}@{}", target.user, target.host);
        }
        Ok(Self { handle: Arc::new(handle) })
    }

    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }
}

#[derive(Default)]
struct ResolvedConfig {
    hostname: Option<String>,
    identity_files: Vec<PathBuf>,
}

/// Lets OpenSSH resolve ~/.ssh/config (Include, Match, wildcards) via
/// `ssh -G` instead of re-implementing its parser. Falls back to defaults.
async fn resolve_ssh_config(target: &SshTarget) -> ResolvedConfig {
    let output = tokio::process::Command::new("ssh")
        .args(["-G", "-p", &target.port.to_string(), "-l", &target.user, "--", &target.host])
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => parse_ssh_g(&String::from_utf8_lossy(&out.stdout)),
        _ => ResolvedConfig {
            hostname: None,
            identity_files: ["id_ed25519", "id_ecdsa", "id_rsa"].iter().map(|n| expand_home(&format!("~/.ssh/{n}"))).collect(),
        },
    }
}

fn parse_ssh_g(output: &str) -> ResolvedConfig {
    let mut resolved = ResolvedConfig::default();
    for line in output.lines() {
        match line.split_once(' ') {
            Some(("hostname", value)) => resolved.hostname = Some(value.to_string()),
            Some(("identityfile", value)) => resolved.identity_files.push(expand_home(value)),
            _ => {}
        }
    }
    resolved
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// ssh-agent first, then unencrypted IdentityFiles.
async fn authenticate(handle: &mut Handle<KnownHostsCheck>, user: &str, identity_files: &[PathBuf]) -> Result<bool> {
    let hash_alg = handle.best_supported_rsa_hash().await?.flatten();

    if let Ok(mut agent) = AgentClient::connect_env().await {
        for identity in agent.request_identities().await.unwrap_or_default() {
            let key = identity.public_key().into_owned();
            let result = handle.authenticate_publickey_with(user, key, hash_alg, &mut agent).await?;
            if result.success() {
                return Ok(true);
            }
        }
    }

    for path in identity_files {
        let Ok(key) = load_secret_key(path, None) else { continue };
        let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
        if handle.authenticate_publickey(user, key).await?.success() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[async_trait]
impl SshTransport for RusshTransport {
    async fn exec(&self, cmd: &str) -> Result<mpsc::Receiver<ExecEvent>> {
        let mut channel = self.handle.channel_open_session().await?;
        channel.exec(true, cmd).await?;

        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut stdout = LineBuffer::default();
            let mut stderr = LineBuffer::default();
            loop {
                // Stop when the receiver is dropped (e.g. a log follower was
                // unsubscribed), even if the remote command is quiet.
                let msg = tokio::select! {
                    msg = channel.wait() => msg,
                    _ = tx.closed() => None,
                };
                let Some(msg) = msg else { break };
                let (lines, wrap): (Vec<String>, fn(String) -> ExecEvent) = match msg {
                    ChannelMsg::Data { ref data } => (stdout.push(data), ExecEvent::Stdout),
                    ChannelMsg::ExtendedData { ref data, ext: 1 } => (stderr.push(data), ExecEvent::Stderr),
                    ChannelMsg::ExitStatus { exit_status } => {
                        let _ = tx.send(ExecEvent::Exit(exit_status)).await;
                        continue;
                    }
                    _ => continue,
                };
                for line in lines {
                    if tx.send(wrap(line)).await.is_err() {
                        break;
                    }
                }
            }
            if tx.is_closed() {
                let _ = channel.close().await;
                return;
            }
            for line in stdout.finish() {
                let _ = tx.send(ExecEvent::Stdout(line)).await;
            }
            for line in stderr.finish() {
                let _ = tx.send(ExecEvent::Stderr(line)).await;
            }
        });
        Ok(rx)
    }
}

/// Reassembles lines from SSH data chunks, which can split mid-line.
#[derive(Default)]
struct LineBuffer(Vec<u8>);

impl LineBuffer {
    fn push(&mut self, data: &[u8]) -> Vec<String> {
        self.0.extend_from_slice(data);
        let mut lines = vec![];
        while let Some(pos) = self.0.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.0.drain(..=pos).collect();
            lines.push(String::from_utf8_lossy(&line[..pos]).trim_end_matches('\r').to_string());
        }
        lines
    }

    fn finish(&mut self) -> Vec<String> {
        if self.0.is_empty() {
            return vec![];
        }
        vec![String::from_utf8_lossy(&std::mem::take(&mut self.0)).to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_g_output() {
        let out = "user app\nhostname 10.0.0.1\nport 22\nidentityfile /keys/a\nidentityfile /keys/b\n";
        let resolved = parse_ssh_g(out);
        assert_eq!(resolved.hostname.as_deref(), Some("10.0.0.1"));
        assert_eq!(resolved.identity_files, vec![PathBuf::from("/keys/a"), PathBuf::from("/keys/b")]);
    }

    #[test]
    fn line_buffer_joins_split_chunks() {
        let mut buf = LineBuffer::default();
        assert_eq!(buf.push(b"hel"), Vec::<String>::new());
        assert_eq!(buf.push(b"lo\r\nwor"), vec!["hello"]);
        assert_eq!(buf.push(b"ld\n\n"), vec!["world", ""]);
        assert_eq!(buf.push(b"tail"), Vec::<String>::new());
        assert_eq!(buf.finish(), vec!["tail"]);
    }
}
