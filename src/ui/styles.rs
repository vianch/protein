//! Catppuccin Mocha palette, shared styles and status glyphs.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};

pub const BASE: Color = Color::Rgb(0x1e, 0x1e, 0x2e);
pub const MANTLE: Color = Color::Rgb(0x18, 0x18, 0x25);
pub const SURFACE0: Color = Color::Rgb(0x31, 0x32, 0x44);
pub const SURFACE1: Color = Color::Rgb(0x45, 0x47, 0x5a);
pub const OVERLAY0: Color = Color::Rgb(0x6c, 0x70, 0x86);
pub const SUBTEXT0: Color = Color::Rgb(0xa6, 0xad, 0xc8);
pub const TEXT: Color = Color::Rgb(0xcd, 0xd6, 0xf4);
pub const GREEN: Color = Color::Rgb(0xa6, 0xe3, 0xa1);
pub const BLUE: Color = Color::Rgb(0x89, 0xb4, 0xfa);
pub const RED: Color = Color::Rgb(0xf3, 0x8b, 0xa8);
pub const YELLOW: Color = Color::Rgb(0xf9, 0xe2, 0xaf);
pub const PEACH: Color = Color::Rgb(0xfa, 0xb3, 0x87);
pub const MAUVE: Color = Color::Rgb(0xcb, 0xa6, 0xf7);
pub const LAVENDER: Color = Color::Rgb(0xb4, 0xbe, 0xfe);
pub const TEAL: Color = Color::Rgb(0x94, 0xe2, 0xd5);

static ASCII: OnceLock<bool> = OnceLock::new();

/// Opt out of box-drawing glyphs. Called once at startup.
pub fn set_ascii(enabled: bool) {
    let _ = ASCII.set(enabled);
}

pub fn ascii() -> bool {
    *ASCII.get().unwrap_or(&false)
}

pub fn background() -> Style {
    Style::default().bg(BASE).fg(TEXT)
}

pub fn dim() -> Style {
    Style::default().fg(OVERLAY0)
}

pub fn label() -> Style {
    Style::default().fg(SUBTEXT0)
}

pub fn heading() -> Style {
    Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)
}

pub fn border(focused: bool) -> Style {
    if focused {
        Style::default().fg(LAVENDER)
    } else {
        Style::default().fg(SURFACE1)
    }
}

/// The visible focus outline that replaces hover states.
pub fn focused_field() -> Style {
    Style::default()
        .bg(SURFACE0)
        .fg(TEXT)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_row() -> Style {
    Style::default().bg(SURFACE0).add_modifier(Modifier::BOLD)
}

pub fn button(focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(LAVENDER)
            .fg(MANTLE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(SURFACE0).fg(TEXT)
    }
}

pub fn error() -> Style {
    Style::default().fg(RED).add_modifier(Modifier::BOLD)
}

pub fn hint() -> Style {
    Style::default().fg(YELLOW)
}

pub fn selection_marker() -> &'static str {
    if ascii() {
        "> "
    } else {
        "\u{25b6} "
    }
}

pub fn checkbox(checked: bool) -> &'static str {
    match (checked, ascii()) {
        (true, false) => "[\u{00d7}]",
        (false, false) => "[ ]",
        (true, true) => "[x]",
        (false, true) => "[ ]",
    }
}

pub fn radio(selected: bool) -> &'static str {
    match (selected, ascii()) {
        (true, false) => "(\u{25cf})",
        (false, false) => "( )",
        (true, true) => "(*)",
        (false, true) => "( )",
    }
}

/// Glyph + colour for a `caffeinate` this app did not start.
pub fn external_glyph() -> (&'static str, Style) {
    (
        if ascii() { "=" } else { "\u{25c6}" },
        Style::default().fg(MAUVE).add_modifier(Modifier::BOLD),
    )
}

/// Glyph + colour for a status. Kept together so the table and the details
/// modal cannot disagree.
pub fn status_glyph(status: &crate::models::SessionStatus) -> (&'static str, Style) {
    use crate::models::SessionStatus;
    let is_ascii = ascii();
    match status {
        SessionStatus::Running => (
            if is_ascii { "*" } else { "\u{25cf}" },
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ),
        SessionStatus::Finished(_) => (
            if is_ascii { "+" } else { "\u{2713}" },
            Style::default().fg(BLUE),
        ),
        SessionStatus::Error(_) => (
            if is_ascii { "!" } else { "\u{2717}" },
            Style::default().fg(RED).add_modifier(Modifier::BOLD),
        ),
        SessionStatus::Stopped => (
            if is_ascii { "-" } else { "\u{25a0}" },
            Style::default().fg(OVERLAY0),
        ),
    }
}
