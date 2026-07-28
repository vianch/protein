//! Rendering. Every draw pass rebuilds `App::regions`, the click map that gives
//! each on-screen control a mouse route to the same `Action` its key produces.

pub mod form;
pub mod help;
pub mod styles;
pub mod table;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, InputMode};

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.regions.clear();

    let area = frame.area();
    frame.render_widget(Block::default().style(styles::background()), area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(area);

    table::draw_header(frame, app, chunks[0]);
    table::draw_table(frame, app, chunks[1]);
    table::draw_footer(frame, app, chunks[2]);

    match app.mode {
        InputMode::Form => form::draw(frame, app, area),
        InputMode::Help => help::draw_help(frame, app, area),
        InputMode::Details => help::draw_details(frame, app, area),
        InputMode::ProcessPicker => {
            form::draw(frame, app, area);
            form::draw_picker(frame, app, area);
        }
        InputMode::Normal => {}
    }
}

/// Centre a `percent_x` x `percent_y` box inside `area`.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

/// Clear the area and draw a rounded modal frame, returning the inner region.
pub fn modal_block(frame: &mut Frame, area: Rect, title: &str, footer: &str) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(styles::border(true))
        .title(format!(" {title} "))
        .title_style(styles::heading())
        .style(styles::background());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !footer.is_empty() && area.height >= 2 {
        let hint_area = Rect {
            x: area.x + 2,
            y: area.y + area.height - 1,
            width: area.width.saturating_sub(4),
            height: 1,
        };
        frame.render_widget(Paragraph::new(footer).style(styles::dim()), hint_area);
    }
    inner
}
