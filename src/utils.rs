//! Pure formatting helpers plus the small amount of system probing the UI needs.

use std::process::Command;

use sysinfo::{Pid, ProcessRefreshKind, System};

/// Split a user-typed command line into argv.
///
/// Handles single and double quotes, which is what "cargo build" and
/// `say "hello there"` need. Backslash escapes and shell expansion are not
/// supported — `caffeinate` execs the utility directly, it is not a shell, so
/// there is nothing to expand.
pub fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut in_token = false;

    for character in input.chars() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => current.push(character),
            None if character == '\'' || character == '"' => {
                quote = Some(character);
                in_token = true;
            }
            None if character.is_whitespace() => {
                if in_token {
                    args.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            None => {
                current.push(character);
                in_token = true;
            }
        }
    }

    if in_token {
        args.push(current);
    }
    args
}

/// `H:MM:SS` stopwatch, used for both elapsed and remaining time.
///
/// Past a day it switches to `2d 3:04:05`: an external `caffeinate` that has been
/// up for two weeks reads as `329:46:15` otherwise, which nobody can parse.
pub fn format_clock(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let clock = format!("{}:{:02}:{:02}", hours, (seconds % 3600) / 60, seconds % 60);

    if days > 0 {
        return format!("{days}d {clock}");
    }
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// Coarse human duration: `2h 30m`, `45s`, `1d 4h`.
pub fn format_human(seconds: u64) -> String {
    if seconds == 0 {
        return "0s".to_string();
    }

    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let remainder = seconds % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if remainder > 0 && days == 0 && hours == 0 {
        parts.push(format!("{remainder}s"));
    }
    parts.join(" ")
}

/// Unicode block progress bar sized to `width` cells.
pub fn progress_bar(progress: f64, width: usize) -> String {
    let filled = (progress.clamp(0.0, 1.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!(
        "{}{}",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(width - filled)
    )
}

/// Truncate to `width` display cells, adding an ellipsis when it does not fit.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width <= 1 {
        return text.chars().take(width).collect();
    }
    let kept: String = text.chars().take(width - 1).collect();
    format!("{kept}\u{2026}")
}

#[derive(Debug, Clone)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub memory_mb: u64,
}

/// `ps`-equivalent snapshot for the PID picker, sorted by name.
///
/// No CPU column: macOS does not hand a non-root process another process's CPU
/// time, so `cpu_usage()` is 0.0 for everything here and a 0.0% column would
/// only mislead. Name plus memory plus the filter is enough to find a PID.
pub fn list_processes() -> Vec<ProcessEntry> {
    let mut system = System::new();
    system.refresh_processes();

    let mut entries: Vec<ProcessEntry> = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessEntry {
            pid: pid.as_u32(),
            name: process.name().to_string(),
            memory_mb: process.memory() / 1024 / 1024,
        })
        .collect();

    entries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.pid.cmp(&right.pid))
    });
    entries
}

pub fn pid_is_alive(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_process(Pid::from_u32(pid))
}

/// A `caffeinate` process this app did not start.
#[derive(Debug, Clone)]
pub struct ExternalProcess {
    pub pid: u32,
    /// Full argv including the program name.
    pub argv: Vec<String>,
    /// Name of the process that launched it, when still resolvable — the useful
    /// answer to "where did this assertion come from".
    pub parent_name: Option<String>,
    /// Seconds since the Unix epoch.
    pub start_time: u64,
}

/// Scans for `caffeinate` processes.
///
/// Holds its `System` across calls: `refresh_processes` then diffs an existing
/// table instead of rebuilding it, which is what keeps a once-a-second rescan off
/// the UI's critical path.
pub struct ProcessScanner {
    system: System,
}

impl Default for ProcessScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessScanner {
    pub fn new() -> Self {
        Self {
            system: System::new(),
        }
    }

    pub fn scan(&mut self) -> Vec<ExternalProcess> {
        // `everything()` is required, not belt-and-braces: the default refresh
        // leaves `cmd()` empty on macOS, which would show every external session
        // as a flagless `caffeinate` and silently lose its -t/-w target.
        self.system
            .refresh_processes_specifics(ProcessRefreshKind::everything());

        let parent_name = |pid: Option<Pid>| {
            pid.and_then(|pid| self.system.process(pid))
                .map(|process| process.name().to_string())
        };

        let mut found: Vec<ExternalProcess> = self
            .system
            .processes()
            .iter()
            .filter(|(_, process)| process.name() == CAFFEINATE_PROCESS)
            .map(|(pid, process)| ExternalProcess {
                pid: pid.as_u32(),
                argv: process.cmd().to_vec(),
                parent_name: parent_name(process.parent()),
                start_time: process.start_time(),
            })
            .collect();

        found.sort_by_key(|process| process.pid);
        found
    }
}

/// `Process::name()` is the executable name, which is what we match on.
const CAFFEINATE_PROCESS: &str = "caffeinate";

/// True when the machine is on battery, in which case `-s` is a no-op. Used only
/// to surface an inline note — never to block input.
pub fn on_battery_power() -> bool {
    Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout).contains("Now drawing from 'Battery Power'")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_args_respects_quotes() {
        assert_eq!(
            split_args("cargo build --release"),
            ["cargo", "build", "--release"]
        );
        assert_eq!(split_args("say \"hello there\""), ["say", "hello there"]);
        assert_eq!(split_args("  spaced   out  "), ["spaced", "out"]);
        assert!(split_args("").is_empty());
    }

    #[test]
    fn split_args_keeps_empty_quoted_token() {
        assert_eq!(split_args("echo \"\""), ["echo", ""]);
    }

    #[test]
    fn clock_pads_minutes_and_seconds() {
        assert_eq!(format_clock(5025), "1:23:45");
        assert_eq!(format_clock(0), "0:00:00");
        assert_eq!(format_clock(-10), "0:00:00");
        assert_eq!(format_clock(86_399), "23:59:59");
    }

    #[test]
    fn clock_breaks_out_days_past_24_hours() {
        assert_eq!(format_clock(86_400), "1d 0:00:00");
        // 13d 17:24:45 — a real long-lived external caffeinate.
        assert_eq!(format_clock(1_186_785), "13d 17:39:45");
    }

    #[test]
    fn human_duration_drops_zero_units() {
        assert_eq!(format_human(3600), "1h");
        assert_eq!(format_human(7230), "2h");
        assert_eq!(format_human(5400), "1h 30m");
        assert_eq!(format_human(45), "45s");
        assert_eq!(format_human(0), "0s");
        assert_eq!(format_human(90_061), "1d 1h 1m");
    }

    #[test]
    fn progress_bar_fills_proportionally() {
        assert_eq!(progress_bar(0.0, 4), "\u{2591}\u{2591}\u{2591}\u{2591}");
        assert_eq!(progress_bar(1.0, 4), "\u{2588}\u{2588}\u{2588}\u{2588}");
        assert_eq!(progress_bar(0.5, 4), "\u{2588}\u{2588}\u{2591}\u{2591}");
        assert_eq!(progress_bar(9.0, 4).chars().count(), 4);
    }

    #[test]
    fn truncate_adds_ellipsis_only_when_needed() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("truncate me", 5), "trun\u{2026}");
    }
}
