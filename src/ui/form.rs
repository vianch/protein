//! The new/edit modal and the PID picker it can open.
//!
//! Fields are built as a flat list of lines, each optionally owning a
//! `FormField`. The same list drives rendering, the scroll window and the click
//! map, so a clicked line always focuses the field drawn on it.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use crate::app::{Action, App, FormField, SessionForm};
use crate::models::TargetKind;
use crate::ui::{modal_block, styles};
use crate::utils;

const MODAL_WIDTH_PERCENT: u16 = 60;
const MODAL_HEIGHT_PERCENT: u16 = 70;
const PICKER_WIDTH_PERCENT: u16 = 70;
const PICKER_HEIGHT_PERCENT: u16 = 70;

struct FormLine<'a> {
    line: Line<'a>,
    /// Field owning the whole row.
    field: Option<FormField>,
    /// `(x offset in the row, width, field)` for rows holding several controls.
    inline: Vec<(u16, u16, FormField)>,
}

impl<'a> FormLine<'a> {
    fn text(line: Line<'a>) -> Self {
        Self {
            line,
            field: None,
            inline: Vec::new(),
        }
    }

    fn control(line: Line<'a>, field: FormField) -> Self {
        Self {
            line,
            field: Some(field),
            inline: Vec::new(),
        }
    }

    fn row(line: Line<'a>, inline: Vec<(u16, u16, FormField)>) -> Self {
        Self {
            line,
            field: None,
            inline,
        }
    }

    /// True when this row draws `field`, whether it owns the whole row or is one
    /// of several controls on it.
    fn owns(&self, field: FormField) -> bool {
        self.field == Some(field)
            || self
                .inline
                .iter()
                .any(|(_, _, candidate)| *candidate == field)
    }
}

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(form) = app.form.clone() else {
        return;
    };
    let modal = crate::ui::centered_rect(MODAL_WIDTH_PERCENT, MODAL_HEIGHT_PERCENT, area);
    let inner = modal_block(
        frame,
        modal,
        form.title(),
        "Tab move  \u{2022} Space toggle  \u{2022} Ctrl+S save & launch  \u{2022} Esc cancel",
    );
    // The block's bottom row carries the hint, so keep content off it.
    let body = Rect {
        height: inner.height.saturating_sub(1),
        ..inner
    };
    if body.height == 0 || body.width < 8 {
        return;
    }

    let lines = build_lines(&form, app.on_battery, body.width as usize);
    let max_scroll = (lines.len() as u16).saturating_sub(body.height);
    let mut scroll = form.scroll.min(max_scroll);

    // Keep the focused field on screen. On a short terminal the form is taller
    // than the modal, and without this Tab could move focus to Save & Launch
    // while it stayed scrolled out of sight — a control with no visible state.
    if let Some(focused) = lines
        .iter()
        .position(|entry| entry.owns(form.field))
        .map(|index| index as u16)
    {
        if focused < scroll {
            scroll = focused;
        } else if focused >= scroll + body.height {
            scroll = focused - body.height + 1;
        }
        scroll = scroll.min(max_scroll);
    }

    if let Some(stored) = &mut app.form {
        stored.scroll = scroll;
    }

    for (offset, entry) in lines
        .iter()
        .skip(scroll as usize)
        .take(body.height as usize)
        .enumerate()
    {
        let line_area = Rect {
            x: body.x,
            y: body.y + offset as u16,
            width: body.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(entry.line.clone()), line_area);
        if let Some(field) = entry.field {
            app.register(line_area, Action::FormActivate(field));
        }
        for (offset_x, width, field) in &entry.inline {
            let button_area = Rect {
                x: line_area.x + offset_x,
                y: line_area.y,
                width: *width,
                height: 1,
            };
            app.register(button_area, Action::FormActivate(*field));
        }
    }

    if lines.len() > body.height as usize {
        let mut state = ScrollbarState::new(lines.len()).position(scroll as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(styles::dim())
                .begin_symbol(None)
                .end_symbol(None),
            body,
            &mut state,
        );
    }
}

