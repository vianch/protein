//! Application state and the single reducer every input path funnels through.
//!
//! Keys, footer clicks, row clicks and form clicks all resolve to an `Action`
//! first, so every command has both a keyboard and a mouse route by
//! construction rather than by discipline.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use uuid::Uuid;

use crate::config;
use crate::daemon::Daemon;
use crate::models::{CaffeinateFlags, CaffeineSession, SessionStatus, Target, TargetKind};
use crate::utils::{self, ProcessEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Form,
    Help,
    Details,
    ProcessPicker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Table,
    Footer,
    Form,
}

/// Every state transition in the app. `Copy` so the footer table can be a
/// `const` and so mouse hit-testing can store actions by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,

    SelectNext,
    SelectPrevious,
    SelectIndex(usize),

    FocusNext,
    FocusPrevious,
    SetFocus(Focus),
    FooterNext,
    FooterPrevious,
    ActivateFooter(usize),

    ShowHelp,
    ShowDetails,
    CloseOverlay,

    NewSession,
    EditSelected,
    DuplicateSelected,
    KillSelected,
    /// Drop the selected row. A stopped session has no process to kill, so
    /// without this it can never leave the list.
    DeleteSelected,
    RestartSelected,
    LaunchSelected,
    /// Rescan the process table for externally started `caffeinate` sessions.
    Refresh,

    /// Focus a field and, for anything that is not a text input, act on it —
    /// the click equivalent of focus-then-Space.
    FormActivate(FormField),
    FormNextField,
    FormPreviousField,
    FormToggle,
    FormChar(char),
    FormBackspace,
    FormScroll(i8),
    /// `true` also launches the session.
    FormSubmit(bool),
    FormCancel,

    OpenProcessPicker,
    PickerNext,
    PickerPrevious,
    PickerSelect(usize),
    PickerConfirm,
    PickerChar(char),
    PickerBackspace,
    PickerClose,

    Tick,
    Nothing,
}

/// Footer buttons. One list drives the rendered labels, the keyboard focus ring
/// and the click targets, so they cannot drift apart.
pub const FOOTER_BUTTONS: &[(&str, Action)] = &[
    ("[N]ew", Action::NewSession),
    ("[E]dit", Action::EditSelected),
    ("[K]ill", Action::KillSelected),
    ("[Shift+D]elete", Action::DeleteSelected),
    ("[Shift+R]estart", Action::RestartSelected),
    ("[D]uplicate", Action::DuplicateSelected),
    ("[R]efresh", Action::Refresh),
    ("[Q]uit", Action::Quit),
    ("[?]Help", Action::ShowHelp),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Name,
    FlagDisplay,
    FlagIdle,
    FlagDisk,
    FlagSystem,
    FlagUserActive,
    TargetIndefinite,
    TargetTimeout,
    TargetCommand,
    TargetWaitPid,
    TargetValue,
    PickProcess,
    SaveAndLaunch,
    SaveOnly,
    Cancel,
}

