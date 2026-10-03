use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::db::{self, NewRun, Project};

/// Allowlisted kamal invocations. The UI never sends raw argv.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KamalCommand {
    Version,
    Deploy,
    Redeploy,
    Rollback { version: String },
    LockAcquire { message: String },
    LockRelease,
    LockStatus,
    AppBoot,
    AppStop,
}

impl KamalCommand {
    pub fn name(&self) -> &'static str {
        match self {
            KamalCommand::Version => "version",
            KamalCommand::Deploy => "deploy",
            KamalCommand::Redeploy => "redeploy",
            KamalCommand::Rollback { .. } => "rollback",
            KamalCommand::LockAcquire { .. } => "lock acquire",
            KamalCommand::LockRelease => "lock release",
            KamalCommand::LockStatus => "lock status",
            KamalCommand::AppBoot => "app boot",
            KamalCommand::AppStop => "app stop",
        }
    }

    pub fn args(&self, project: &Path, destination: Option<&str>) -> Result<Vec<String>> {
        let mut args: Vec<String> = self.name().split(' ').map(String::from).collect();
        match self {
            KamalCommand::Version => return Ok(args),
            KamalCommand::Rollback { version } => {
                if version.is_empty() || !version.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
                    bail!("invalid version: {version:?}");
                }
                args.push(version.clone());
            }
            KamalCommand::LockAcquire { message } => {
                if message.trim().is_empty() {
                    bail!("lock message is required");
                }
                args.extend(["-m".into(), message.clone()]);
            }
            _ => {}
        }
        Ok(with_destination(args, project, destination))
    }
}

/// The Rails console: runs in the already-running app container.
pub fn console_args(project: &Path, destination: Option<&str>) -> Vec<String> {
    let args = ["app", "exec", "--interactive", "--reuse", "bin/rails console"];
    with_destination(args.map(String::from).to_vec(), project, destination)
}

/// Whether the project has kamal's base `config/deploy.yml`. Without one, each
/// `config/deploy.<dest>.yml` is a standalone config, selected with `-c`.
pub fn has_base_config(project: &Path) -> bool {
    project.join("config/deploy.yml").is_file()
}

pub fn with_destination(mut args: Vec<String>, project: &Path, destination: Option<&str>) -> Vec<String> {
    if let Some(dest) = destination {
        if has_base_config(project) {
            args.extend(["-d".into(), dest.into()]);
        } else {
            args.extend(["-c".into(), format!("config/deploy.{dest}.yml")]);
        }
    }
    args
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    Line { stream: &'static str, text: String },
    Exit { code: Option<i32> },
}

/// A resolved kamal invocation: program, argv and the environment to run it in.
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

impl Launch {
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args).current_dir(&self.cwd).env_clear().envs(self.env.iter().cloned()).stdin(Stdio::null());
        cmd
    }
}

/// Uses the project's configured binary, else its `bin/kamal` binstub, else
/// `kamal` on the login PATH. Runs in the project's login-shell environment
/// so rbenv/mise/asdf resolve the right Ruby, without shell startup noise.
pub async fn kamal_launch(project: &Path, kamal_bin: Option<&str>, args: &[String]) -> Result<Launch> {
    let mut env = login_env(project).await?;
    env.retain(|(k, _)| k != "SSHKIT_COLOR");
    env.push(("SSHKIT_COLOR".into(), "1".into()));
    let path_var = env.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v.as_str()).unwrap_or("");

    let binstub = project.join("bin/kamal");
    let program = match kamal_bin {
        Some(bin) if bin.contains('/') => PathBuf::from(bin),
        Some(bin) => which(path_var, bin).with_context(|| format!("{bin} not found on login PATH"))?,
        None if binstub.is_file() => binstub,
        None => which(path_var, "kamal").context("kamal not found on login PATH; install it or add a bin/kamal binstub")?,
    };
    Ok(Launch { program, args: args.to_vec(), env, cwd: project.to_path_buf() })
}

fn which(path_var: &str, bin: &str) -> Option<PathBuf> {
    path_var.split(':').map(|dir| Path::new(dir).join(bin)).find(|p| p.is_file())
}

const ENV_MARKER: &str = "__KDM_ENV__";

