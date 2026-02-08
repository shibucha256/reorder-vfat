mod app;
mod fs_ops;
mod platform;
mod ui;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::time::Duration;

use app::{handle_confirm_sort_keys, handle_drive_select_keys, handle_normal_keys, handle_rename_keys};
use app::App;
use ui::ui;

pub fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let start_dir = std::env::current_dir().context("get current dir")?;
    let mut app = App::new(start_dir)?;

    let tick_rate = Duration::from_millis(200);
    loop {
        terminal.draw(|frame| ui(frame, &mut app))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match app.mode {
                    app::Mode::Normal => {
                        if handle_normal_keys(&mut app, key.code, key.modifiers)? {
                            return Ok(());
                        }
                    }
                    app::Mode::Renaming => {
                        handle_rename_keys(&mut app, key.code)?;
                    }
                    app::Mode::ConfirmSort => {
                        handle_confirm_sort_keys(&mut app, key.code)?;
                    }
                    app::Mode::SelectDrive => {
                        handle_drive_select_keys(&mut app, key.code)?;
                    }
                }
            }
        }
    }
}