impl FormField {
    /// Tab order. `TargetValue` and `PickProcess` are filtered out when the
    /// selected target has no value to type.
    const ORDER: &'static [FormField] = &[
        FormField::Name,
        FormField::FlagDisplay,
        FormField::FlagIdle,
        FormField::FlagDisk,
        FormField::FlagSystem,
        FormField::FlagUserActive,
        FormField::TargetIndefinite,
        FormField::TargetTimeout,
        FormField::TargetCommand,
        FormField::TargetWaitPid,
        FormField::TargetValue,
        FormField::PickProcess,
        FormField::SaveAndLaunch,
        FormField::SaveOnly,
        FormField::Cancel,
    ];

    fn is_available(self, kind: TargetKind) -> bool {
        match self {
            FormField::TargetValue => kind != TargetKind::Indefinite,
            FormField::PickProcess => kind == TargetKind::WaitPid,
            _ => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionForm {
    /// `Some` when editing an existing session, `None` for new/duplicate.
    pub editing: Option<Uuid>,
    pub name: String,
    pub flags: CaffeinateFlags,
    pub target_kind: TargetKind,
    /// Each kind keeps its own draft so toggling radios is not destructive.
    pub timeout_input: String,
    pub command_input: String,
    pub pid_input: String,
    pub field: FormField,
    pub error: Option<String>,
    pub scroll: u16,
}

impl SessionForm {
    pub fn blank() -> Self {
        Self {
            editing: None,
            name: String::new(),
            flags: CaffeinateFlags {
                idle: true,
                ..Default::default()
            },
            target_kind: TargetKind::Indefinite,
            timeout_input: String::new(),
            command_input: String::new(),
            pid_input: String::new(),
            field: FormField::Name,
            error: None,
            scroll: 0,
        }
    }

    pub fn from_session(session: &CaffeineSession, editing: bool) -> Self {
        let mut form = Self::blank();
        form.editing = editing.then_some(session.id);
        form.name = if editing {
            session.name.clone()
        } else {
            format!("{} copy", session.name)
        };
        form.flags = session.flags;
        form.target_kind = session.target.kind();
        match &session.target {
            Target::Indefinite => {}
            Target::Timeout(seconds) => form.timeout_input = seconds.to_string(),
            Target::Command(command) => form.command_input = command.clone(),
            Target::WaitPid(pid) => form.pid_input = pid.to_string(),
        }
        form
    }

    pub fn title(&self) -> &'static str {
        if self.editing.is_some() {
            "Edit session"
        } else {
            "New session"
        }
    }

    /// The input backing `FormField::TargetValue` for the selected kind.
    pub fn target_value(&self) -> &str {
        match self.target_kind {
            TargetKind::Indefinite => "",
            TargetKind::Timeout => &self.timeout_input,
            TargetKind::Command => &self.command_input,
            TargetKind::WaitPid => &self.pid_input,
        }
    }

    fn target_value_mut(&mut self) -> Option<&mut String> {
        match self.target_kind {
            TargetKind::Indefinite => None,
            TargetKind::Timeout => Some(&mut self.timeout_input),
            TargetKind::Command => Some(&mut self.command_input),
            TargetKind::WaitPid => Some(&mut self.pid_input),
        }
    }

    /// Inline hint under the value input: unit help, not an error.
    pub fn target_hint(&self) -> Option<String> {
        match self.target_kind {
            TargetKind::Timeout => {
                let seconds: u64 = self.timeout_input.parse().ok()?;
                Some(format!("{} = {}", seconds, utils::format_human(seconds)))
            }
            TargetKind::Command => Some("caffeinate exits when the utility does".to_string()),
            TargetKind::WaitPid => Some("caffeinate exits when that process does".to_string()),
            TargetKind::Indefinite => None,
        }
    }

    pub fn fields_in_order(&self) -> Vec<FormField> {
        FormField::ORDER
            .iter()
            .copied()
            .filter(|field| field.is_available(self.target_kind))
            .collect()
    }

    fn step_field(&mut self, delta: i32) {
        let order = self.fields_in_order();
        let current = order
            .iter()
            .position(|field| *field == self.field)
            .unwrap_or(0) as i32;
        let length = order.len() as i32;
        let next = (current + delta).rem_euclid(length) as usize;
        self.field = order[next];
    }

    /// Only Name and the value input accept typed characters.
    fn is_text_field(&self) -> bool {
        matches!(self.field, FormField::Name | FormField::TargetValue)
    }

    fn build_target(&self) -> Result<Target, String> {
        match self.target_kind {
            TargetKind::Indefinite => Ok(Target::Indefinite),
            TargetKind::Timeout => {
                let seconds: u64 = self
                    .timeout_input
                    .trim()
                    .parse()
                    .map_err(|_| "Timeout must be a whole number of seconds".to_string())?;
                if seconds == 0 {
                    return Err("Timeout must be greater than 0".to_string());
                }
                Ok(Target::Timeout(seconds))
            }
            TargetKind::Command => {
                let command = self.command_input.trim();
                if command.is_empty() {
                    return Err("Command cannot be empty".to_string());
                }
                if utils::split_args(command).is_empty() {
                    return Err("Command must name a utility to run".to_string());
                }
                Ok(Target::Command(command.to_string()))
            }
            TargetKind::WaitPid => {
                let pid: u32 = self
                    .pid_input
                    .trim()
                    .parse()
                    .map_err(|_| "PID must be a positive number".to_string())?;
                Ok(Target::WaitPid(pid))
            }
        }
    }

    /// `launching` adds the checks that only matter for a live process.
    pub fn validate(&self, launching: bool) -> Result<(String, CaffeinateFlags, Target), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Name is required".to_string());
        }
        let target = self.build_target()?;
        if launching {
            if let Target::WaitPid(pid) = target {
                if !utils::pid_is_alive(pid) {
                    return Err(format!("No live process with PID {pid}"));
                }
            }
        }
        Ok((name.to_string(), self.flags, target))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessPicker {
    pub entries: Vec<ProcessEntry>,
    pub filter: String,
    pub selected: usize,
}

impl ProcessPicker {
    pub fn open() -> Self {
        Self {
            entries: utils::list_processes(),
            filter: String::new(),
            selected: 0,
        }
    }

    pub fn visible(&self) -> Vec<&ProcessEntry> {
        let needle = self.filter.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                needle.is_empty()
                    || entry.name.to_lowercase().contains(&needle)
                    || entry.pid.to_string().starts_with(&needle)
            })
            .collect()
    }

    fn clamp(&mut self) {
        let count = self.visible().len();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }
}

pub struct App {
    pub sessions: Vec<CaffeineSession>,
    pub mode: InputMode,
    pub form: Option<SessionForm>,
    pub table_state: TableState,
    pub focus: Focus,

    pub daemon: Daemon,
    pub picker: Option<ProcessPicker>,
    pub footer_index: usize,
    pub on_battery: bool,
    /// Transient one-liner in the header. `true` means render it as an error.
    pub message: Option<(String, bool)>,
    pub should_quit: bool,
    /// Clickable regions recorded by the last render, in draw order.
    pub regions: Vec<(Rect, Action)>,
    scanner: utils::ProcessScanner,
}

impl App {
    pub fn new() -> Self {
        let sessions = config::load();
        let mut app = Self {
            sessions,
            mode: InputMode::Normal,
            form: None,
            table_state: TableState::default(),
            focus: Focus::Table,
            daemon: Daemon::new(),
            picker: None,
            footer_index: 0,
            on_battery: utils::on_battery_power(),
            message: None,
            should_quit: false,
            regions: Vec::new(),
            scanner: utils::ProcessScanner::new(),
        };
        // Adopt anything already holding an assertion before the first frame, so
        // the list is never a partial picture of what is keeping the Mac awake.
        app.sync_external();
        if !app.sessions.is_empty() {
            app.table_state.select(Some(0));
        }
        app
    }

    /// Reconcile the discovered `caffeinate` processes with the table.
    ///
    /// Runs on every tick and on explicit refresh. Rows are added and removed but
    /// never rebuilt, so ids — and therefore the selection — stay put across scans.
    fn sync_external(&mut self) {
        let live = self.scanner.scan();
        self.reconcile_external(&live);
    }

