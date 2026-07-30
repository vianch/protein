//! `caf` — a terminal UI over macOS `caffeinate(8)`.

mod app;
mod config;
mod daemon;
mod models;
mod ui;
mod utils;

use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::app::{Action, App};

/// Status/countdown refresh rate.
const TICK: Duration = Duration::from_secs(1);
/// How long the input thread waits for a key before looping.
const INPUT_POLL: Duration = Duration::from_millis(250);

const USAGE: &str = "\
caf \u{2014} build, launch and track macOS caffeinate sessions

USAGE:
    caf [OPTIONS]

OPTIONS:
    --ascii          Use ASCII glyphs instead of box-drawing characters
    --theme <name>   Colour theme: mocha (default), purple
    -h, --help       Print this help
    -V, --version    Print version

Sessions are stored in ~/.config/protein/sessions.json.
Press ? inside the app for the full keybinding and mouse reference.";

#[tokio::main]
async fn main() -> Result<()> {
    let mut ascii = false;
    let mut theme = std::env::var("PROTEIN_THEME").ok();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--ascii" => ascii = true,
            "--theme" => match arguments.next() {
                Some(name) => theme = Some(name),
                None => {
                    eprintln!("caf: --theme needs a name ({})", ui::styles::theme_names());
                    std::process::exit(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("caf {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => {
                eprintln!("caf: unknown argument `{other}`\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    ui::styles::set_ascii(ascii || std::env::var_os("PROTEIN_ASCII").is_some());
    if let Some(name) = &theme {
        if !ui::styles::set_theme(name) {
            eprintln!(
                "caf: unknown theme `{name}` (available: {})",
                ui::styles::theme_names()
            );
            std::process::exit(2);
        }
    }

    let mut terminal = enter_terminal()?;
    let result = run(&mut terminal).await;
    leave_terminal(&mut terminal);
    result
}

type Tui = Terminal<CrosstermBackend<io::Stdout>>;

fn enter_terminal() -> Result<Tui> {
    // ratatui::try_init also installs a panic hook that restores the terminal, so
    // a crash cannot leave the user stuck in raw mode.
    let mut terminal = ratatui::try_init().context("entering the alternate screen")?;
    execute!(io::stdout(), EnableMouseCapture).context("enabling mouse capture")?;
    terminal.clear()?;
    Ok(terminal)
}

fn leave_terminal(terminal: &mut Tui) {
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let _ = terminal.show_cursor();
}

async fn run(terminal: &mut Tui) -> Result<()> {
    let mut app = App::new();
    let mut events = spawn_input_reader();
    let mut ticker = tokio::time::interval(TICK);

    terminal.draw(|frame| ui::draw(frame, &mut app))?;

    while !app.should_quit {
        let mut dirty = false;

        tokio::select! {
            _ = ticker.tick() => {
                app.dispatch(Action::Tick);
                dirty = true;
            }
            maybe_event = events.recv() => {
                match maybe_event {
                    Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        // A keypress clears the previous one-liner.
                        app.message = None;
                        app.on_key(key);
                        dirty = true;
                    }
                    Some(Event::Mouse(mouse)) => {
                        app.on_mouse(mouse);
                        dirty = true;
                    }
                    Some(Event::Resize(_, _)) => dirty = true,
                    Some(_) => {}
                    // The reader thread is gone; nothing is left to drive the UI.
                    None => break,
                }
            }
        }

        if dirty {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
        }
    }

    app.shutdown().context("saving sessions")
}

/// crossterm's reader is blocking, so it lives on its own thread and feeds the
/// async loop through a channel.
fn spawn_input_reader() -> UnboundedReceiver<Event> {
    let (sender, receiver) = mpsc::unbounded_channel();
    std::thread::spawn(move || read_events(sender));
    receiver
}

fn read_events(sender: UnboundedSender<Event>) {
    loop {
        match event::poll(INPUT_POLL) {
            Ok(true) => match event::read() {
                Ok(event) => {
                    if sender.send(event).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            },
            Ok(false) => {}
            Err(_) => return,
        }
    }
}
