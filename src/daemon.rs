//! Process supervision: spawn `caffeinate`, notice when it exits, kill it.
//!
//! Polling happens on the UI loop's 1s tick rather than in a detached task.
//! ponytail: the UI is the only reader of this state, so a background task would
//! only buy an `Arc<Mutex<..>>` and identical timing. Move `poll` onto its own
//! task if a headless/daemon mode ever needs it without a terminal attached.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use chrono::{Duration as ChronoDuration, Local};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tokio::process::{Child, Command};
use uuid::Uuid;

use crate::models::{CaffeineSession, SessionStatus, Target, CAFFEINATE};

/// How long a session gets to honour SIGTERM before SIGKILL.
pub const GRACE_PERIOD: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct Daemon {
    children: HashMap<Uuid, Child>,
    /// Sessions sent SIGTERM, with the instant SIGKILL becomes fair game.
    escalate_at: HashMap<Uuid, Instant>,
}

impl Daemon {
    pub fn new() -> Self {
        Self::default()
    }

    /// Launch the session's command line and record its PID.
    ///
    /// stdio is discarded: a `Command` target's output would otherwise scribble
    /// over the TUI, and stdin must not be shared or the child fights for keys.
    pub fn spawn(&mut self, session: &mut CaffeineSession) -> Result<()> {
        if self.children.contains_key(&session.id) {
            return Err(anyhow!("session is already running"));
        }

        let child = Command::new(CAFFEINATE)
            .args(session.args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| anyhow!("could not start {CAFFEINATE}: {error}"))?;

        session.pid = child.id();
        session.status = SessionStatus::Running;
        session.started_at = Local::now();
        session.expires_at = match session.target {
            Target::Timeout(seconds) => {
                Some(Local::now() + ChronoDuration::seconds(seconds as i64))
            }
            _ => None,
        };

        self.children.insert(session.id, child);
        self.escalate_at.remove(&session.id);
        Ok(())
    }

    /// Reap any child that has exited and escalate overdue kills. Called once
    /// per second from the event loop.
    pub fn poll(&mut self, sessions: &mut [CaffeineSession]) {
        let now = Instant::now();

        for session in sessions.iter_mut() {
            let Some(child) = self.children.get_mut(&session.id) else {
                // No child handle: an external session we signalled. There is
                // nothing to reap — only the escalation to finish. The scan that
                // discovered it is what removes the row once it dies.
                if let (Some(deadline), Some(pid)) =
                    (self.escalate_at.get(&session.id), session.pid)
                {
                    if now >= *deadline {
                        let _ = signal(pid, Signal::SIGKILL);
                    }
                }
                continue;
            };

            match child.try_wait() {
                Ok(Some(status)) => {
                    // A killed session is Stopped, not Finished — the exit was ours.
                    session.status = if self.escalate_at.contains_key(&session.id) {
                        SessionStatus::Stopped
                    } else {
                        SessionStatus::Finished(status.code().unwrap_or(-1))
                    };
                    session.pid = None;
                    self.children.remove(&session.id);
                    self.escalate_at.remove(&session.id);
                }
                Ok(None) => {
                    if let Some(deadline) = self.escalate_at.get(&session.id) {
                        if now >= *deadline {
                            if let Some(pid) = session.pid {
                                let _ = signal(pid, Signal::SIGKILL);
                            }
                            // Keep the deadline set so the next tick reaps it.
                        }
                    }
                }
                Err(error) => {
                    session.status = SessionStatus::Error(error.to_string());
                    session.pid = None;
                    self.children.remove(&session.id);
                    self.escalate_at.remove(&session.id);
                }
            }
        }
    }

    /// Send SIGTERM and arm the SIGKILL escalation. Returns without blocking —
    /// `poll` finishes the job so the UI stays responsive.
    ///
    /// Works for external sessions too: those have a PID but no `Child`, so they
    /// are signalled directly and left for the scan to notice.
    pub fn kill(&mut self, session: &mut CaffeineSession) -> Result<()> {
        let Some(pid) = session.pid else {
            session.status = SessionStatus::Stopped;
            return Ok(());
        };
        if !self.children.contains_key(&session.id) && !session.external {
            session.status = SessionStatus::Stopped;
            session.pid = None;
            return Ok(());
        }

        self.escalate_at
            .insert(session.id, Instant::now() + GRACE_PERIOD);
        signal(pid, Signal::SIGTERM)
    }

    pub fn is_supervised(&self, id: Uuid) -> bool {
        self.children.contains_key(&id)
    }

    /// Drop any bookkeeping for a session that no longer exists, so a removed
    /// external row cannot leak an armed escalation.
    pub fn forget(&mut self, id: Uuid) {
        self.children.remove(&id);
        self.escalate_at.remove(&id);
    }

    /// Terminate and reap everything before the process exits, so no
    /// `caffeinate` outlives the UI and no zombie is left behind.
    pub fn shutdown(&mut self, sessions: &mut [CaffeineSession]) {
        for session in sessions.iter_mut() {
            if self.children.contains_key(&session.id) {
                if let Some(pid) = session.pid {
                    let _ = signal(pid, Signal::SIGTERM);
                }
            }
        }

        let deadline = Instant::now() + GRACE_PERIOD;
        while Instant::now() < deadline && !self.children.is_empty() {
            self.reap_exited();
            std::thread::sleep(Duration::from_millis(25));
        }

        // Anything still alive gets SIGKILL, then one last reap.
        for child in self.children.values_mut() {
            let _ = child.start_kill();
        }
        self.reap_exited();

        for session in sessions.iter_mut() {
            // External sessions keep running after we exit — they are not ours to
            // stop, and reporting them as Stopped would be a lie.
            if session.external {
                continue;
            }
            if session.status == SessionStatus::Running {
                session.status = SessionStatus::Stopped;
            }
            session.pid = None;
        }
        self.children.clear();
        self.escalate_at.clear();
    }

    fn reap_exited(&mut self) {
        self.children
            .retain(|_, child| !matches!(child.try_wait(), Ok(Some(_))));
    }
}

fn signal(pid: u32, sig: Signal) -> Result<()> {
    kill(Pid::from_raw(pid as i32), sig).map_err(|error| anyhow!("signalling {pid}: {error}"))
}