    /// The pure half of `sync_external`: given the live process list, add and drop
    /// external rows. No I/O, so it is directly testable.
    fn reconcile_external(&mut self, live: &[utils::ExternalProcess]) {
        let selected_id = self.selected().map(|session| session.id);
        let mut removed = Vec::new();
        self.sessions.retain(|session| {
            if !session.external {
                return true;
            }
            let still_alive = live.iter().any(|process| Some(process.pid) == session.pid);
            if !still_alive {
                removed.push(session.id);
            }
            still_alive
        });
        for id in removed {
            self.daemon.forget(id);
        }

        for process in live {
            // A PID we already track is either ours or an external row we have.
            if self
                .sessions
                .iter()
                .any(|session| session.pid == Some(process.pid))
            {
                continue;
            }
            self.sessions.push(CaffeineSession::from_external(process));
        }

        // Your own sessions stay at the top; a machine with six stray
        // `caffeinate`s must not bury them. Stable, so order within each group is
        // whatever it already was.
        self.sessions.sort_by_key(|session| session.external);

        // Rows move as externals come and go, so the selection follows the session
        // it was on, not the index it happened to sit at.
        if let Some(id) = selected_id {
            if let Some(index) = self.index_of(id) {
                self.table_state.select(Some(index));
            }
        }
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        match self.table_state.selected() {
            Some(_) if self.sessions.is_empty() => self.table_state.select(None),
            Some(index) if index >= self.sessions.len() => {
                self.table_state.select(Some(self.sessions.len() - 1));
            }
            None if !self.sessions.is_empty() => self.table_state.select(Some(0)),
            _ => {}
        }
    }

