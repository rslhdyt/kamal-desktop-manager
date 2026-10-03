use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::task::JoinHandle;

use crate::db::now_ms;
use crate::docker::Container;
use crate::metrics::{self, CpuTimes, HostMetrics};
use crate::pool::{host_key, SshPool};
use crate::ssh::SshTarget;

const FOCUSED_INTERVAL: Duration = Duration::from_secs(5);
const BACKGROUND_INTERVAL: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
pub struct HostSnapshot {
    /// user@host:port — what the UI filters `host:update` events by.
    pub key: String,
    pub ts: i64,
    pub metrics: Option<HostMetrics>,
    pub containers: Vec<Container>,
    pub error: Option<String>,
}

pub type Emit = Arc<dyn Fn(&HostSnapshot) + Send + Sync>;

struct Watch {
    count: usize,
    task: JoinHandle<()>,
}

/// One poller per watched host, shared by every view that watches it.
pub struct Collector {
    pool: Arc<SshPool>,
    emit: Emit,
    focused: Arc<AtomicBool>,
    watches: Mutex<HashMap<String, Watch>>,
    latest: Arc<Mutex<HashMap<String, HostSnapshot>>>,
}

impl Collector {
    pub fn new(pool: Arc<SshPool>, emit: Emit) -> Self {
        Self {
            pool,
            emit,
            focused: Arc::new(AtomicBool::new(true)),
            watches: Mutex::default(),
            latest: Arc::default(),
        }
    }

    /// Slows polling while the window is in the background.
    pub fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Relaxed);
    }

    /// Starts (or joins) the poller for a host; returns the last snapshot if any.
    pub fn watch(&self, target: SshTarget) -> Option<HostSnapshot> {
        let key = host_key(&target);
        let mut watches = self.watches.lock().unwrap();
        match watches.get_mut(&key) {
            Some(w) => w.count += 1,
            None => {
                let task = tokio::spawn(poll(target, self.pool.clone(), self.emit.clone(), self.focused.clone(), self.latest.clone()));
                watches.insert(key.clone(), Watch { count: 1, task });
            }
        }
        self.latest.lock().unwrap().get(&key).cloned()
    }

    pub fn unwatch(&self, target: &SshTarget) {
        let key = host_key(target);
        let mut watches = self.watches.lock().unwrap();
        if let Some(w) = watches.get_mut(&key) {
            w.count -= 1;
            if w.count == 0 {
                w.task.abort();
                watches.remove(&key);
            }
        }
    }
}

async fn poll(target: SshTarget, pool: Arc<SshPool>, emit: Emit, focused: Arc<AtomicBool>, latest: Arc<Mutex<HashMap<String, HostSnapshot>>>) {
    let key = host_key(&target);
    let mut prev_cpu: Option<CpuTimes> = None;
    let mut backoff = Duration::ZERO;
    loop {
        let snapshot = match poll_once(&target, &pool, prev_cpu).await {
            Ok((metrics, cpu, containers)) => {
                prev_cpu = Some(cpu);
                backoff = Duration::ZERO;
                HostSnapshot { key: key.clone(), ts: now_ms(), metrics: Some(metrics), containers, error: None }
            }
            Err(e) => {
                pool.evict(&target).await;
                backoff = (backoff * 2).clamp(FOCUSED_INTERVAL, MAX_BACKOFF);
                // Keep the last good data on screen alongside the error.
                let last = latest.lock().unwrap().get(&key).cloned();
                HostSnapshot {
                    key: key.clone(),
                    ts: now_ms(),
                    metrics: last.as_ref().and_then(|s| s.metrics.clone()),
                    containers: last.map(|s| s.containers).unwrap_or_default(),
                    error: Some(format!("{e:#}")),
                }
            }
        };
        emit(&snapshot);
        latest.lock().unwrap().insert(key.clone(), snapshot);

        let interval = if focused.load(Ordering::Relaxed) { FOCUSED_INTERVAL } else { BACKGROUND_INTERVAL };
        tokio::time::sleep(interval.max(backoff)).await;
    }
}

async fn poll_once(target: &SshTarget, pool: &SshPool, prev_cpu: Option<CpuTimes>) -> anyhow::Result<(HostMetrics, CpuTimes, Vec<Container>)> {
    let transport = pool.get(target).await?;
    let out = transport.exec_collect(&metrics::batch_command()).await?;
    let sample = metrics::parse_batch(&out.stdout, prev_cpu).map_err(|e| {
        let stderr = out.stderr.join("\n");
        if stderr.is_empty() { e } else { e.context(stderr) }
    })?;
    Ok((sample.metrics, sample.cpu, sample.containers))
}
