//! Header bar, session table and footer button row.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table,
};
use ratatui::Frame;

use crate::app::{Action, App, Focus, FOOTER_BUTTONS};
use crate::models::SessionStatus;
use crate::ui::styles;
use crate::utils;

/// Cells reserved for the inline progress bar in the Time column.
const BAR_WIDTH: usize = 10;
const INDEX_WIDTH: u16 = 5;
const NAME_MIN: u16 = 12;
const FLAGS_WIDTH: u16 = 14;
const TARGET_WIDTH: u16 = 18;
const PID_WIDTH: u16 = 7;
const STATUS_WIDTH: u16 = 11;
const TIME_WIDTH: u16 = 19;

pub fn draw_header(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(styles::border(false))
        .style(styles::background());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let total = app.sessions.len();
    let running = app.running_count();
    let mut spans = vec![
        Span::styled("protein", styles::heading()),
        Span::styled("  |  ", styles::dim()),
        Span::styled(
            format!("{total} session{}", if total == 1 { "" } else { "s" }),
            styles::label(),
        ),
        Span::styled("  |  ", styles::dim()),
        Span::styled(format!("{running} running"), styles::label()),
    ];
    let external = app.external_count();
    if external > 0 {
        spans.push(Span::styled("  |  ", styles::dim()));
        spans.push(Span::styled(
            format!("{external} external"),
            Style::default().fg(styles::theme().mauve),
        ));
    }
    spans.push(Span::styled("  |  ", styles::dim()));
    spans.push(Span::styled("? for help", styles::dim()));
    if app.on_battery {
        spans.push(Span::styled("  |  ", styles::dim()));
        spans.push(Span::styled("on battery", styles::hint()));
    }

    // The status one-liner takes what the title bar does not need, so a long
    // header (counts plus battery note) is never the thing that gets clipped.
    let title = Line::from(spans);
    let title_width = (title.width() as u16 + 2).min(inner.width);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(title_width), Constraint::Min(0)])
        .split(inner);

    frame.render_widget(Paragraph::new(title), columns[0]);
    app.register(area, Action::SetFocus(Focus::Table));

    if let Some((text, is_error)) = &app.message {
        let style = if *is_error {
            styles::error()
        } else {
            styles::label()
        };
        let width = columns[1].width as usize;
        frame.render_widget(
            Paragraph::new(Span::styled(utils::truncate(text, width), style))
                .alignment(Alignment::Right),
            columns[1],
        );
    }
}

