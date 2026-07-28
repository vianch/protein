//! Core data model: a `caffeinate` invocation plus its live process state.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::utils;

/// The binary we drive. Every session is one invocation of it.
pub const CAFFEINATE: &str = "caffeinate";

/// The real `caffeinate(8)` assertion flags. There is deliberately no `-p`:
/// it does not exist in the tool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaffeinateFlags {
    /// `-d` — prevent the display from sleeping.
    pub display: bool,
    /// `-i` — prevent the system from idle sleeping.
    pub idle: bool,
    /// `-m` — prevent the disk from idle sleeping.
    pub disk: bool,
    /// `-s` — prevent the system from sleeping. Only honoured on AC power.
    pub system: bool,
    /// `-u` — declare that the user is active.
    pub user_active: bool,
}

impl CaffeinateFlags {
    pub fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        for (enabled, flag) in [
            (self.display, "-d"),
            (self.idle, "-i"),
            (self.disk, "-m"),
            (self.system, "-s"),
            (self.user_active, "-u"),
        ] {
            if enabled {
                args.push(flag.to_string());
            }
        }
        args
    }

    pub fn is_empty(&self) -> bool {
        !(self.display || self.idle || self.disk || self.system || self.user_active)
    }

    /// Column label. `caffeinate` with no assertion flags behaves as `-i`,
    /// so say that rather than showing an empty cell.
    pub fn label(&self) -> String {
        if self.is_empty() {
            return "(-i default)".to_string();
        }
        self.args().join(" ")
    }
}

/// Which "shape" of session this is. Mirrors the mutually exclusive tail of a
/// `caffeinate` command line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    /// Run until killed.
    #[default]
    Indefinite,
    /// `-t <secs>`
    Timeout(u64),
    /// Trailing utility plus its arguments.
    Command(String),
    /// `-w <pid>`
    WaitPid(u32),
}

impl Target {
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Indefinite => Vec::new(),
            Self::Timeout(seconds) => vec!["-t".to_string(), seconds.to_string()],
            Self::Command(command) => utils::split_args(command),
            Self::WaitPid(pid) => vec!["-w".to_string(), pid.to_string()],
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Indefinite => "indefinite".to_string(),
            Self::Timeout(seconds) => utils::format_human(*seconds),
            Self::Command(command) => command.clone(),
            Self::WaitPid(pid) => format!("wait pid {pid}"),
        }
    }

    pub fn kind(&self) -> TargetKind {
        match self {
            Self::Indefinite => TargetKind::Indefinite,
            Self::Timeout(_) => TargetKind::Timeout,
            Self::Command(_) => TargetKind::Command,
            Self::WaitPid(_) => TargetKind::WaitPid,
        }
    }
}

/// The radio-group discriminant, decoupled from `Target` so the form can hold a
/// selected kind while every kind's draft input survives switching between them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TargetKind {
    #[default]
    Indefinite,
    Timeout,
    Command,
    WaitPid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Running,
    Stopped,
    /// Process exited on its own with this code.
    Finished(i32),
    Error(String),
}

impl SessionStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Running => "RUNNING".to_string(),
            Self::Stopped => "STOPPED".to_string(),
            Self::Finished(0) => "DONE".to_string(),
            Self::Finished(code) => format!("EXIT {code}"),
            Self::Error(_) => "ERROR".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaffeineSession {
    pub id: Uuid,
    pub pid: Option<u32>,
    pub name: String,
    pub flags: CaffeinateFlags,
    pub target: Target,
    pub status: SessionStatus,
    pub started_at: DateTime<Local>,
    pub expires_at: Option<DateTime<Local>>,
    /// A `caffeinate` this app did not spawn, discovered by scanning processes.
    ///
    /// Never serialized and never trusted from disk: ownership is recomputed from
    /// the live process table on every scan, the same way the row is created.
    #[serde(skip)]
    pub external: bool,
}

impl CaffeineSession {
    pub fn new(name: String, flags: CaffeinateFlags, target: Target) -> Self {
        Self {
            id: Uuid::new_v4(),
            pid: None,
            name,
            flags,
            target,
            status: SessionStatus::Stopped,
            started_at: Local::now(),
            expires_at: None,
            external: false,
        }
    }