fn build_lines<'a>(form: &SessionForm, on_battery: bool, width: usize) -> Vec<FormLine<'a>> {
    let mut lines = vec![
        FormLine::text(Line::from(Span::styled("Name", styles::heading()))),
        FormLine::control(
            text_input(
                &form.name,
                "session name",
                form.field == FormField::Name,
                width,
            ),
            FormField::Name,
        ),
        FormLine::text(Line::from("")),
        FormLine::text(Line::from(Span::styled("Assertions", styles::heading()))),
    ];
    for (field, label, checked) in [
        (FormField::FlagDisplay, "Display (-d)", form.flags.display),
        (FormField::FlagIdle, "Idle (-i)", form.flags.idle),
        (FormField::FlagDisk, "Disk (-m)", form.flags.disk),
        (FormField::FlagSystem, "System (-s)", form.flags.system),
        (
            FormField::FlagUserActive,
            "User-active (-u)",
            form.flags.user_active,
        ),
    ] {
        let focused = form.field == field;
        let mut spans = vec![Span::styled(
            format!(" {} {label}", styles::checkbox(checked)),
            if focused {
                styles::focused_field()
            } else {
                styles::label()
            },
        )];
        if field == FormField::FlagSystem && checked && on_battery {
            spans.push(Span::styled("  no effect on battery", styles::hint()));
        }
        if field == FormField::FlagUserActive && checked && form.target_kind != TargetKind::Timeout
        {
            spans.push(Span::styled("  held until exit", styles::dim()));
        }
        lines.push(FormLine::control(Line::from(spans), field));
    }
    if form.flags.is_empty() {
        lines.push(FormLine::text(Line::from(Span::styled(
            "  no assertions selected \u{2014} caffeinate defaults to -i",
            styles::dim(),
        ))));
    }
    lines.push(FormLine::text(Line::from("")));

    lines.push(FormLine::text(Line::from(Span::styled(
        "Target",
        styles::heading(),
    ))));
    for (field, kind, label) in [
        (
            FormField::TargetIndefinite,
            TargetKind::Indefinite,
            "Indefinite",
        ),
        (
            FormField::TargetTimeout,
            TargetKind::Timeout,
            "Timeout (-t)",
        ),
        (FormField::TargetCommand, TargetKind::Command, "Command"),
        (
            FormField::TargetWaitPid,
            TargetKind::WaitPid,
            "Wait for PID (-w)",
        ),
    ] {
        let focused = form.field == field;
        lines.push(FormLine::control(
            Line::from(Span::styled(
                format!(" {} {label}", styles::radio(form.target_kind == kind)),
                if focused {
                    styles::focused_field()
                } else {
                    styles::label()
                },
            )),
            field,
        ));
    }

    if form.target_kind != TargetKind::Indefinite {
        let placeholder = match form.target_kind {
            TargetKind::Timeout => "seconds",
            TargetKind::Command => "utility and arguments",
            TargetKind::WaitPid => "pid",
            TargetKind::Indefinite => "",
        };
        lines.push(FormLine::control(
            text_input(
                form.target_value(),
                placeholder,
                form.field == FormField::TargetValue,
                width,
            ),
            FormField::TargetValue,
        ));
        if let Some(hint) = form.target_hint() {
            lines.push(FormLine::text(Line::from(Span::styled(
                format!("  {hint}"),
                styles::dim(),
            ))));
        }
        if form.target_kind == TargetKind::WaitPid {
            lines.push(FormLine::control(
                Line::from(Span::styled(
                    " [ pick from running processes ] ",
                    styles::button(form.field == FormField::PickProcess),
                )),
                FormField::PickProcess,
            ));
        }
    }

    lines.push(FormLine::text(Line::from("")));
    if let Some(error) = &form.error {
        lines.push(FormLine::text(Line::from(Span::styled(
            format!(" {error}"),
            styles::error(),
        ))));
    }

    // Buttons share one row, so each gets its own click region measured as the
    // row is built.
    let mut buttons = Vec::new();
    let mut regions = Vec::new();
    let mut cursor: u16 = 0;
    for (field, label) in [
        (FormField::SaveAndLaunch, "[ Save & Launch ]"),
        (FormField::SaveOnly, "[ Save Only ]"),
        (FormField::Cancel, "[ Cancel ]"),
    ] {
        let text = format!(" {label} ");
        let text_width = text.chars().count() as u16;
        buttons.push(Span::styled(text, styles::button(form.field == field)));
        regions.push((cursor, text_width, field));
        cursor += text_width;
        buttons.push(Span::raw(" "));
        cursor += 1;
    }
    lines.push(FormLine::row(Line::from(buttons), regions));

    lines
}

/// One-line text input with a block cursor when focused.
fn text_input<'a>(value: &str, placeholder: &str, focused: bool, width: usize) -> Line<'a> {
    let inner_width = width.saturating_sub(4).max(4);
    let style = if focused {
        styles::focused_field()
    } else {
        styles::background().fg(styles::TEXT)
    };

    if value.is_empty() && !focused {
        return Line::from(Span::styled(
            format!(
                " {:<inner_width$} ",
                utils::truncate(placeholder, inner_width)
            ),
            styles::dim(),
        ));
    }

    // Keep the tail visible while typing past the field width.
    let visible: String = {
        let characters: Vec<char> = value.chars().collect();
        let start = characters
            .len()
            .saturating_sub(inner_width.saturating_sub(1));
        characters[start..].iter().collect()
    };
    let cursor = if focused { "\u{2588}" } else { "" };
    let padded = format!("{visible}{cursor}");
    Line::from(Span::styled(
        format!(" {:<inner_width$} ", utils::truncate(&padded, inner_width)),
        style,
    ))
}

