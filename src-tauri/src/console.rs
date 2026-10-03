use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;

use crate::runner::Launch;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConsoleEvent {
    Output { data: String },
    Exit,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

/// Interactive kamal processes (e.g. `kamal app exec -i`) on local PTYs.
#[derive(Default)]
pub struct Consoles {
    next_id: AtomicU64,
    sessions: Arc<Mutex<HashMap<u64, Session>>>,
}

impl Consoles {
    pub fn open(&self, launch: &Launch, rows: u16, cols: u16, emit: impl Fn(ConsoleEvent) + Send + 'static) -> Result<u64> {
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        let mut cmd = CommandBuilder::new(&launch.program);
        cmd.args(&launch.args);
        cmd.cwd(&launch.cwd);
        cmd.env_clear();
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd).context("spawning console")?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.sessions.lock().unwrap().insert(id, Session { master: pair.master, writer, child });

        // Blocking PTY reads live on their own thread.
        let sessions = self.sessions.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut pending = Vec::new();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                pending.extend_from_slice(&buf[..n]);
                let data = take_utf8(&mut pending);
                if !data.is_empty() {
                    emit(ConsoleEvent::Output { data });
                }
            }
            let session = sessions.lock().unwrap().remove(&id);
            if let Some(mut s) = session {
                let _ = s.child.wait();
            }
            emit(ConsoleEvent::Exit);
        });
        Ok(id)
    }

    pub fn write(&self, id: u64, data: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        let s = sessions.get_mut(&id).context("console session ended")?;
        s.writer.write_all(data.as_bytes())?;
        s.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: u64, rows: u16, cols: u16) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let s = sessions.get(&id).context("console session ended")?;
        s.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        Ok(())
    }

    /// Kills the local kamal process; the reader thread then reports Exit.
    pub fn close(&self, id: u64) {
        if let Some(s) = self.sessions.lock().unwrap().get_mut(&id) {
            let _ = s.child.kill();
        }
    }
}

/// Drains the longest valid UTF-8 prefix, keeping a split multi-byte
/// character for the next read.
fn take_utf8(pending: &mut Vec<u8>) -> String {
    let valid = match std::str::from_utf8(pending) {
        Ok(_) => pending.len(),
        // An invalid sequence (not just a truncated tail) is passed through lossily.
        Err(e) if e.error_len().is_some() => pending.len(),
        Err(e) => e.valid_up_to(),
    };
    let bytes: Vec<u8> = pending.drain(..valid).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_split_utf8_for_next_read() {
        let mut pending = "héllo".as_bytes()[..2].to_vec(); // "h" + first byte of "é"
        assert_eq!(take_utf8(&mut pending), "h");
        pending.extend_from_slice(&"héllo".as_bytes()[2..]);
        assert_eq!(take_utf8(&mut pending), "éllo");
        assert!(pending.is_empty());
    }

    #[test]
    fn runs_an_interactive_process() {
        let launch = Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "read line; echo \"got:$line\"".into()],
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
            cwd: std::env::temp_dir(),
        };
        let consoles = Consoles::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let id = consoles.open(&launch, 24, 80, move |e| drop(tx.send(e))).unwrap();
        consoles.resize(id, 30, 100).unwrap();
        consoles.write(id, "hi\r").unwrap();

        let mut output = String::new();
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(5)).expect("console output") {
                ConsoleEvent::Output { data } => output += &data,
                ConsoleEvent::Exit => break,
            }
        }
        assert!(output.contains("got:hi"), "{output:?}");
    }
}
