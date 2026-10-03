mod collector;
mod console;
mod db;
mod docker;
mod logs;
mod metrics;
mod pool;
mod project;
mod proxy;
mod runner;
mod ssh;

use std::path::PathBuf;
use std::sync::Arc;

use sqlx::SqlitePool;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State, WindowEvent};

use crate::collector::{Collector, HostSnapshot};
use crate::console::{ConsoleEvent, Consoles};
use crate::db::{Project, Run};
use crate::logs::{LogEvent, LogOptions, LogStreams};
use crate::pool::SshPool;
use crate::project::ProjectConfig;
use crate::proxy::{ProxyRequest, ProxyRoute};
use crate::runner::{KamalCommand, RunEvent, RunRegistry};
use crate::ssh::SshTarget;

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

struct AppState {
    pool: SqlitePool,
    runs_dir: PathBuf,
    registry: Arc<RunRegistry>,
    ssh: Arc<SshPool>,
    collector: Collector,
    logs: LogStreams,
    consoles: Consoles,
}

#[tauri::command]
async fn project_add(state: State<'_, AppState>, path: String) -> CmdResult<Project> {
    let dir = runner::validate_project(&path).map_err(err)?;
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
    db::project_add(&state.pool, &name, &path).await.map_err(err)
}

/// Process start, for the one-off "UI ready" log below.
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

#[tauri::command]
async fn project_list(state: State<'_, AppState>) -> CmdResult<Vec<Project>> {
    // The UI calls this first, so it marks when the window became usable (PRD: cold start < 1.5 s).
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        if let Some(start) = STARTED.get() {
            eprintln!("kdm: ui ready after {} ms", start.elapsed().as_millis());
        }
    });
    db::project_list(&state.pool).await.map_err(err)
}