    /// Build a read-only row for a `caffeinate` process owned by someone else.
    pub fn from_external(process: &crate::utils::ExternalProcess) -> Self {
        let (flags, target) = parse_caffeinate_argv(&process.argv);
        let started_at = DateTime::from_timestamp(process.start_time as i64, 0)
            .map(|utc| utc.with_timezone(&Local))
            .unwrap_or_else(Local::now);
        let expires_at = match target {
            Target::Timeout(seconds) => {
                Some(started_at + chrono::Duration::seconds(seconds as i64))
            }
            _ => None,
        };

        Self {
            id: Uuid::new_v4(),
            pid: Some(process.pid),
            name: process
                .parent_name
                .clone()
                .unwrap_or_else(|| CAFFEINATE.to_string()),
            flags,
            target,
            status: SessionStatus::Running,
            started_at,
            expires_at,
            external: true,
        }
    }

    /// Arguments after the program name: assertion flags, then the target tail.
    pub fn args(&self) -> Vec<String> {
        let mut args = self.flags.args();
        args.extend(self.target.args());
        args
    }

    /// Full command line, for the details modal.
    pub fn command_line(&self) -> String {
        let args = self.args();
        if args.is_empty() {
            return CAFFEINATE.to_string();
        }
        format!("{CAFFEINATE} {}", args.join(" "))
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, SessionStatus::Running)
    }

    pub fn elapsed_seconds(&self) -> i64 {
        (Local::now() - self.started_at).num_seconds().max(0)
    }

    /// Seconds left before `-t` fires. `None` for every other target.
    pub fn remaining_seconds(&self) -> Option<i64> {
        let expires_at = self.expires_at?;
        Some((expires_at - Local::now()).num_seconds().max(0))
    }

    /// 0.0..=1.0 completion for `Timeout` targets only.
    pub fn progress(&self) -> Option<f64> {
        let Target::Timeout(total) = self.target else {
            return None;
        };
        if total == 0 {
            return None;
        }
        let remaining = self.remaining_seconds()? as f64;
        Some((1.0 - remaining / total as f64).clamp(0.0, 1.0))
    }
}

/// The `[-disu]` set from `caffeinate(8)`, plus `-m`.
const ASSERTION_LETTERS: &str = "dimsu";

