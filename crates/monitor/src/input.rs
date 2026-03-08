use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::state::{AppMode, AppState, Focus, RowInfo};

/// Pure mapping from key events to actions. No state mutation.
pub(crate) fn handle_key(
    state: &AppState,
    row_infos: &[RowInfo],
    key: KeyEvent,
    dir: &Path,
) -> Option<Action> {
    // When help overlay is visible, only handle dismiss keys
    if state.show_help {
        return match key.code {
            KeyCode::Char('?') | KeyCode::Esc => Some(Action::ToggleHelp),
            _ => None,
        };
    }

    // When filter input is active, route all keys to filter handling
    if state.filter_editing {
        return handle_filter_key(key);
    }

    match &state.mode {
        AppMode::Table => handle_table_key(state, row_infos, key, dir),
        AppMode::Detail(_) => handle_detail_key(key),
        AppMode::ConfirmClearHistory => handle_confirm_clear_key(key),
    }
}

fn handle_table_key(
    state: &AppState,
    row_infos: &[RowInfo],
    key: KeyEvent,
    dir: &Path,
) -> Option<Action> {
    match key.code {
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Tab | KeyCode::BackTab => Some(Action::ToggleFocus),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Char('/') => Some(Action::StartFilter),
        KeyCode::Char('X') => Some(Action::PromptClearHistory),
        KeyCode::Enter => match state.focus {
            Focus::Active => {
                if let Some(info) = row_infos.get(state.selected)
                    && !info.consultation_id.is_empty()
                {
                    return Some(Action::EnterDetail(info.consultation_id.clone()));
                }
                None
            }
            Focus::History => {
                let indices = state.filtered_history_indices();
                let actual_idx = indices.get(state.history_selected).copied();
                if let Some(record) = actual_idx.and_then(|i| state.history.get(i)) {
                    if let Some(cid) = &record.consultation_id {
                        let path = dir.join(format!("{cid}.events.jsonl"));
                        if path.exists() {
                            Some(Action::EnterDetail(cid.clone()))
                        } else {
                            Some(Action::Flash("log file not found".into(), 20))
                        }
                    } else {
                        Some(Action::Flash("no log available for this entry".into(), 20))
                    }
                } else {
                    None
                }
            }
        },
        _ => None,
    }
}

fn handle_confirm_clear_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('y') => Some(Action::ClearHistory),
        _ => Some(Action::CancelClear),
    }
}

fn handle_filter_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Enter => Some(Action::FilterAccept),
        KeyCode::Esc => Some(Action::FilterCancel),
        KeyCode::Backspace => Some(Action::FilterBackspace),
        KeyCode::Char(c) => Some(Action::FilterInput(c)),
        _ => None,
    }
}

fn handle_detail_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::ExitDetail),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::ScrollDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::ScrollUp),
        KeyCode::Char('d') => Some(Action::HalfPageDown),
        KeyCode::Char('u') => Some(Action::HalfPageUp),
        KeyCode::Char('G') => Some(Action::ScrollToBottom),
        KeyCode::Char('y') => Some(Action::YankResponse),
        _ => None,
    }
}
