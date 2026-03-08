mod action;
mod app;
mod format;
mod input;
mod poller;
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
use crate::poller::PollCommand;
use crate::state::{AppMode, AppState};

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

    let poll_interval = Duration::from_millis(500);
    let render_interval = Duration::from_millis(100);

    let (update_rx, cmd_tx, _poll_thread) = poller::spawn(dir.clone(), poll_interval);

    // Pre-initialize syntect data on a background thread so the first code
    // block render doesn't stutter. The OnceLock ensures the render path
    // blocks only if init hasn't finished yet.
    std::thread::spawn(ui::init_syntax);

    let mut row_infos = Vec::new();

    loop {
        // Drain all pending poll updates (non-blocking)
        while let Ok(update) = update_rx.try_recv() {
            state.apply_poll_update(update);
        }

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

        state.ensure_filter_cache();
        guard.terminal.draw(|frame| ui::render(frame, &mut state))?;
        state.tick += 1;

        if event::poll(render_interval)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(action) = input::handle_key(&state, &row_infos, key, &dir)
        {
            if matches!(action, Action::Quit) {
                let _ = cmd_tx.send(PollCommand::Shutdown);
                break;
            }

            let entering_detail = matches!(&action, Action::EnterDetail(_));
            let entering_thread_detail = matches!(&action, Action::EnterThreadDetail(_));
            let exiting_detail = matches!(&action, Action::ExitDetail);
            let clearing_history = matches!(&action, Action::ClearHistory);

            state.apply(action, &dir);

            if entering_detail {
                if let AppMode::Detail(ref detail) = state.mode {
                    let _ = cmd_tx.send(PollCommand::EnterDetail {
                        consultation_id: detail.consultation_id.clone(),
                        file_offset: detail.file_offset,
                    });
                }
            } else if entering_thread_detail {
                if let AppMode::ThreadDetail(ref detail) = state.mode {
                    // Poll the latest turn's event file
                    if let Some(last_cid) = detail.turn_ids.last() {
                        let _ = cmd_tx.send(PollCommand::EnterDetail {
                            consultation_id: last_cid.clone(),
                            file_offset: detail.active_file_offset,
                        });
                    }
                }
            } else if exiting_detail {
                let _ = cmd_tx.send(PollCommand::ExitDetail);
            } else if clearing_history {
                let _ = fs::File::create(dir.join(consult_llm_core::monitoring::HISTORY_FILE));
                let _ = cmd_tx.send(PollCommand::ResetHistory);
            }
        }
    }

    Ok(())
}
