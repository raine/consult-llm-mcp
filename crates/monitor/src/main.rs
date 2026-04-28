mod action;
mod app;
mod format;
mod input;
mod meta;
mod poller;
mod state;
mod ui;

use std::fs;
use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use consult_llm_core::monitoring::{active_dir, runs_dir, sessions_dir};

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
        crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

// ── Main ────────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("consult-llm-monitor {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        default_hook(info);
    }));

    consult_llm_core::path_migrate::migrate_if_needed();

    let mut guard = TerminalGuard::new()?;
    let mut state = AppState::new();
    let dir = sessions_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = active_dir();
    let _ = runs_dir();

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

        let mut should_quit = false;
        if event::poll(render_interval)? {
            // Drain all queued input events in one pass so a single render
            // covers a whole burst of wheel/key activity.
            loop {
                let action = match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        input::handle_key(&state, &row_infos, key, &dir)
                    }
                    Event::Mouse(mouse) => input::handle_mouse(&state, &row_infos, mouse, &dir),
                    _ => None,
                };
                if let Some(action) = action
                    && dispatch_action(action, &mut state, &cmd_tx, &dir)
                {
                    should_quit = true;
                    break;
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if should_quit {
            let _ = cmd_tx.send(PollCommand::Shutdown);
            break;
        }
    }

    Ok(())
}

/// Apply an action and send any side-effect commands to the poller.
/// Returns `true` if this action was `Quit` and the loop should stop.
fn dispatch_action(
    action: Action,
    state: &mut AppState,
    cmd_tx: &std::sync::mpsc::Sender<PollCommand>,
    dir: &std::path::Path,
) -> bool {
    if matches!(action, Action::Quit) {
        return true;
    }

    let entering_detail = matches!(
        &action,
        Action::EnterDetail(_) | Action::NextSibling | Action::PrevSibling
    );
    let entering_thread_detail = matches!(&action, Action::EnterThreadDetail(_));
    let exiting_detail = matches!(&action, Action::ExitDetail);
    let clearing_history = matches!(&action, Action::ClearHistory);

    state.apply(action);

    if entering_detail {
        if let AppMode::Detail(ref detail) = state.mode {
            let _ = cmd_tx.send(PollCommand::EnterDetail {
                run_id: detail.run_id.clone(),
                file_offset: detail.file_offset,
            });
        }
    } else if entering_thread_detail {
        if let AppMode::ThreadDetail(ref detail) = state.mode
            && let Some(last_run_id) = detail.turn_ids.last()
        {
            let _ = cmd_tx.send(PollCommand::EnterDetail {
                run_id: last_run_id.clone(),
                file_offset: detail.active_file_offset,
            });
        }
    } else if exiting_detail {
        let _ = cmd_tx.send(PollCommand::ExitDetail);
    } else if clearing_history {
        let _ = fs::File::create(dir.join(consult_llm_core::monitoring::HISTORY_FILE));
        let _ = cmd_tx.send(PollCommand::ResetHistory);
    }

    false
}