/// The environment an interactive login shell has in `project` (version
/// managers usually hook in via .zshrc and pick tools per directory).
/// Captured once per project; anything the rc files print is ignored.
async fn login_env(project: &Path) -> Result<Vec<(String, String)>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<PathBuf, Vec<(String, String)>>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(env) = cache.lock().unwrap().get(project) {
        return Ok(env.clone());
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let script = format!(r#"printf '\0{ENV_MARKER}\0'; env -0"#);
    let capture = Command::new(shell).arg("-lic").arg(script).current_dir(project).stdin(Stdio::null()).output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(20), capture)
        .await
        .context("login shell took over 20s to start")?
        .context("spawning login shell")?;
    let env = parse_env(&out.stdout).context("could not read the login shell environment")?;
    cache.lock().unwrap().insert(project.to_path_buf(), env.clone());
    Ok(env)
}

fn parse_env(stdout: &[u8]) -> Option<Vec<(String, String)>> {
    let marker = format!("\0{ENV_MARKER}\0");
    let start = stdout.windows(marker.len()).rposition(|w| w == marker.as_bytes())? + marker.len();
    let env: Vec<(String, String)> = stdout[start..]
        .split(|&b| b == 0)
        .filter_map(|entry| {
            let (k, v) = std::str::from_utf8(entry).ok()?.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect();
    env.iter().any(|(k, _)| k == "PATH").then_some(env)
}

/// Deletes every Docker keychain entry for `server`. Entries written by another
/// credential helper (e.g. Docker Desktop's) can be neither updated nor erased by
/// the current `docker-credential-osxkeychain` (-25244), and `docker logout`
/// ignores that failure, so kamal's `docker login` keeps hitting -25299.
/// Kamal logs in again on every deploy, so removing them is safe.
pub async fn clear_docker_login(server: &str) -> Result<usize> {
    if !valid_registry(server) {
        bail!("not a registry host: {server}");
    }
    let host = server.split(':').next().unwrap_or(server);
    let mut removed = 0;
    // `security` deletes one matching item per call.
    while removed < 20 {
        let out = Command::new("/usr/bin/security")
            .args(["delete-internet-password", "-s", host, "-l", "Docker Credentials"])
            .stdin(Stdio::null())
            .output()
            .await
            .context("running security")?;
        if !out.status.success() {
            break;
        }
        removed += 1;
    }
    if removed == 0 {
        bail!("no Docker keychain entry for {host}");
    }
    Ok(removed)
}

fn valid_registry(server: &str) -> bool {
    !server.is_empty() && !server.starts_with('-') && server.chars().all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c))
}

pub fn validate_project(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    if !has_base_config(&path) && crate::project::destinations(&path).is_empty() {
        bail!("{} has no config/deploy.yml", path.display());
    }
    Ok(path)
}

