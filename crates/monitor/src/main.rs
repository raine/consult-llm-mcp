mod action;
mod app;
mod format;
mod input;
mod polling;
mod state;
mod ui;

use std::fs;
use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use consult_llm_core::monitoring::sessions_dir;

use crate::action::Action;
use crate::state::AppState;

// ── Terminal guard ───────────────────────────────────────────────────────

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = crossterm::execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
    }
}

// ── Main ────────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    let mut guard = TerminalGuard::new()?;
    let mut state = AppState::new();
    let dir = sessions_dir();
    let _ = fs::create_dir_all(&dir);

    state.poll_files(&dir);
    state.poll_history(&dir);
    state.check_liveness();
    state.prune_finished(&dir);

    let poll_interval = Duration::from_millis(500);
    let mut last_poll = std::time::Instant::now();
    let render_interval = Duration::from_millis(100);

    let mut row_infos = Vec::new();

    loop {
        // Remember what was selected before rebuilding
        let prev_selected = row_infos.get(state.selected).cloned();

        // Rebuild sorted rows
        row_infos = state.build_row_infos();
        state.row_count = row_infos.len();

        // Restore selection by identity
        if let Some(prev) = &prev_selected {
            if let Some(new_idx) = row_infos.iter().position(|r| r == prev) {
                state.selected = new_idx;
            } else if state.row_count > 0 {
                state.selected = state.selected.min(state.row_count - 1);
            }
        } else if state.row_count > 0 && state.selected >= state.row_count {
            state.selected = state.row_count - 1;
        }

        // Tick down flash message
        if let Some((_, ttl)) = &mut state.flash {
            if *ttl == 0 {
                state.flash = None;
            } else {
                *ttl -= 1;
            }
        }

        guard.terminal.draw(|frame| ui::render(frame, &mut state))?;
        state.tick += 1;

        if event::poll(render_interval)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = input::handle_key(&state, &row_infos, key, &dir)
        {
            if matches!(action, Action::Quit) {
                break;
            }
            state.apply(action, &dir);
        }

        if last_poll.elapsed() >= poll_interval {
            state.poll_files(&dir);
            state.poll_history(&dir);
            state.check_liveness();
            state.prune_finished(&dir);
            state.poll_detail_events(&dir);
            last_poll = std::time::Instant::now();
        }
    }

    Ok(())
}
