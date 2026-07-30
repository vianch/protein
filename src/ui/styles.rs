//! Colour themes, shared styles and status glyphs.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};

/// The fifteen semantic slots every theme has to fill. Slot names follow
/// Catppuccin's vocabulary because that was the first theme; other palettes map
/// their nearest colour onto each one.
pub struct Theme {
    pub name: &'static str,
    pub base: Color,
    pub mantle: Color,
    pub surface0: Color,
    pub surface1: Color,
    pub overlay0: Color,
    pub subtext0: Color,
    pub text: Color,
    pub green: Color,
    pub blue: Color,
    pub red: Color,
    pub yellow: Color,
    pub peach: Color,
    pub mauve: Color,
    pub lavender: Color,
    pub teal: Color,
}

const MOCHA: Theme = Theme {
    name: "mocha",
    base: Color::Rgb(0x1e, 0x1e, 0x2e),
    mantle: Color::Rgb(0x18, 0x18, 0x25),
    surface0: Color::Rgb(0x31, 0x32, 0x44),
    surface1: Color::Rgb(0x45, 0x47, 0x5a),
    overlay0: Color::Rgb(0x6c, 0x70, 0x86),
    subtext0: Color::Rgb(0xa6, 0xad, 0xc8),
    text: Color::Rgb(0xcd, 0xd6, 0xf4),
    green: Color::Rgb(0xa6, 0xe3, 0xa1),
    blue: Color::Rgb(0x89, 0xb4, 0xfa),
    red: Color::Rgb(0xf3, 0x8b, 0xa8),
    yellow: Color::Rgb(0xf9, 0xe2, 0xaf),
    peach: Color::Rgb(0xfa, 0xb3, 0x87),
    mauve: Color::Rgb(0xcb, 0xa6, 0xf7),
    lavender: Color::Rgb(0xb4, 0xbe, 0xfe),
    teal: Color::Rgb(0x94, 0xe2, 0xd5),
};

/// Shades of Purple — the ANSI palette, with the structural slots (mantle,
/// surfaces, overlay) filled from the theme's editor chrome colours.
const PURPLE: Theme = Theme {
    name: "purple",
    base: Color::Rgb(0x1e, 0x1e, 0x3f),
    mantle: Color::Rgb(0x19, 0x19, 0x35),
    surface0: Color::Rgb(0x2d, 0x2b, 0x55),
    surface1: Color::Rgb(0x4d, 0x4c, 0x7d),
    overlay0: Color::Rgb(0x6e, 0x68, 0xa8),
    subtext0: Color::Rgb(0xa5, 0x99, 0xe9),
    text: Color::Rgb(0xe3, 0xdf, 0xff),
    green: Color::Rgb(0x3a, 0xd9, 0x00),
    blue: Color::Rgb(0x9e, 0xff, 0xff),
    red: Color::Rgb(0xff, 0x62, 0x8c),
    yellow: Color::Rgb(0xfa, 0xd0, 0x00),
    peach: Color::Rgb(0xff, 0x9d, 0x00),
    mauve: Color::Rgb(0xb3, 0x62, 0xff),
    lavender: Color::Rgb(0xfb, 0x94, 0xff),
    teal: Color::Rgb(0x9e, 0xff, 0xff),
};

const THEMES: &[&Theme] = &[&MOCHA, &PURPLE];

static THEME: OnceLock<&'static Theme> = OnceLock::new();
static ASCII: OnceLock<bool> = OnceLock::new();

/// Comma-separated theme names, for `--help`.
pub fn theme_names() -> String {
    THEMES
        .iter()
        .map(|theme| theme.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Select a theme by name. Returns `false` for an unknown name, leaving the
/// default in place — a typo must not stop the app from starting.
pub fn set_theme(name: &str) -> bool {
    match THEMES.iter().find(|theme| theme.name == name) {
        Some(theme) => {
            let _ = THEME.set(theme);
            true
        }
        None => false,
    }
}

pub fn theme() -> &'static Theme {
    THEME.get().copied().unwrap_or(&MOCHA)
}

/// Opt out of box-drawing glyphs. Called once at startup.
pub fn set_ascii(enabled: bool) {
    let _ = ASCII.set(enabled);
}

pub fn ascii() -> bool {
    *ASCII.get().unwrap_or(&false)
}

pub fn background() -> Style {
    Style::default().bg(theme().base).fg(theme().text)
}

pub fn dim() -> Style {
    Style::default().fg(theme().overlay0)
}

pub fn label() -> Style {
    Style::default().fg(theme().subtext0)
}

pub fn heading() -> Style {
    Style::default()
        .fg(theme().mauve)
        .add_modifier(Modifier::BOLD)
}

pub fn border(focused: bool) -> Style {
    if focused {
        Style::default().fg(theme().lavender)
    } else {
        Style::default().fg(theme().surface1)
    }
}

/// The visible focus outline that replaces hover states.
pub fn focused_field() -> Style {
    Style::default()
        .bg(theme().surface0)
        .fg(theme().text)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_row() -> Style {
    Style::default()
        .bg(theme().surface0)
        .add_modifier(Modifier::BOLD)
}

pub fn button(focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(theme().lavender)
            .fg(theme().mantle)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(theme().surface0).fg(theme().text)
    }
}

pub fn error() -> Style {
    Style::default()
        .fg(theme().red)
        .add_modifier(Modifier::BOLD)
}

pub fn hint() -> Style {
    Style::default().fg(theme().yellow)
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
        Style::default()
            .fg(theme().mauve)
            .add_modifier(Modifier::BOLD),
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
            Style::default()
                .fg(theme().green)
                .add_modifier(Modifier::BOLD),
        ),
        SessionStatus::Finished(_) => (
            if is_ascii { "+" } else { "\u{2713}" },
            Style::default().fg(theme().blue),
        ),
        SessionStatus::Error(_) => (
            if is_ascii { "!" } else { "\u{2717}" },
            Style::default()
                .fg(theme().red)
                .add_modifier(Modifier::BOLD),
        ),
        SessionStatus::Stopped => (
            if is_ascii { "-" } else { "\u{25a0}" },
            Style::default().fg(theme().overlay0),
        ),
    }
}