#[tauri::command]
async fn project_remove(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    for log in db::project_remove(&state.pool, id).await.map_err(err)? {
        let _ = std::fs::remove_file(log);
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct ProjectInfo {
    destinations: Vec<String>,
    /// The destination actually loaded: projects without a base config fall back to their first one.
    destination: Option<String>,
    has_base_config: bool,
    config: ProjectConfig,
}

/// Loads config for a destination and remembers it as the project's default.
#[tauri::command]
async fn project_config(state: State<'_, AppState>, id: i64, destination: Option<String>) -> CmdResult<ProjectInfo> {
    let project = db::project_get(&state.pool, id).await.map_err(err)?;
    let dir = runner::validate_project(&project.path).map_err(err)?;
    let destinations = project::destinations(&dir);
    let has_base_config = runner::has_base_config(&dir);
    let destination = if has_base_config { destination } else { destination.or_else(|| destinations.first().cloned()) };
    let args = runner::with_destination(vec!["config".into()], &dir, destination.as_deref());
    let yaml = runner::output(&project, &args).await.map_err(err)?;
    let config = project::parse_config(&yaml).map_err(err)?;
    db::project_set_destination(&state.pool, id, destination.as_deref()).await.map_err(err)?;
    Ok(ProjectInfo { destinations, destination, has_base_config, config })
}

#[tauri::command]
async fn run_start(
    state: State<'_, AppState>,
    project_id: i64,
    destination: Option<String>,
    command: KamalCommand,
    on_event: Channel<RunEvent>,
) -> CmdResult<i64> {
    let project = db::project_get(&state.pool, project_id).await.map_err(err)?;
    runner::start(&state.registry, &state.pool, &state.runs_dir, &project, destination, command, move |event| {
        let _ = on_event.send(event);
    })
    .await
    .map_err(err)
}

#[tauri::command]
async fn clear_docker_login(server: String) -> CmdResult<usize> {
    runner::clear_docker_login(&server).await.map_err(err)
}

#[tauri::command]
fn run_cancel(state: State<'_, AppState>, run_id: i64, force: bool) -> CmdResult<()> {
    state.registry.cancel(run_id, force).map_err(err)
}

#[tauri::command]
async fn runs_list(state: State<'_, AppState>, project_id: i64, limit: i64) -> CmdResult<Vec<Run>> {
    db::runs_list(&state.pool, project_id, limit).await.map_err(err)
}

#[tauri::command]
async fn run_log(state: State<'_, AppState>, run_id: i64) -> CmdResult<String> {
    let run = db::run_get(&state.pool, run_id).await.map_err(err)?;
    tokio::fs::read_to_string(&run.log_path).await.map_err(|e| format!("reading log: {e}"))
}

/// Starts polling a host (shared with other watchers). Updates arrive as
/// `host:update` events; returns the last snapshot if one exists.
/// Async so it runs on Tauri's tokio runtime — the poller is spawned from here,
/// and sync commands run on the main thread, which has no runtime.
#[tauri::command]
async fn host_watch(state: State<'_, AppState>, target: SshTarget) -> CmdResult<Option<HostSnapshot>> {
    Ok(state.collector.watch(target))
}

#[tauri::command]
async fn host_unwatch(state: State<'_, AppState>, target: SshTarget) -> CmdResult<()> {
    state.collector.unwatch(&target);
    Ok(())
}

#[tauri::command]
async fn logs_subscribe(
    state: State<'_, AppState>,
    target: SshTarget,
    container: String,
    opts: LogOptions,
    on_event: Channel<LogEvent>,
) -> CmdResult<u64> {
    let emit = move |event| {
        let _ = on_event.send(event);
    };
    state.logs.subscribe(&state.ssh, &target, &container, &opts, emit).await.map_err(err)
}

/// Current kamal-proxy routes on a host. On an unrecognised table the error
/// carries the raw output so the UI can still show it.
#[tauri::command]
async fn proxy_routes(state: State<'_, AppState>, target: SshTarget) -> CmdResult<Vec<ProxyRoute>> {
    let transport = state.ssh.get(&target).await.map_err(err)?;
    let out = transport.exec_collect(proxy::LIST_COMMAND).await.map_err(err)?;
    if out.exit != Some(0) {
        return Err(format!("kamal-proxy list failed: {}", out.stderr.join("\n")));
    }
    proxy::parse_list(&out.stdout).map_err(|e| format!("{e:#}\n\n{}", out.stdout.join("\n")))
}

/// Follows kamal-proxy request logs for the given proxy services.
/// Stop with `logs_unsubscribe`.
#[tauri::command]
async fn proxy_requests_subscribe(
    state: State<'_, AppState>,
    target: SshTarget,
    services: Vec<String>,
    since: String,
    on_event: Channel<ProxyRequest>,
) -> CmdResult<u64> {
    let cmd = proxy::requests_command(&services, &since).map_err(err)?;
    let on_line = move |line: String| {
        if let Some(req) = proxy::parse_request(&line) {
            let _ = on_event.send(req);
        }
    };
    state.logs.follow(&state.ssh, &target, &cmd, on_line, |_| {}).await.map_err(err)
}

#[tauri::command]
async fn console_open(
    state: State<'_, AppState>,
    project_id: i64,
    destination: Option<String>,
    rows: u16,
    cols: u16,
    on_event: Channel<ConsoleEvent>,
) -> CmdResult<u64> {
    let project = db::project_get(&state.pool, project_id).await.map_err(err)?;
    let dir = runner::validate_project(&project.path).map_err(err)?;
    let launch = runner::kamal_launch(&dir, project.kamal_bin.as_deref(), &runner::console_args(&dir, destination.as_deref()))
        .await
        .map_err(err)?;
    let emit = move |event| {
        let _ = on_event.send(event);
    };
    state.consoles.open(&launch, rows, cols, emit).map_err(err)
}

#[tauri::command]
fn console_write(state: State<'_, AppState>, session_id: u64, data: String) -> CmdResult<()> {
    state.consoles.write(session_id, &data).map_err(err)
}

#[tauri::command]
fn console_resize(state: State<'_, AppState>, session_id: u64, rows: u16, cols: u16) -> CmdResult<()> {
    state.consoles.resize(session_id, rows, cols).map_err(err)
}

#[tauri::command]
fn console_close(state: State<'_, AppState>, session_id: u64) {
    state.consoles.close(session_id)
}

#[tauri::command]
fn logs_unsubscribe(state: State<'_, AppState>, sub_id: u64) {
    state.logs.unsubscribe(sub_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    STARTED.get_or_init(std::time::Instant::now);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            // Builds named Keel used the identifier dev.keel.desktop; carry their data over once.
            let legacy_dir = data_dir.with_file_name("dev.keel.desktop");
            let migrated = !data_dir.exists() && legacy_dir.is_dir() && std::fs::rename(&legacy_dir, &data_dir).is_ok();
            if migrated {
                let _ = std::fs::rename(data_dir.join("keel.db"), data_dir.join("kdm.db"));
            }
            let runs_dir = data_dir.join("runs");
            std::fs::create_dir_all(&runs_dir)?;
            let pool = tauri::async_runtime::block_on(async {
                let pool = db::open(&data_dir.join("kdm.db")).await?;
                if migrated {
                    db::rebase_log_paths(&pool, &legacy_dir.to_string_lossy(), &data_dir.to_string_lossy()).await?;
                }
                anyhow::Ok(pool)
            })?;
            let ssh_pool = Arc::new(SshPool::default());
            let handle = app.handle().clone();
            let emit = Arc::new(move |snapshot: &HostSnapshot| {
                let _ = handle.emit("host:update", snapshot);
            });
            let collector = Collector::new(ssh_pool.clone(), emit);
            app.manage(AppState { pool, runs_dir, registry: Arc::default(), ssh: ssh_pool, collector, logs: LogStreams::default(), consoles: Consoles::default() });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Focused(focused) = event {
                window.state::<AppState>().collector.set_focused(*focused);
            }
        })
        .invoke_handler(tauri::generate_handler![
            project_add,
            project_list,
            project_remove,
            project_config,
            run_start,
            run_cancel,
            clear_docker_login,
            runs_list,
            run_log,
            host_watch,
            host_unwatch,
            logs_subscribe,
            logs_unsubscribe,
            proxy_routes,
            proxy_requests_subscribe,
            console_open,
            console_write,
            console_resize,
            console_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod live {
    use super::*;

    /// End-to-end check against a real project (read-only commands only):
    /// KDM_PROJECT=/path/to/app cargo test live_spike -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_spike() {
        let path = std::env::var("KDM_PROJECT").expect("set KDM_PROJECT");
        let destination = std::env::var("KDM_DESTINATION").ok();
        let project = Project { id: 0, name: "live".into(), path: path.clone(), kamal_bin: None, default_destination: None, created_at: 0 };

        let args = runner::with_destination(vec!["config".into()], std::path::Path::new(&path), destination.as_deref());
        let config = project::parse_config(&runner::output(&project, &args).await.unwrap()).unwrap();
        println!("config: {config:?}");

        let tmp = tempfile::tempdir().unwrap();
        let pool = db::memory().await;
        let project = db::project_add(&pool, "live", &path).await.unwrap();
        let registry = Arc::new(RunRegistry::default());
        let id = runner::start(&registry, &pool, tmp.path(), &project, destination, KamalCommand::LockStatus, |e| {
            if let RunEvent::Line { text, .. } = e {
                println!("kamal> {text}");
            }
        })
        .await
        .unwrap();
        loop {
            let run = db::run_get(&pool, id).await.unwrap();
            if run.finished_at.is_some() {
                println!("run: {run:?}");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        let host = config.primary_host.clone().unwrap_or_else(|| config.hosts[0].clone());
        let target = SshTarget { host, port: config.ssh_port, user: config.ssh_user.clone() };

        // Two polls through the collector: the second has CPU % (needs a delta).
        let ssh_pool = Arc::new(SshPool::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let collector = Collector::new(ssh_pool.clone(), Arc::new(move |s: &HostSnapshot| {
            let _ = tx.send(s.clone());
        }));
        collector.watch(target.clone());
        rx.recv().await.unwrap();
        let snapshot = rx.recv().await.unwrap();
        collector.unwatch(&target);
        assert_eq!(snapshot.error, None);
        println!("metrics: {:?}", snapshot.metrics.unwrap());
        for ct in snapshot.containers.iter().filter(|c| c.service.as_deref() == Some(config.service.as_str())) {
            println!("container: {} {} stale={} cpu={:?} mem={:?}", ct.name, ct.state, ct.stale, ct.cpu_pct, ct.mem_bytes);
        }

        // Follow logs of a running app container over the pooled connection, then stop.
        let web = snapshot
            .containers
            .iter()
            .find(|c| c.service.as_deref() == Some(config.service.as_str()) && c.state == "running")
            .expect("a running app container");
        let streams = LogStreams::default();
        let (ltx, mut lrx) = tokio::sync::mpsc::unbounded_channel();
        let opts = LogOptions { lines: Some(3), ..Default::default() };
        let id = streams
            .subscribe(&ssh_pool, &target, &web.name, &opts, move |e| {
                let _ = ltx.send(e);
            })
            .await
            .unwrap();
        for _ in 0..3 {
            if let Some(LogEvent::Line { text }) = lrx.recv().await {
                println!("log: {}", text.chars().take(120).collect::<String>());
            }
        }
        streams.unsubscribe(id);

        // Proxy routes and recent requests for this app's proxy services.
        let transport = ssh_pool.get(&target).await.unwrap();
        let out = transport.exec_collect(proxy::LIST_COMMAND).await.unwrap();
        let routes = proxy::parse_list(&out.stdout).unwrap();
        let services: Vec<String> = config.roles.iter().map(|r| format!("{}-{r}", config.service)).collect();
        for r in routes.iter().filter(|r| services.contains(&r.service)) {
            println!("route: {r:?}");
        }
        let (rtx, mut rrx) = tokio::sync::mpsc::unbounded_channel();
        let cmd = proxy::requests_command(&services, "24h").unwrap();
        let id = streams
            .follow(&ssh_pool, &target, &cmd, move |line| drop(rtx.send(proxy::parse_request(&line))), |_| {})
            .await
            .unwrap();
        let mut seen = 0;
        while seen < 3 {
            match tokio::time::timeout(std::time::Duration::from_secs(10), rrx.recv()).await {
                Ok(Some(Some(req))) => {
                    println!("request: {} {} {} {} {:.1}ms", req.time, req.method, req.path, req.status, req.duration_ms);
                    seen += 1;
                }
                Ok(Some(None)) => {} // deploy/health lines for the same service
                _ => break,
            }
        }
        streams.unsubscribe(id);

        if std::env::var("KDM_LIVE_CONSOLE").is_ok() {
            let dir = std::path::Path::new(&path);
            let launch = runner::kamal_launch(dir, None, &runner::console_args(dir, None)).await.unwrap();
            let consoles = Consoles::default();
            let (ctx, crx) = std::sync::mpsc::channel();
            let id = consoles.open(&launch, 24, 100, move |e| drop(ctx.send(e))).unwrap();
            let mut output = String::new();
            let mut sent = false;
            loop {
                match crx.recv_timeout(std::time::Duration::from_secs(60)).expect("console output") {
                    ConsoleEvent::Output { data } => output += &data,
                    ConsoleEvent::Exit => break,
                }
                if !sent && output.contains(">") {
                    consoles.write(id, "puts 20+22\r").unwrap();
                    consoles.write(id, "exit\r").unwrap();
                    sent = true;
                }
            }
            println!("console saw 42: {}", output.contains("42"));
        }
    }
}