    pub fn external_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|session| session.external)
            .count()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.table_state
            .selected()
            .filter(|index| *index < self.sessions.len())
    }

    pub fn selected(&self) -> Option<&CaffeineSession> {
        self.selected_index().map(|index| &self.sessions[index])
    }

    pub fn running_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|session| session.is_running())
            .count()
    }

    pub fn register(&mut self, area: Rect, action: Action) {
        self.regions.push((area, action));
    }

    /// Topmost hit wins, so modals shadow the table underneath them.
    fn action_at(&self, column: u16, row: u16) -> Option<Action> {
        self.regions
            .iter()
            .rev()
            .find(|(area, _)| {
                column >= area.x
                    && column < area.x + area.width
                    && row >= area.y
                    && row < area.y + area.height
            })
            .map(|(_, action)| *action)
    }

    // ---------------------------------------------------------------- input

    pub fn on_key(&mut self, key: KeyEvent) {
        let action = self.action_for_key(key);
        self.dispatch(action);
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        let action = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                match self.action_at(mouse.column, mouse.row) {
                    Some(action) => action,
                    None => return,
                }
            }
            MouseEventKind::ScrollDown => self.scroll_action(1),
            MouseEventKind::ScrollUp => self.scroll_action(-1),
            _ => return,
        };
        self.dispatch(action);
    }

    fn scroll_action(&self, delta: i8) -> Action {
        match self.mode {
            InputMode::Form => Action::FormScroll(delta),
            InputMode::ProcessPicker => {
                if delta > 0 {
                    Action::PickerNext
                } else {
                    Action::PickerPrevious
                }
            }
            InputMode::Normal => {
                if delta > 0 {
                    Action::SelectNext
                } else {
                    Action::SelectPrevious
                }
            }
            _ => Action::Nothing,
        }
    }

    fn action_for_key(&self, key: KeyEvent) -> Action {
        match self.mode {
            InputMode::Help | InputMode::Details => match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter => {
                    Action::CloseOverlay
                }
                _ => Action::Nothing,
            },
            InputMode::ProcessPicker => match key.code {
                KeyCode::Esc => Action::PickerClose,
                KeyCode::Enter => Action::PickerConfirm,
                KeyCode::Down => Action::PickerNext,
                KeyCode::Up => Action::PickerPrevious,
                KeyCode::Backspace => Action::PickerBackspace,
                KeyCode::Char(character) => Action::PickerChar(character),
                _ => Action::Nothing,
            },
            InputMode::Form => self.form_action_for_key(key),
            InputMode::Normal => self.normal_action_for_key(key),
        }
    }

    fn form_action_for_key(&self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let is_text = self.form.as_ref().is_some_and(SessionForm::is_text_field);

        match key.code {
            KeyCode::Esc => Action::FormCancel,
            KeyCode::Char('s') if ctrl => Action::FormSubmit(true),
            KeyCode::BackTab => Action::FormPreviousField,
            KeyCode::Tab if shift => Action::FormPreviousField,
            KeyCode::Tab => Action::FormNextField,
            KeyCode::Down => Action::FormNextField,
            KeyCode::Up => Action::FormPreviousField,
            KeyCode::Backspace => Action::FormBackspace,
            KeyCode::Enter => Action::FormToggle,
            KeyCode::Char(' ') if !is_text => Action::FormToggle,
            KeyCode::Char(character) if is_text => Action::FormChar(character),
            _ => Action::Nothing,
        }
    }

    fn normal_action_for_key(&self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => Action::KillSelected,
                KeyCode::Char('r') => Action::Refresh,
                _ => Action::Nothing,
            };
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char('?') => Action::ShowHelp,
            KeyCode::Char('n') => Action::NewSession,
            KeyCode::Char('e') => Action::EditSelected,
            // `k` is Kill, so vim-up is gone and the arrow keys are the only way
            // up. `r` is Refresh, so Restart moved to Shift+R.
            KeyCode::Char('k') => Action::KillSelected,
            // Shift+D, not `x` (an old Kill reflex) and not Delete (40% keyboards
            // have no dedicated Del key).
            KeyCode::Char('D') => Action::DeleteSelected,
            KeyCode::Char('r') | KeyCode::F(5) => Action::Refresh,
            KeyCode::Char('R') => Action::RestartSelected,
            KeyCode::Char('d') => Action::DuplicateSelected,
            KeyCode::Tab => Action::FocusNext,
            KeyCode::BackTab => Action::FocusPrevious,
            KeyCode::Enter => match self.focus {
                Focus::Footer => Action::ActivateFooter(self.footer_index),
                _ => Action::ShowDetails,
            },
            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                Focus::Footer => Action::FooterNext,
                _ => Action::SelectNext,
            },
            KeyCode::Up => match self.focus {
                Focus::Footer => Action::FooterPrevious,
                _ => Action::SelectPrevious,
            },
            KeyCode::Char('l') | KeyCode::Right => match self.focus {
                Focus::Footer => Action::FooterNext,
                _ => Action::ShowDetails,
            },
            KeyCode::Char('h') | KeyCode::Left => match self.focus {
                Focus::Footer => Action::FooterPrevious,
                _ => Action::Nothing,
            },
            _ => Action::Nothing,
        }
    }

    // -------------------------------------------------------------- reducer

    /// Run an action and any follow-up it returns. The cap is a guard against a
    /// future action pair that chains into itself.
    pub fn dispatch(&mut self, action: Action) {
        let mut next = Some(action);
        let mut steps = 0;
        while let Some(current) = next {
            next = self.handle_action(current);
            steps += 1;
            if steps > 8 {
                break;
            }
        }
    }

    pub fn handle_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::Nothing => None,
            Action::Quit => {
                if self.mode == InputMode::Normal {
                    self.should_quit = true;
                    None
                } else {
                    Some(Action::CloseOverlay)
                }
            }
            Action::Tick => {
                self.daemon.poll(&mut self.sessions);
                self.sync_external();
                None
            }
            Action::Refresh => {
                self.daemon.poll(&mut self.sessions);
                self.on_battery = utils::on_battery_power();
                self.sync_external();
                let external = self.external_count();
                self.message = Some((
                    format!(
                        "Rescanned \u{2014} {external} external session{}",
                        if external == 1 { "" } else { "s" }
                    ),
                    false,
                ));
                None
            }

            Action::SelectNext => self.move_selection(1),
            Action::SelectPrevious => self.move_selection(-1),
            Action::SelectIndex(index) => {
                if index < self.sessions.len() {
                    self.table_state.select(Some(index));
                    self.focus = Focus::Table;
                }
                None
            }

            Action::FocusNext => self.cycle_focus(1),
            Action::FocusPrevious => self.cycle_focus(-1),
            Action::SetFocus(focus) => {
                self.focus = focus;
                None
            }
            Action::FooterNext => {
                self.footer_index = (self.footer_index + 1) % FOOTER_BUTTONS.len();
                None
            }
            Action::FooterPrevious => {
                self.footer_index =
                    (self.footer_index + FOOTER_BUTTONS.len() - 1) % FOOTER_BUTTONS.len();
                None
            }
            Action::ActivateFooter(index) => {
                self.footer_index = index;
                self.focus = Focus::Footer;
                FOOTER_BUTTONS.get(index).map(|(_, action)| *action)
            }

            Action::ShowHelp => {
                self.mode = InputMode::Help;
                None
            }
            Action::ShowDetails => {
                if self.selected().is_some() {
                    self.mode = InputMode::Details;
                }
                None
            }
            Action::CloseOverlay => {
                match self.mode {
                    InputMode::ProcessPicker => return Some(Action::PickerClose),
                    InputMode::Form => return Some(Action::FormCancel),
                    _ => {}
                }
                self.mode = InputMode::Normal;
                None
            }

            Action::NewSession => {
                self.form = Some(SessionForm::blank());
                self.mode = InputMode::Form;
                self.focus = Focus::Form;
                None
            }
            Action::EditSelected => self.open_form_from_selection(true),
            Action::DuplicateSelected => self.open_form_from_selection(false),

            Action::KillSelected => self.kill_selected(),
            Action::DeleteSelected => self.delete_selected(),
            Action::RestartSelected => self.restart_selected(),
            Action::LaunchSelected => self.launch_selected(),

            Action::FormActivate(field) => {
                let Some(form) = &mut self.form else {
                    return None;
                };
                if !field.is_available(form.target_kind) {
                    return None;
                }
                form.field = field;
                self.focus = Focus::Form;
                if matches!(field, FormField::Name | FormField::TargetValue) {
                    None
                } else {
                    Some(Action::FormToggle)
                }
            }
            Action::FormNextField => {
                if let Some(form) = &mut self.form {
                    form.step_field(1);
                }
                None
            }
            Action::FormPreviousField => {
                if let Some(form) = &mut self.form {
                    form.step_field(-1);
                }
                None
            }
            Action::FormScroll(delta) => {
                if let Some(form) = &mut self.form {
                    form.scroll = form.scroll.saturating_add_signed(delta as i16);
                }
                None
            }
            Action::FormToggle => self.form_toggle(),
            Action::FormChar(character) => {
                self.form_insert(character);
                None
            }
            Action::FormBackspace => {
                if let Some(form) = &mut self.form {
                    match form.field {
                        FormField::Name => {
                            form.name.pop();
                        }
                        FormField::TargetValue => {
                            if let Some(value) = form.target_value_mut() {
                                value.pop();
                            }
                        }
                        _ => {}
                    }
                }
                None
            }
            Action::FormSubmit(launch) => self.form_submit(launch),
            Action::FormCancel => {
                self.form = None;
                self.picker = None;
                self.mode = InputMode::Normal;
                self.focus = Focus::Table;
                None
            }

            Action::OpenProcessPicker => {
                self.picker = Some(ProcessPicker::open());
                self.mode = InputMode::ProcessPicker;
                None
            }
            Action::PickerNext => {
                if let Some(picker) = &mut self.picker {
                    picker.selected = picker.selected.saturating_add(1);
                    picker.clamp();
                }
                None
            }
            Action::PickerPrevious => {
                if let Some(picker) = &mut self.picker {
                    picker.selected = picker.selected.saturating_sub(1);
                }
                None
            }
            Action::PickerSelect(index) => {
                if let Some(picker) = &mut self.picker {
                    picker.selected = index;
                    picker.clamp();
                }
                Some(Action::PickerConfirm)
            }
            Action::PickerChar(character) => {
                if let Some(picker) = &mut self.picker {
                    picker.filter.push(character);
                    picker.selected = 0;
                }
                None
            }
            Action::PickerBackspace => {
                if let Some(picker) = &mut self.picker {
                    picker.filter.pop();
                    picker.selected = 0;
                }
                None
            }
            Action::PickerConfirm => {
                let chosen = self.picker.as_ref().and_then(|picker| {
                    picker.visible().get(picker.selected).map(|entry| entry.pid)
                });
                if let (Some(pid), Some(form)) = (chosen, &mut self.form) {
                    form.pid_input = pid.to_string();
                    form.field = FormField::TargetValue;
                    form.error = None;
                }
                Some(Action::PickerClose)
            }
            Action::PickerClose => {
                self.picker = None;
                self.mode = if self.form.is_some() {
                    InputMode::Form
                } else {
                    InputMode::Normal
                };
                None
            }
        }
    }

    // ------------------------------------------------------------- handlers

    fn move_selection(&mut self, delta: i32) -> Option<Action> {
        if self.sessions.is_empty() {
            self.table_state.select(None);
            return None;
        }
        let length = self.sessions.len() as i32;
        let current = self.table_state.selected().unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(length) as usize;
        self.table_state.select(Some(next));
        self.focus = Focus::Table;
        None
    }

    fn cycle_focus(&mut self, delta: i32) -> Option<Action> {
        if self.mode == InputMode::Form {
            return Some(if delta >= 0 {
                Action::FormNextField
            } else {
                Action::FormPreviousField
            });
        }
        self.focus = match (self.focus, delta >= 0) {
            (Focus::Table, true) => Focus::Footer,
            (Focus::Footer, true) => Focus::Table,
            (Focus::Table, false) => Focus::Footer,
            (Focus::Footer, false) => Focus::Table,
            (Focus::Form, _) => Focus::Table,
        };
        None
    }

    fn open_form_from_selection(&mut self, editing: bool) -> Option<Action> {
        let Some(session) = self.selected() else {
            self.message = Some(("No session selected".to_string(), true));
            return None;
        };
        // An external session has no config of ours to edit, but copying its flags
        // into a new session is exactly how you take one over.
        if editing && session.external {
            self.message = Some((
                "External session \u{2014} press d to copy it into a new one".to_string(),
                true,
            ));
            return None;
        }
        self.form = Some(SessionForm::from_session(session, editing));
        self.mode = InputMode::Form;
        self.focus = Focus::Form;
        None
    }

    fn kill_selected(&mut self) -> Option<Action> {
        let Some(index) = self.selected_index() else {
            self.message = Some(("No session selected".to_string(), true));
            return None;
        };
        if !self.sessions[index].is_running() {
            self.message = Some(("Session is not running".to_string(), true));
            return None;
        }
        match self.daemon.kill(&mut self.sessions[index]) {
            Ok(()) => {
                let session = &self.sessions[index];
                let name = session.name.clone();
                let detail = match session.pid {
                    Some(pid) if session.external => format!(" (external, pid {pid})"),
                    _ => String::new(),
                };
                self.message = Some((format!("Stopping \u{201c}{name}\u{201d}{detail}"), false));
            }
            Err(error) => self.message = Some((error.to_string(), true)),
        }
        None
    }

    /// Remove the selected session from the list. Refuses while it is still
    /// running — kill it first, so a keystroke cannot orphan a live
    /// `caffeinate`. External rows are not ours to delete: they come back on the
    /// next rescan anyway.
    fn delete_selected(&mut self) -> Option<Action> {
        let Some(index) = self.selected_index() else {
            self.message = Some(("No session selected".to_string(), true));
            return None;
        };
        if self.sessions[index].is_running() {
            self.message = Some(("Kill the session before deleting it".to_string(), true));
            return None;
        }
        if self.sessions[index].external {
            self.message = Some(("External sessions cannot be deleted".to_string(), true));
            return None;
        }
        let name = self.sessions.remove(index).name;
        self.clamp_selection();
        self.message = Some((format!("Deleted \u{201c}{name}\u{201d}"), false));
        None
    }

    fn launch_selected(&mut self) -> Option<Action> {
        let index = self.selected_index()?;
        if self.sessions[index].external {
            return None;
        }
        if self.sessions[index].is_running() {
            self.message = Some(("Session is already running".to_string(), true));
            return None;
        }
        if let Target::WaitPid(pid) = self.sessions[index].target {
            if !utils::pid_is_alive(pid) {
                self.message = Some((format!("No live process with PID {pid}"), true));
                return None;
            }
        }
        match self.daemon.spawn(&mut self.sessions[index]) {
            Ok(()) => {
                let name = self.sessions[index].name.clone();
                self.message = Some((format!("Launched \u{201c}{name}\u{201d}"), false));
            }
            Err(error) => {
                self.sessions[index].status = SessionStatus::Error(error.to_string());
                self.message = Some((error.to_string(), true));
            }
        }
        None
    }

    fn restart_selected(&mut self) -> Option<Action> {
        let Some(index) = self.selected_index() else {
            self.message = Some(("No session selected".to_string(), true));
            return None;
        };
        // Restarting someone else's process would kill it and hand ownership to us
        // silently. Copy-then-launch is the explicit path.
        if self.sessions[index].external {
            self.message = Some((
                "External session \u{2014} press d to copy it into a new one".to_string(),
                true,
            ));
            return None;
        }
        if self.sessions[index].is_running() {
            // Kill is asynchronous; relaunch once the reaper has cleared it.
            let _ = self.daemon.kill(&mut self.sessions[index]);
            let id = self.sessions[index].id;
            self.wait_for_exit(id);
        }
        Some(Action::LaunchSelected)
    }

    /// Bounded blocking wait used only by restart, where relaunching before the
    /// old process is reaped would collide on the same session id.
    fn wait_for_exit(&mut self, id: Uuid) {
        let deadline = std::time::Instant::now() + crate::daemon::GRACE_PERIOD * 3;
        while std::time::Instant::now() < deadline && self.daemon.is_supervised(id) {
            self.daemon.poll(&mut self.sessions);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn form_insert(&mut self, character: char) {
        let Some(form) = &mut self.form else {
            return;
        };
        match form.field {
            FormField::Name => form.name.push(character),
            FormField::TargetValue => {
                let digits_only =
                    matches!(form.target_kind, TargetKind::Timeout | TargetKind::WaitPid);
                if digits_only && !character.is_ascii_digit() {
                    return;
                }
                if let Some(value) = form.target_value_mut() {
                    value.push(character);
                }
            }
            _ => {}
        }
        form.error = None;
    }

    fn form_toggle(&mut self) -> Option<Action> {
        let Some(form) = &mut self.form else {
            return None;
        };
        match form.field {
            FormField::FlagDisplay => form.flags.display = !form.flags.display,
            FormField::FlagIdle => form.flags.idle = !form.flags.idle,
            FormField::FlagDisk => form.flags.disk = !form.flags.disk,
            FormField::FlagSystem => form.flags.system = !form.flags.system,
            FormField::FlagUserActive => form.flags.user_active = !form.flags.user_active,
            FormField::TargetIndefinite => form.target_kind = TargetKind::Indefinite,
            FormField::TargetTimeout => form.target_kind = TargetKind::Timeout,
            FormField::TargetCommand => form.target_kind = TargetKind::Command,
            FormField::TargetWaitPid => form.target_kind = TargetKind::WaitPid,
            FormField::PickProcess => return Some(Action::OpenProcessPicker),
            FormField::SaveAndLaunch => return Some(Action::FormSubmit(true)),
            FormField::SaveOnly => return Some(Action::FormSubmit(false)),
            FormField::Cancel => return Some(Action::FormCancel),
            // Enter on a text field advances, matching every other form on earth.
            FormField::Name | FormField::TargetValue => return Some(Action::FormNextField),
        }
        // Picking a target that takes a value drops the caret straight into it;
        // picking Indefinite retires that input, so focus must leave it.
        if matches!(
            form.field,
            FormField::TargetTimeout | FormField::TargetCommand | FormField::TargetWaitPid
        ) {
            form.field = FormField::TargetValue;
        } else if !form.field.is_available(form.target_kind) {
            form.field = FormField::TargetIndefinite;
        }
        form.error = None;
        None
    }

    fn form_submit(&mut self, launch: bool) -> Option<Action> {
        let Some(form) = &self.form else {
            return None;
        };
        let (name, flags, target) = match form.validate(launch) {
            Ok(valid) => valid,
            Err(error) => {
                if let Some(form) = &mut self.form {
                    form.error = Some(error);
                }
                return None;
            }
        };
        let editing = form.editing;

        let index = match editing.and_then(|id| self.index_of(id)) {
            Some(index) => {
                if self.sessions[index].is_running() {
                    let _ = self.daemon.kill(&mut self.sessions[index]);
                    let id = self.sessions[index].id;
                    self.wait_for_exit(id);
                }
                let session = &mut self.sessions[index];
                session.name = name;
                session.flags = flags;
                session.target = target;
                session.expires_at = None;
                index
            }
            None => {
                self.sessions
                    .push(CaffeineSession::new(name, flags, target));
                self.sessions.len() - 1
            }
        };

        self.table_state.select(Some(index));
        self.form = None;
        self.picker = None;
        self.mode = InputMode::Normal;
        self.focus = Focus::Table;

        if launch {
            return Some(Action::LaunchSelected);
        }
        self.message = Some(("Saved".to_string(), false));
        None
    }

    fn index_of(&self, id: Uuid) -> Option<usize> {
        self.sessions.iter().position(|session| session.id == id)
    }

    /// Terminate every child we own and persist the list. Best effort: a failed
    /// write is reported, never fatal.
    ///
    /// External sessions are neither killed nor saved: they belong to another
    /// process and are rediscovered by scanning, not by remembering.
    pub fn shutdown(&mut self) -> Result<()> {
        self.daemon.shutdown(&mut self.sessions);
        let owned: Vec<CaffeineSession> = self
            .sessions
            .iter()
            .filter(|session| !session.external)
            .cloned()
            .collect();
        config::save(&owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form_with(kind: TargetKind, value: &str) -> SessionForm {
        let mut form = SessionForm::blank();
        form.name = "Session".into();
        form.target_kind = kind;
        match kind {
            TargetKind::Timeout => form.timeout_input = value.into(),
            TargetKind::Command => form.command_input = value.into(),
            TargetKind::WaitPid => form.pid_input = value.into(),
            TargetKind::Indefinite => {}
        }
        form
    }

    fn stopped(name: &str) -> CaffeineSession {
        let mut session = CaffeineSession::new(
            name.to_string(),
            CaffeinateFlags::default(),
            Target::Indefinite,
        );
        session.status = SessionStatus::Stopped;
        session
    }

    #[test]
    fn delete_removes_a_stopped_session_and_keeps_the_selection_valid() {
        let mut app = App::new();
        app.sessions = vec![stopped("first"), stopped("second")];
        app.table_state.select(Some(1));

        app.dispatch(Action::DeleteSelected);

        assert_eq!(app.sessions.len(), 1);
        assert_eq!(app.sessions[0].name, "first");
        assert_eq!(app.table_state.selected(), Some(0));
    }

    #[test]
    fn delete_refuses_while_the_session_is_running() {
        let mut app = App::new();
        let mut running = stopped("live");
        running.status = SessionStatus::Running;
        app.sessions = vec![running];
        app.table_state.select(Some(0));

        app.dispatch(Action::DeleteSelected);

        assert_eq!(app.sessions.len(), 1);
        assert!(app.message.as_ref().is_some_and(|(_, is_error)| *is_error));
    }

    #[test]
    fn name_is_required() {
        let mut form = form_with(TargetKind::Indefinite, "");
        form.name = "   ".into();
        assert_eq!(form.validate(false).unwrap_err(), "Name is required");
    }

    #[test]
    fn timeout_must_be_positive() {
        assert_eq!(
            form_with(TargetKind::Timeout, "0")
                .validate(false)
                .unwrap_err(),
            "Timeout must be greater than 0"
        );
        assert!(form_with(TargetKind::Timeout, "60").validate(false).is_ok());
    }

    #[test]
    fn command_must_be_non_empty() {
        assert_eq!(
            form_with(TargetKind::Command, "  ")
                .validate(false)
                .unwrap_err(),
            "Command cannot be empty"
        );
    }

    #[test]
    fn dead_pid_blocks_launch_but_not_save() {
        // PID 0 is never a launchable user process.
        let form = form_with(TargetKind::WaitPid, "0");
        assert!(form.validate(false).is_ok(), "save-only skips liveness");
        assert!(form.validate(true).is_err(), "launch requires a live pid");
    }

    #[test]
    fn tab_order_hides_value_field_for_indefinite() {
        let indefinite = form_with(TargetKind::Indefinite, "");
        assert!(!indefinite
            .fields_in_order()
            .contains(&FormField::TargetValue));

        let timeout = form_with(TargetKind::Timeout, "30");
        assert!(timeout.fields_in_order().contains(&FormField::TargetValue));
        assert!(!timeout.fields_in_order().contains(&FormField::PickProcess));

        let wait = form_with(TargetKind::WaitPid, "1");
        assert!(wait.fields_in_order().contains(&FormField::PickProcess));
    }

    #[test]
    fn tab_wraps_around_available_fields() {
        let mut form = form_with(TargetKind::Indefinite, "");
        form.field = FormField::Cancel;
        form.step_field(1);
        assert_eq!(form.field, FormField::Name);
        form.step_field(-1);
        assert_eq!(form.field, FormField::Cancel);
    }

    #[test]
    fn timeout_digits_only_and_hint_is_human_readable() {
        let mut app = App {
            sessions: Vec::new(),
            mode: InputMode::Form,
            form: Some(form_with(TargetKind::Timeout, "")),
            table_state: TableState::default(),
            focus: Focus::Form,
            daemon: Daemon::new(),
            picker: None,
            footer_index: 0,
            on_battery: false,
            message: None,
            should_quit: false,
            regions: Vec::new(),
            scanner: utils::ProcessScanner::new(),
        };
        if let Some(form) = &mut app.form {
            form.field = FormField::TargetValue;
        }
        for character in "3a6b0c0".chars() {
            app.dispatch(Action::FormChar(character));
        }
        let form = app.form.as_ref().unwrap();
        assert_eq!(form.timeout_input, "3600");
        assert_eq!(form.target_hint().unwrap(), "3600 = 1h");
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn kill_is_k_and_refresh_is_r_without_colliding() {
        let app = App::new();

        assert_eq!(
            app.action_for_key(key(KeyCode::Char('k'))),
            Action::KillSelected
        );
        assert_eq!(app.action_for_key(key(KeyCode::Char('r'))), Action::Refresh);
        // Restart had to move off `r`.
        assert_eq!(
            app.action_for_key(key(KeyCode::Char('R'))),
            Action::RestartSelected
        );
        // `k` is Kill, so it must no longer move the selection; the arrow does.
        assert_eq!(app.action_for_key(key(KeyCode::Up)), Action::SelectPrevious);
        assert_eq!(
            app.action_for_key(key(KeyCode::Char('j'))),
            Action::SelectNext
        );
        // And the key kill used to live on is now unbound.
        assert_eq!(app.action_for_key(key(KeyCode::Char('x'))), Action::Nothing);

        assert_eq!(
            app.action_for_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Action::KillSelected
        );
        assert_eq!(
            app.action_for_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            Action::Refresh
        );
        assert_eq!(app.action_for_key(key(KeyCode::F(5))), Action::Refresh);
    }

    #[test]
    fn every_footer_button_matches_its_labelled_key() {
        let app = App::new();
        for (label, action) in FOOTER_BUTTONS {
            // The letter between the brackets is the key the label promises.
            let promised = label
                .trim_start_matches('[')
                .split(']')
                .next()
                .expect("every footer label brackets its key");
            let code = match promised {
                "Shift+R" => KeyCode::Char('R'),
                "Shift+D" => KeyCode::Char('D'),
                other => KeyCode::Char(other.chars().next().unwrap().to_ascii_lowercase()),
            };
            assert_eq!(
                app.action_for_key(key(code)),
                *action,
                "footer label {label:?} does not match the key it advertises"
            );
        }
    }

    #[test]
    fn k_still_types_a_letter_inside_the_form() {
        let mut app = App::new();
        app.sessions.clear();
        app.dispatch(Action::NewSession);
        for character in "kayak".chars() {
            app.on_key(key(KeyCode::Char(character)));
        }
        assert_eq!(app.form.as_ref().unwrap().name, "kayak");
        assert_eq!(app.mode, InputMode::Form, "typing k must not kill anything");
    }

    #[test]
    fn footer_click_resolves_to_its_keybinding_action() {
        let mut app = App::new();
        app.sessions.clear();
        let follow_up = app.handle_action(Action::ActivateFooter(0));
        assert_eq!(follow_up, Some(Action::NewSession));
        assert_eq!(app.focus, Focus::Footer);
    }

    #[test]
    fn save_only_adds_a_stopped_session() {
        let mut app = App::new();
        app.sessions.clear();
        app.form = Some(form_with(TargetKind::Indefinite, ""));
        app.mode = InputMode::Form;
        app.dispatch(Action::FormSubmit(false));

        assert_eq!(app.sessions.len(), 1);
        assert_eq!(app.sessions[0].status, SessionStatus::Stopped);
        assert_eq!(app.mode, InputMode::Normal);
        assert!(app.form.is_none());
    }

    #[test]
    fn escape_in_form_returns_to_table_without_saving() {
        let mut app = App::new();
        app.sessions.clear();
        app.dispatch(Action::NewSession);
        assert_eq!(app.mode, InputMode::Form);
        app.dispatch(Action::FormCancel);
        assert_eq!(app.mode, InputMode::Normal);
        assert_eq!(app.focus, Focus::Table);
        assert!(app.sessions.is_empty());
    }

    #[test]
    fn quit_closes_a_modal_before_it_exits() {
        let mut app = App::new();
        app.dispatch(Action::ShowHelp);
        app.dispatch(Action::Quit);
        assert!(!app.should_quit, "first quit closes the modal");
        assert_eq!(app.mode, InputMode::Normal);
        app.dispatch(Action::Quit);
        assert!(app.should_quit);
    }

    fn external(pid: u32, line: &str) -> utils::ExternalProcess {
        utils::ExternalProcess {
            pid,
            argv: line.split_whitespace().map(str::to_string).collect(),
            parent_name: Some("zsh".to_string()),
            start_time: 1_700_000_000,
        }
    }

    #[test]
    fn reconcile_adopts_then_drops_external_processes() {
        let mut app = App::new();
        app.sessions.clear();

        // Two bare `caffeinate &` from a shell, exactly what the scan sees.
        app.reconcile_external(&[external(101, "caffeinate"), external(202, "caffeinate")]);
        assert_eq!(app.sessions.len(), 2);
        assert_eq!(app.external_count(), 2);
        assert_eq!(
            app.sessions
                .iter()
                .filter_map(|s| s.pid)
                .collect::<Vec<_>>(),
            [101, 202]
        );
        assert!(app.sessions.iter().all(|s| s.is_running()));

        // Idempotent: rescanning the same pids must not duplicate rows.
        let first_ids: Vec<_> = app.sessions.iter().map(|s| s.id).collect();
        app.reconcile_external(&[external(101, "caffeinate"), external(202, "caffeinate")]);
        assert_eq!(app.sessions.len(), 2);
        assert_eq!(
            app.sessions.iter().map(|s| s.id).collect::<Vec<_>>(),
            first_ids,
            "ids must be stable so the selection does not jump"
        );

        // One dies; its row goes away and the other stays.
        app.reconcile_external(&[external(202, "caffeinate")]);
        assert_eq!(app.sessions.len(), 1);
        assert_eq!(app.sessions[0].pid, Some(202));
    }

    #[test]
    fn our_sessions_sort_above_externals_and_selection_follows_the_row() {
        let mut app = App::new();
        app.sessions.clear();
        app.reconcile_external(&[external(101, "caffeinate"), external(202, "caffeinate")]);

        // A session saved after the scan must still land above the external rows.
        app.sessions.push(CaffeineSession::new(
            "mine".into(),
            CaffeinateFlags::default(),
            Target::Indefinite,
        ));
        let mine = app.sessions.last().unwrap().id;
        app.table_state.select(Some(2));
        assert_eq!(app.selected().unwrap().id, mine);

        app.reconcile_external(&[external(101, "caffeinate"), external(202, "caffeinate")]);
        assert_eq!(app.sessions[0].name, "mine");
        assert!(app.sessions[1].external && app.sessions[2].external);
        assert_eq!(
            app.selected().unwrap().id,
            mine,
            "the selection tracks the session, not the index"
        );

        // Dropping an external above nothing must not shift the selection either.
        app.table_state.select(Some(2));
        let tail = app.sessions[2].id;
        app.reconcile_external(&[external(202, "caffeinate")]);
        assert_eq!(app.selected().unwrap().id, tail);
    }

    #[test]
    fn reconcile_never_touches_our_own_sessions() {
        let mut app = App::new();
        app.sessions.clear();
        let mut owned = CaffeineSession::new(
            "mine".into(),
            CaffeinateFlags::default(),
            Target::Indefinite,
        );
        owned.pid = Some(555);
        owned.status = SessionStatus::Running;
        app.sessions.push(owned);

        // Our own child shows up in the process scan too — it must not be adopted
        // as external, and a stopped session of ours must survive an empty scan.
        app.reconcile_external(&[external(555, "caffeinate -i")]);
        assert_eq!(app.sessions.len(), 1);
        assert!(!app.sessions[0].external);

        app.sessions[0].status = SessionStatus::Stopped;
        app.sessions[0].pid = None;
        app.reconcile_external(&[]);
        assert_eq!(
            app.sessions.len(),
            1,
            "saved sessions are not process-backed"
        );
    }

    #[test]
    fn external_sessions_reject_edit_and_restart_but_allow_duplicate() {
        let mut app = App::new();
        app.sessions.clear();
        app.reconcile_external(&[external(303, "caffeinate -d -i")]);
        app.table_state.select(Some(0));

        app.dispatch(Action::EditSelected);
        assert!(app.form.is_none(), "edit is blocked");
        assert!(app.message.as_ref().unwrap().1, "and reported as an error");

        app.message = None;
        app.dispatch(Action::RestartSelected);
        assert_eq!(app.sessions.len(), 1, "restart did not relaunch anything");
        assert!(app.message.as_ref().unwrap().1);

        app.message = None;
        app.dispatch(Action::DuplicateSelected);
        let form = app.form.as_ref().expect("duplicate opens a prefilled form");
        assert_eq!(form.editing, None, "the copy is a new session");
        assert!(
            form.flags.display && form.flags.idle,
            "flags came from the argv"
        );
    }

    #[test]
    fn external_sessions_are_not_persisted() {
        let mut app = App::new();
        app.sessions.clear();
        app.reconcile_external(&[external(404, "caffeinate -i")]);
        app.sessions.push(CaffeineSession::new(
            "mine".into(),
            CaffeinateFlags::default(),
            Target::Indefinite,
        ));

        let owned: Vec<_> = app.sessions.iter().filter(|s| !s.external).collect();
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].name, "mine");
    }

    #[test]
    fn selection_wraps_and_ignores_empty_list() {
        let mut app = App::new();
        app.sessions = vec![
            CaffeineSession::new("one".into(), CaffeinateFlags::default(), Target::Indefinite),
            CaffeineSession::new("two".into(), CaffeinateFlags::default(), Target::Indefinite),
        ];
        app.table_state.select(Some(0));
        app.dispatch(Action::SelectPrevious);
        assert_eq!(app.table_state.selected(), Some(1));
        app.dispatch(Action::SelectNext);
        assert_eq!(app.table_state.selected(), Some(0));

        app.sessions.clear();
        app.dispatch(Action::SelectNext);
        assert_eq!(app.table_state.selected(), None);
    }
}