pub fn draw_picker(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(picker) = &app.picker else {
        return;
    };
    let modal = crate::ui::centered_rect(PICKER_WIDTH_PERCENT, PICKER_HEIGHT_PERCENT, area);
    let inner = modal_block(
        frame,
        modal,
        "Pick a process",
        "Type to filter  \u{2022} \u{2191}/\u{2193} move  \u{2022} Enter select  \u{2022} Esc back",
    );
    let body = Rect {
        height: inner.height.saturating_sub(1),
        ..inner
    };
    if body.height < 3 {
        return;
    }

    let filter_line = Line::from(vec![
        Span::styled(" filter ", styles::label()),
        Span::styled(
            format!("{}\u{2588}", picker.filter),
            styles::focused_field(),
        ),
    ]);
    frame.render_widget(Paragraph::new(filter_line), Rect { height: 1, ..body });

    let header = Line::from(Span::styled(
        format!(" {:>7}  {:<40} {:>9}", "PID", "NAME", "MEM"),
        styles::label(),
    ));
    frame.render_widget(
        Paragraph::new(header),
        Rect {
            y: body.y + 1,
            height: 1,
            ..body
        },
    );

    let list_area = Rect {
        y: body.y + 2,
        height: body.height.saturating_sub(2),
        ..body
    };
    let visible = picker.visible();
    let capacity = list_area.height as usize;
    // Keep the selection inside the window without storing a separate offset.
    let offset = picker.selected.saturating_sub(capacity.saturating_sub(1));

    let mut regions = Vec::new();
    for (row, entry) in visible.iter().skip(offset).take(capacity).enumerate() {
        let index = offset + row;
        let selected = index == picker.selected;
        let line = Line::from(Span::styled(
            format!(
                " {:>7}  {:<40} {:>6} MB",
                entry.pid,
                utils::truncate(&entry.name, 40),
                entry.memory_mb
            ),
            if selected {
                styles::selected_row().fg(styles::TEXT)
            } else {
                styles::background().fg(styles::SUBTEXT0)
            },
        ));
        let row_area = Rect {
            x: list_area.x,
            y: list_area.y + row as u16,
            width: list_area.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(line), row_area);
        regions.push((row_area, index));
    }

    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("  no matching process", styles::dim())),
            list_area,
        );
    }

    if visible.len() > capacity {
        let mut state = ScrollbarState::new(visible.len()).position(picker.selected);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(styles::dim())
                .begin_symbol(None)
                .end_symbol(None),
            list_area,
            &mut state,
        );
    }

    // Registering needs `&mut app`, so it happens once every read of `picker` is
    // done with.
    for (row_area, index) in regions {
        app.register(row_area, Action::PickerSelect(index));
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use crate::app::{Action, App, FormField};

    /// Render a frame and return the screen as lines of text.
    fn screen(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|row| {
                (0..width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn shows(lines: &[String], needle: &str) -> bool {
        lines.iter().any(|line| line.contains(needle))
    }

    /// With a target that takes a value the form is taller than the modal, so the
    /// buttons sit past the fold. Focusing one must scroll it into view.
    #[test]
    fn focused_control_is_scrolled_into_view() {
        let mut app = App::new();
        app.sessions.clear();
        app.dispatch(Action::NewSession);
        app.dispatch(Action::FormActivate(FormField::TargetTimeout));
        for character in "7200".chars() {
            app.dispatch(Action::FormChar(character));
        }

        // 30 rows leaves an 18-row modal body for 19 rows of form.
        let before = screen(&mut app, 110, 30);
        assert!(shows(&before, "7200 = 2h"), "the value hint renders");
        assert!(
            !shows(&before, "Save & Launch"),
            "buttons start below the fold, otherwise this test proves nothing"
        );

        app.dispatch(Action::FormActivate(FormField::SaveAndLaunch));
        let after = screen(&mut app, 110, 30);
        assert!(
            shows(&after, "Save & Launch"),
            "focusing a control past the fold must scroll it into view"
        );
    }

    /// The click map is rebuilt every frame; a control that is drawn must be
    /// clickable, and one scrolled out of view must not be.
    #[test]
    fn click_map_only_covers_drawn_controls() {
        let mut app = App::new();
        app.sessions.clear();
        app.dispatch(Action::NewSession);
        app.dispatch(Action::FormActivate(FormField::TargetTimeout));
        // The digits matter: they add the "7200 = 2h" hint line, which is what
        // pushes the form past the modal body and the buttons off-screen.
        for character in "7200".chars() {
            app.dispatch(Action::FormChar(character));
        }
        let _ = screen(&mut app, 110, 30);

        let registered: Vec<FormField> = app
            .regions
            .iter()
            .filter_map(|(_, action)| match action {
                Action::FormActivate(field) => Some(*field),
                _ => None,
            })
            .collect();

        assert!(registered.contains(&FormField::FlagIdle));
        assert!(registered.contains(&FormField::TargetTimeout));
        assert!(
            !registered.contains(&FormField::SaveAndLaunch),
            "an off-screen button must not be clickable"
        );
    }
}
