use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::ssh::{RusshTransport, SshTarget, SshTransport};

pub fn host_key(target: &SshTarget) -> String {
    format!("{}@{}:{}", target.user, target.host, target.port)
}

type Slot = Arc<tokio::sync::Mutex<Option<Arc<RusshTransport>>>>;

/// One SSH connection per user@host:port, shared by pollers and log streams.
/// Each host has its own slot lock, so a slow host never blocks the others.
#[derive(Default)]
pub struct SshPool {
    slots: Mutex<HashMap<String, Slot>>,
}

impl SshPool {
    pub async fn get(&self, target: &SshTarget) -> Result<Arc<dyn SshTransport>> {
        let slot = self.slots.lock().unwrap().entry(host_key(target)).or_default().clone();
        let mut conn = slot.lock().await;
        if let Some(existing) = conn.as_ref().filter(|c| !c.is_closed()) {
            return Ok(existing.clone());
        }
        let fresh = Arc::new(RusshTransport::connect(target).await?);
        *conn = Some(fresh.clone());
        Ok(fresh)
    }

    /// Drops the connection after a failed exec so the next call reconnects.
    pub async fn evict(&self, target: &SshTarget) {
        let slot = self.slots.lock().unwrap().get(&host_key(target)).cloned();
        if let Some(slot) = slot {
            *slot.lock().await = None;
        }
    }
}