pub fn draw_table(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Table;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(styles::border(focused))
        .title(" sessions ")
        .title_style(styles::label())
        .style(styles::background());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.sessions.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("No sessions yet.", styles::label())),
            Line::from(Span::styled(
                "Press n (or click [N]ew) to build one.",
                styles::dim(),
            )),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(empty, inner);
        return;
    }

    let selected = app.table_state.selected().unwrap_or(0);
    let rows: Vec<Row> = app
        .sessions
        .iter()
        .enumerate()
        .map(|(index, session)| {
            // An external session is Running, but its Status cell says who owns it
            // — that is the more useful fact about a row you cannot edit.
            let (glyph, glyph_style, status_label) = if session.external {
                let (glyph, style) = styles::external_glyph();
                (glyph, style, "EXTERNAL".to_string())
            } else {
                let (glyph, style) = styles::status_glyph(&session.status);
                (glyph, style, session.status.label())
            };
            let marker = if index == selected {
                styles::selection_marker()
            } else {
                "  "
            };

            Row::new(vec![
                Cell::from(Span::styled(
                    format!("{marker}{}", index + 1),
                    if index == selected {
                        styles::heading()
                    } else {
                        styles::dim()
                    },
                )),
                Cell::from(Span::styled(
                    utils::truncate(&session.name, inner.width as usize / 3),
                    if session.is_running() {
                        styles::background().fg(styles::theme().text)
                    } else {
                        styles::label()
                    },
                )),
                Cell::from(Span::styled(
                    session.flags.label(),
                    styles::background().fg(styles::theme().teal),
                )),
                Cell::from(Span::styled(
                    utils::truncate(&session.target.label(), TARGET_WIDTH as usize),
                    styles::background().fg(styles::theme().peach),
                )),
                Cell::from(Span::styled(
                    session
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    if session.pid.is_some() {
                        styles::background().fg(styles::theme().subtext0)
                    } else {
                        styles::dim()
                    },
                )),
                Cell::from(Line::from(vec![
                    Span::styled(format!("{glyph} "), glyph_style),
                    Span::styled(status_label, glyph_style),
                ])),
                Cell::from(time_cell(session)),
            ])
        })
        .collect();

    let header = Row::new(vec![
        "  #", "Name", "Flags", "Target", "PID", "Status", "Time",
    ])
    .style(styles::label())
    .bottom_margin(0);

    let widths = [
        Constraint::Length(INDEX_WIDTH),
        Constraint::Min(NAME_MIN),
        Constraint::Length(FLAGS_WIDTH),
        Constraint::Length(TARGET_WIDTH),
        Constraint::Length(PID_WIDTH),
        Constraint::Length(STATUS_WIDTH),
        Constraint::Length(TIME_WIDTH),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(styles::selected_row())
        .column_spacing(1);

    frame.render_stateful_widget(table, inner, &mut app.table_state);

    // One click region per visible row, so a click selects exactly that row.
    let first_visible = app.table_state.offset();
    let body_top = inner.y + 1;
    let visible_rows = inner.height.saturating_sub(1);
    for offset in 0..visible_rows {
        let index = first_visible + offset as usize;
        if index >= app.sessions.len() {
            break;
        }
        let row_area = Rect {
            x: inner.x,
            y: body_top + offset,
            width: inner.width,
            height: 1,
        };
        app.register(row_area, Action::SelectIndex(index));
    }

    if app.sessions.len() > visible_rows as usize {
        let mut scrollbar_state = ScrollbarState::new(app.sessions.len()).position(selected);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(styles::dim())
                .begin_symbol(None)
                .end_symbol(None),
            inner,
            &mut scrollbar_state,
        );
    }
}

/// Elapsed clock, or a progress bar plus remaining time for `-t` sessions.
fn time_cell(session: &crate::models::CaffeineSession) -> Line<'static> {
    if !session.is_running() {
        if let SessionStatus::Finished(_) = session.status {
            return Line::from(Span::styled(
                utils::format_clock(session.elapsed_seconds()),
                styles::dim(),
            ));
        }
        return Line::from(Span::styled("-", styles::dim()));
    }

    match session.progress() {
        Some(progress) => {
            let remaining = session.remaining_seconds().unwrap_or(0);
            let colour = if progress > 0.9 {
                styles::theme().yellow
            } else {
                styles::theme().green
            };
            Line::from(vec![
                Span::styled(
                    utils::progress_bar(progress, BAR_WIDTH),
                    styles::background().fg(colour),
                ),
                Span::raw(" "),
                Span::styled(utils::format_clock(remaining), styles::label()),
            ])
        }
        None => {
            // Indefinite, Command and WaitPid have no end date to count down to.
            Line::from(Span::styled(
                utils::format_clock(session.elapsed_seconds()),
                styles::label(),
            ))
        }
    }
}

pub fn draw_footer(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Footer;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(styles::border(focused))
        .style(styles::background());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = Vec::new();
    let mut cursor = inner.x;
    let mut button_areas = Vec::new();

    for (index, (label, _)) in FOOTER_BUTTONS.iter().enumerate() {
        let text = format!(" {label} ");
        let width = text.chars().count() as u16;
        if cursor + width > inner.x + inner.width {
            break;
        }
        let is_active = focused && index == app.footer_index;
        spans.push(Span::styled(text, styles::button(is_active)));
        button_areas.push((
            Rect {
                x: cursor,
                y: inner.y,
                width,
                height: inner.height.max(1),
            },
            index,
        ));
        cursor += width;

        if index + 1 < FOOTER_BUTTONS.len() {
            spans.push(Span::raw(" "));
            cursor += 1;
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    for (button_area, index) in button_areas {
        app.register(button_area, Action::ActivateFooter(index));
    }
}
