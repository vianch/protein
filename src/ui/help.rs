//! The help modal and the per-session details modal.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::models::{SessionStatus, Target};
use crate::ui::{modal_block, styles};

const KEYS: &[(&str, &str)] = &[
    ("j / \u{2193}", "Move down"),
    ("\u{2191}", "Move up (k is Kill, so vim-up is the arrow)"),
    ("Enter / l", "Session details"),
    ("n", "New session"),
    ("e", "Edit selected"),
    ("k / Ctrl+C", "Kill selected (SIGTERM, SIGKILL after 500ms)"),
    ("Shift+D", "Delete selected (stopped sessions only)"),
    ("Shift+R", "Restart with the same config"),
    ("d", "Duplicate config into a new form"),
    (
        "r / F5 / Ctrl+R",
        "Rescan for external caffeinate processes",
    ),
    ("Tab / Shift+Tab", "Cycle focus: table \u{2194} footer"),
    ("?", "This help"),
    ("q / Esc", "Quit, or close the open modal"),
];

const FORM_KEYS: &[(&str, &str)] = &[
    ("Tab / Shift+Tab", "Next / previous field"),
    ("Space / Enter", "Toggle checkbox, pick radio, press button"),
    ("Ctrl+S", "Save & launch"),
    ("Esc", "Cancel"),
];

const MOUSE: &[&str] = &[
    "Click a table row to select it.",
    "Click a footer button to run its command.",
    "Click a checkbox, radio or button in the form to act on it.",
    "Scroll the wheel over the table, form or PID list to scroll.",
];

pub fn draw_help(frame: &mut Frame, _app: &mut App, area: Rect) {
    let modal = crate::ui::centered_rect(64, 80, area);
    let inner = modal_block(frame, modal, "Help", "any key to close");

    let mut lines = vec![Line::from(Span::styled("Table", styles::heading()))];
    lines.extend(
        KEYS.iter()
            .map(|(key, description)| binding(key, description)),
    );

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Form", styles::heading())));
    lines.extend(
        FORM_KEYS
            .iter()
            .map(|(key, description)| binding(key, description)),
    );

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Mouse", styles::heading())));
    lines.extend(
        MOUSE
            .iter()
            .map(|note| Line::from(Span::styled(format!("  {note}"), styles::label()))),
    );

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "External sessions",
        styles::heading(),
    )));
    lines.extend(
        [
            "\u{25c6} EXTERNAL rows are caffeinate processes protein did not start",
            "(a shell's `caffeinate &`, an app, another terminal). They are found by",
            "scanning every second, show their real PID, and can be killed from here.",
            "Edit and Shift+R are blocked \u{2014} press d to copy the flags instead.",
        ]
        .iter()
        .map(|note| Line::from(Span::styled(format!("  {note}"), styles::label()))),
    );

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  -s only holds on AC power; on battery it is a no-op.",
        styles::dim(),
    )));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn binding<'a>(key: &'a str, description: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {key:<18}"), styles::hint()),
        Span::styled(description, styles::label()),
    ])
}

pub fn draw_details(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(session) = app.selected() else {
        return;
    };
    let modal = crate::ui::centered_rect(66, 60, area);
    let (glyph, glyph_style) = styles::status_glyph(&session.status);

    let mut lines = vec![
        row(
            "Name",
            Span::styled(
                session.name.clone(),
                styles::background().fg(styles::theme().text),
            ),
        ),
        row(
            "Status",
            Span::styled(format!("{glyph} {}", session.status.label()), glyph_style),
        ),
        row(
            "Command",
            Span::styled(
                session.command_line(),
                styles::background().fg(styles::theme().teal),
            ),
        ),
        row(
            "Flags",
            Span::styled(
                session.flags.label(),
                styles::background().fg(styles::theme().peach),
            ),
        ),
        row(
            "Target",
            Span::styled(
                session.target.label(),
                styles::background().fg(styles::theme().peach),
            ),
        ),
        row(
            "PID",
            Span::styled(
                session
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                styles::label(),
            ),
        ),
        row(
            "Started",
            Span::styled(
                session.started_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                styles::label(),
            ),
        ),
    ];

    if let Some(expires_at) = session.expires_at {
        lines.push(row(
            "Expires",
            Span::styled(
                expires_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                styles::label(),
            ),
        ));
    }
    lines.push(row(
        "Elapsed",
        Span::styled(
            crate::utils::format_clock(session.elapsed_seconds()),
            styles::label(),
        ),
    ));
    if let Some(remaining) = session.remaining_seconds() {
        lines.push(row(
            "Remaining",
            Span::styled(crate::utils::format_clock(remaining), styles::label()),
        ));
    }

    if session.external {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Started outside protein. Kill works; edit and restart do not.",
            styles::hint(),
        )));
        lines.push(Line::from(Span::styled(
            "  Press d to copy its flags into a session of your own.",
            styles::dim(),
        )));
    }
    if session.flags.system && app.on_battery {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  -s has no effect while on battery power.",
            styles::hint(),
        )));
    }
    if let Target::Command(command) = &session.target {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  argv: {:?}", crate::utils::split_args(command)),
            styles::dim(),
        )));
    }
    if let SessionStatus::Error(message) = &session.status {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {message}"),
            styles::error(),
        )));
    }

    let inner = modal_block(frame, modal, "Session details", "any key to close");
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn row<'a>(label: &'a str, value: Span<'a>) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {label:<11}"), styles::label()),
        value,
    ])
}