/// Runs kamal to completion and returns stdout. Stderr goes into the error
/// only — stdout may hold resolved config and must not be logged.
pub async fn output(project: &Project, args: &[String]) -> Result<String> {
    let launch = kamal_launch(Path::new(&project.path), project.kamal_bin.as_deref(), args).await?;
    let out = launch.command().output().await.context("spawning kamal")?;
    if !out.status.success() {
        bail!("kamal {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8(out.stdout)?)
}

type RunKey = (i64, Option<String>);

/// Active runs: at most one per project + destination, with the pid used to
/// signal the run's process group on cancel.
#[derive(Default)]
pub struct RunRegistry {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    busy: HashSet<RunKey>,
    pids: HashMap<i64, u32>,
}

impl RunRegistry {
    fn reserve(self: &Arc<Self>, key: RunKey) -> Result<Reservation> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.busy.insert(key.clone()) {
            bail!("a command is already running for {}", key.1.as_deref().unwrap_or("the default destination"));
        }
        Ok(Reservation { registry: self.clone(), key, run_id: None })
    }

    pub fn cancel(&self, run_id: i64, force: bool) -> Result<()> {
        let pid = *self.inner.lock().unwrap().pids.get(&run_id).context("run is not active")?;
        let signal = if force { libc::SIGKILL } else { libc::SIGINT };
        // Negative pid: signal the whole group, like Ctrl-C in a terminal.
        if unsafe { libc::kill(-(pid as i32), signal) } != 0 {
            bail!("signal failed: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Frees the destination slot when the run ends, however it ends.
struct Reservation {
    registry: Arc<RunRegistry>,
    key: RunKey,
    run_id: Option<i64>,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut inner = self.registry.inner.lock().unwrap();
        inner.busy.remove(&self.key);
        if let Some(id) = self.run_id {
            inner.pids.remove(&id);
        }
    }
}

async fn git_sha(project: &Path) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(project).args(["rev-parse", "HEAD"]).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Starts a run in the background and returns its id. Output streams to
/// `emit` and `<runs_dir>/<id>.log`; the history row is closed on exit.
pub async fn start(
    registry: &Arc<RunRegistry>,
    pool: &SqlitePool,
    runs_dir: &Path,
    project: &Project,
    destination: Option<String>,
    command: KamalCommand,
    emit: impl Fn(RunEvent) + Send + Sync + 'static,
) -> Result<i64> {
    let project_dir = validate_project(&project.path)?;
    let args = command.args(&project_dir, destination.as_deref())?;
    let mut reservation = registry.reserve((project.id, destination.clone()))?;

    let sha = git_sha(&project_dir).await;
    let new = NewRun { project_id: project.id, destination: destination.as_deref(), command: command.name(), args: &args, git_sha: sha.as_deref() };
    let run = db::run_insert(pool, new, |id| runs_dir.join(format!("{id}.log")).to_string_lossy().into_owned()).await?;

    let spawned = async {
        let launch = kamal_launch(&project_dir, project.kamal_bin.as_deref(), &args).await?;
        let log = File::create(&run.log_path).context("creating run log")?;
        let child = launch
            .command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .context("spawning kamal")?;
        anyhow::Ok((log, child))
    }
    .await;
    let (log, child) = match spawned {
        Ok(ok) => ok,
        Err(e) => {
            db::run_finish(pool, run.id, None).await?;
            return Err(e);
        }
    };

    reservation.run_id = Some(run.id);
    if let Some(pid) = child.id() {
        registry.inner.lock().unwrap().pids.insert(run.id, pid);
    }

    let pool = pool.clone();
    tokio::spawn(async move {
        let _reservation = reservation;
        let mut log = BufWriter::new(log);
        let code = pump(child, |event| {
            if let RunEvent::Line { text, .. } = &event {
                let _ = writeln!(log, "{text}");
            }
            emit(event);
        })
        .await
        .unwrap_or(None);
        let _ = log.flush();
        let _ = db::run_finish(&pool, run.id, code).await;
    });
    Ok(run.id)
}

/// Streams merged stdout/stderr lines, then the exit event.
async fn pump(mut child: Child, mut emit: impl FnMut(RunEvent)) -> Result<Option<i32>> {
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();
    let (mut out_open, mut err_open) = (true, true);
    while out_open || err_open {
        tokio::select! {
            line = stdout.next_line(), if out_open => match line? {
                Some(text) => emit(RunEvent::Line { stream: "stdout", text }),
                None => out_open = false,
            },
            line = stderr.next_line(), if err_open => match line? {
                Some(text) => emit(RunEvent::Line { stream: "stderr", text }),
                None => err_open = false,
            },
        }
    }
    let code = child.wait().await?.code();
    emit(RunEvent::Exit { code });
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[test]
    fn parses_env_after_marker() {
        let out = b"p10k noise\x1b[0m\0__KDM_ENV__\0PATH=/a:/b\0X=1=2\0";
        assert_eq!(parse_env(out).unwrap(), [("PATH".into(), "/a:/b".into()), ("X".into(), "1=2".into())]);
        assert!(parse_env(b"no marker").is_none());
    }

    #[test]
    fn validates_registry_hosts() {
        assert!(valid_registry("ghcr.io"));
        assert!(valid_registry("registry.example.com:5000"));
        assert!(!valid_registry(""));
        assert!(!valid_registry("--all"));
        assert!(!valid_registry("ghcr.io; rm -rf ~"));
    }

    #[test]
    fn builds_allowlisted_args() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/deploy.yml"), "service: app\n").unwrap();
        let dest = Some("staging");
        assert_eq!(KamalCommand::Deploy.args(dir, dest).unwrap(), ["deploy", "-d", "staging"]);
        assert_eq!(KamalCommand::Version.args(dir, dest).unwrap(), ["version"]);
        assert_eq!(KamalCommand::AppStop.args(dir, None).unwrap(), ["app", "stop"]);
        assert_eq!(
            KamalCommand::Rollback { version: "abc123".into() }.args(dir, dest).unwrap(),
            ["rollback", "abc123", "-d", "staging"]
        );
        assert_eq!(
            KamalCommand::LockAcquire { message: "db migration; don't deploy".into() }.args(dir, None).unwrap(),
            ["lock", "acquire", "-m", "db migration; don't deploy"]
        );
        assert!(KamalCommand::Rollback { version: "abc; rm -rf /".into() }.args(dir, dest).is_err());
        assert!(KamalCommand::LockAcquire { message: " ".into() }.args(dir, dest).is_err());
    }

    #[test]
    fn standalone_destination_configs() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        assert!(validate_project(&dir.to_string_lossy()).is_err());

        std::fs::write(dir.join("config/deploy.staging.yml"), "service: app\n").unwrap();
        assert!(validate_project(&dir.to_string_lossy()).is_ok());
        assert_eq!(
            KamalCommand::Deploy.args(dir, Some("staging")).unwrap(),
            ["deploy", "-c", "config/deploy.staging.yml"]
        );
    }

    /// A project whose bin/kamal is a script: prints its args, then sleeps.
    fn fake_project(dir: &Path) -> Project {
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/deploy.yml"), "service: app\n").unwrap();
        let bin = dir.join("kamal-fake");
        std::fs::write(&bin, "#!/bin/sh\necho \"args: $*\"\necho oops >&2\n[ \"$1\" = deploy ] && sleep 30\nexit 3\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        Project {
            id: 0,
            name: "app".into(),
            path: dir.to_string_lossy().into(),
            kamal_bin: Some(bin.to_string_lossy().into()),
            default_destination: None,
            created_at: 0,
        }
    }

    async fn wait_finished(pool: &SqlitePool, id: i64) -> db::Run {
        for _ in 0..100 {
            let run = db::run_get(pool, id).await.unwrap();
            if run.finished_at.is_some() {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("run {id} did not finish");
    }

    #[tokio::test]
    async fn runs_stream_log_and_record_history() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = db::memory().await;
        let mut project = fake_project(tmp.path());
        project.id = db::project_add(&pool, "app", &project.path).await.unwrap().id;
        let registry = Arc::new(RunRegistry::default());

        let (tx, mut rx) = mpsc::unbounded_channel();
        let id = start(&registry, &pool, tmp.path(), &project, Some("staging".into()), KamalCommand::LockStatus, move |e| {
            let _ = tx.send(e);
        })
        .await
        .unwrap();

        let run = wait_finished(&pool, id).await;
        assert_eq!(run.exit_code, Some(3));
        assert_eq!(run.command, "lock status");
        let log = std::fs::read_to_string(&run.log_path).unwrap();
        assert!(log.contains("args: lock status -d staging"));
        assert!(log.contains("oops"));
        let mut saw_exit = false;
        while let Ok(e) = rx.try_recv() {
            saw_exit |= matches!(e, RunEvent::Exit { code: Some(3) });
        }
        assert!(saw_exit);
    }

    #[tokio::test]
    async fn one_run_per_destination_and_cancel() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = db::memory().await;
        let mut project = fake_project(tmp.path());
        project.id = db::project_add(&pool, "app", &project.path).await.unwrap().id;
        let registry = Arc::new(RunRegistry::default());
        let dest = || Some("production".to_string());

        let id = start(&registry, &pool, tmp.path(), &project, dest(), KamalCommand::Deploy, |_| {}).await.unwrap();
        let second = start(&registry, &pool, tmp.path(), &project, dest(), KamalCommand::Deploy, |_| {}).await;
        assert!(second.unwrap_err().to_string().contains("already running"));

        tokio::time::sleep(Duration::from_millis(1500)).await; // let the login shell exec
        registry.cancel(id, false).unwrap();
        let run = wait_finished(&pool, id).await;
        assert_eq!(run.exit_code, None, "killed by SIGINT");

        // Slot is free again once the run ends.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let again = start(&registry, &pool, tmp.path(), &project, dest(), KamalCommand::LockStatus, |_| {}).await;
        assert!(again.is_ok());
    }
}