/// Reverse of `CaffeineSession::args`: read an observed argv back into a flag set
/// and a target, so an externally started `caffeinate` displays the same way one
/// of ours does.
///
/// Accepts every form `getopt` does: separate (`-t 300`), attached (`-t300`) and
/// bundled (`-di`). An unrecognised bare word starts the trailing utility.
pub fn parse_caffeinate_argv(argv: &[String]) -> (CaffeinateFlags, Target) {
    let mut flags = CaffeinateFlags::default();
    let mut target = Target::Indefinite;
    let mut arguments = argv.iter().skip(1);

    while let Some(argument) = arguments.next() {
        let letters: Option<&str> = argument.strip_prefix('-');
        match letters {
            // Bundled or single assertion flags: -i, -di, -dimsu.
            Some(letters)
                if !letters.is_empty()
                    && letters
                        .chars()
                        .all(|letter| ASSERTION_LETTERS.contains(letter)) =>
            {
                for letter in letters.chars() {
                    match letter {
                        'd' => flags.display = true,
                        'i' => flags.idle = true,
                        'm' => flags.disk = true,
                        's' => flags.system = true,
                        'u' => flags.user_active = true,
                        _ => {}
                    }
                }
            }
            Some(letters) if letters.starts_with('t') || letters.starts_with('w') => {
                let kind = letters.as_bytes()[0];
                let value = match letters.len() {
                    1 => arguments.next().cloned(),
                    _ => Some(letters[1..].to_string()),
                };
                if let Some(parsed) = value.and_then(|value| value.trim().parse::<u64>().ok()) {
                    target = if kind == b't' {
                        Target::Timeout(parsed)
                    } else {
                        Target::WaitPid(parsed as u32)
                    };
                }
            }
            // Not a flag we know: this is the trailing utility and its arguments.
            _ => {
                let mut parts = vec![argument.clone()];
                parts.extend(arguments.cloned());
                target = Target::Command(parts.join(" "));
                break;
            }
        }
    }

    (flags, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn parses_a_bare_external_caffeinate() {
        // `caffeinate &` from a shell: no flags at all.
        let (flags, target) = parse_caffeinate_argv(&argv("caffeinate"));
        assert!(flags.is_empty());
        assert_eq!(target, Target::Indefinite);
        assert_eq!(flags.label(), "(-i default)");
    }

    #[test]
    fn parses_separate_bundled_and_attached_flags() {
        let (flags, target) = parse_caffeinate_argv(&argv("caffeinate -i -t 300"));
        assert!(flags.idle && !flags.display);
        assert_eq!(target, Target::Timeout(300));

        let (bundled, _) = parse_caffeinate_argv(&argv("caffeinate -dimsu"));
        assert_eq!(
            bundled,
            CaffeinateFlags {
                display: true,
                idle: true,
                disk: true,
                system: true,
                user_active: true
            }
        );

        let (_, attached) = parse_caffeinate_argv(&argv("caffeinate -t3600"));
        assert_eq!(attached, Target::Timeout(3600));
    }

    #[test]
    fn parses_wait_pid_and_trailing_command() {
        let (_, wait) = parse_caffeinate_argv(&argv("caffeinate -i -w 4242"));
        assert_eq!(wait, Target::WaitPid(4242));

        let (flags, command) = parse_caffeinate_argv(&argv("caffeinate -i cargo build --release"));
        assert!(flags.idle);
        assert_eq!(
            command,
            Target::Command("cargo build --release".to_string())
        );
    }

    #[test]
    fn parsing_round_trips_our_own_argv() {
        let session = CaffeineSession::new(
            "Round".into(),
            CaffeinateFlags {
                display: true,
                idle: true,
                system: true,
                ..Default::default()
            },
            Target::Timeout(900),
        );
        let mut observed = vec![CAFFEINATE.to_string()];
        observed.extend(session.args());
        let (flags, target) = parse_caffeinate_argv(&observed);
        assert_eq!(flags, session.flags);
        assert_eq!(target, session.target);
    }

    #[test]
    fn external_rows_are_never_serialized_as_external() {
        let process = crate::utils::ExternalProcess {
            pid: 4242,
            argv: argv("caffeinate -i"),
            parent_name: Some("zsh".to_string()),
            start_time: 1_700_000_000,
        };
        let session = CaffeineSession::from_external(&process);
        assert!(session.external);
        assert_eq!(session.pid, Some(4242));
        assert_eq!(session.name, "zsh");
        assert_eq!(session.status, SessionStatus::Running);

        // `external` is `#[serde(skip)]`, so a round trip must not resurrect it.
        let json = serde_json::to_string(&session).unwrap();
        let restored: CaffeineSession = serde_json::from_str(&json).unwrap();
        assert!(!restored.external);
    }

    #[test]
    fn flag_args_follow_caffeinate_order() {
        let flags = CaffeinateFlags {
            display: true,
            idle: true,
            disk: false,
            system: true,
            user_active: true,
        };
        assert_eq!(flags.args(), ["-d", "-i", "-s", "-u"]);
    }

    #[test]
    fn empty_flags_render_as_default_hint() {
        assert_eq!(CaffeinateFlags::default().label(), "(-i default)");
    }

    #[test]
    fn timeout_target_emits_t_flag_after_assertions() {
        let session = CaffeineSession::new(
            "Movie".into(),
            CaffeinateFlags {
                display: true,
                idle: true,
                ..Default::default()
            },
            Target::Timeout(7200),
        );
        assert_eq!(session.args(), ["-d", "-i", "-t", "7200"]);
        assert_eq!(session.command_line(), "caffeinate -d -i -t 7200");
    }

    #[test]
    fn command_target_appends_utility_argv() {
        let session = CaffeineSession::new(
            "Build".into(),
            CaffeinateFlags {
                idle: true,
                ..Default::default()
            },
            Target::Command("cargo build --release".into()),
        );
        assert_eq!(session.args(), ["-i", "cargo", "build", "--release"]);
    }

    #[test]
    fn wait_pid_target_emits_w_flag() {
        let session = CaffeineSession::new(
            "Wait".into(),
            CaffeinateFlags::default(),
            Target::WaitPid(42),
        );
        assert_eq!(session.args(), ["-w", "42"]);
    }

    #[test]
    fn indefinite_target_has_no_tail() {
        let session = CaffeineSession::new(
            "Forever".into(),
            CaffeinateFlags {
                idle: true,
                ..Default::default()
            },
            Target::Indefinite,
        );
        assert_eq!(session.args(), ["-i"]);
        assert!(session.progress().is_none());
    }

    #[test]
    fn progress_tracks_expiry_window() {
        let mut session = CaffeineSession::new(
            "Half".into(),
            CaffeinateFlags::default(),
            Target::Timeout(100),
        );
        session.expires_at = Some(Local::now() + chrono::Duration::seconds(25));
        let progress = session.progress().expect("timeout target has progress");
        assert!((progress - 0.75).abs() < 0.05, "progress was {progress}");
    }
}
