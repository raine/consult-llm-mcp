mod detail_load;
mod poll;

use arboard::Clipboard;
use consult_llm_core::stream_events::ParsedStreamEvent;

use crate::action::Action;
use crate::state::{AppMode, AppState, Focus, RowInfo};

impl AppState {
    pub(crate) fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => unreachable!("handled in main loop"),
            Action::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::Active => {
                        self.history_selected = 0;
                        Focus::History
                    }
                    Focus::History => Focus::Active,
                };
            }
            Action::MoveDown => match self.focus {
                Focus::Active => {
                    if self.row_count > 0 {
                        self.selected = (self.selected + 1).min(self.row_count - 1);
                    }
                }
                Focus::History => {
                    let count = self.build_history_display_rows().len();
                    if count > 0 {
                        self.history_selected = (self.history_selected + 1).min(count - 1);
                    }
                }
            },
            Action::MoveUp => match self.focus {
                Focus::Active => {
                    self.selected = self.selected.saturating_sub(1);
                }
                Focus::History => {
                    self.history_selected = self.history_selected.saturating_sub(1);
                }
            },
            Action::SelectActiveRow(idx) => {
                if self.row_count > 0 {
                    self.selected = idx.min(self.row_count - 1);
                    self.focus = Focus::Active;
                }
            }
            Action::SelectHistoryRow(idx) => {
                let count = self.build_history_display_rows().len();
                if count > 0 {
                    self.history_selected = idx.min(count - 1);
                    self.focus = Focus::History;
                }
            }
            Action::EnterDetail(run_id) => {
                let from_history = matches!(self.focus, Focus::History);
                self.enter_detail(run_id);
                if from_history && let AppMode::Detail(ref mut detail) = self.mode {
                    detail.scroll = 0;
                    detail.auto_scroll = false;
                }
                self.populate_detail_siblings();
            }
            Action::EnterThreadDetail(thread_id) => {
                self.enter_thread_detail(thread_id);
            }
            Action::PrevTurn => {
                if let AppMode::ThreadDetail(ref mut detail) = self.mode
                    && detail.selected_turn > 0
                {
                    detail.selected_turn -= 1;
                    if let Some(&offset) = detail.turn_line_offsets.get(detail.selected_turn) {
                        detail.scroll = offset;
                        detail.auto_scroll = false;
                    }
                }
            }
            Action::NextTurn => {
                if let AppMode::ThreadDetail(ref mut detail) = self.mode
                    && detail.selected_turn + 1 < detail.turn_count
                {
                    detail.selected_turn += 1;
                    if let Some(&offset) = detail.turn_line_offsets.get(detail.selected_turn) {
                        detail.scroll = offset;
                        detail.auto_scroll = false;
                    }
                }
            }
            Action::NextSibling | Action::PrevSibling => {
                let forward = matches!(action, Action::NextSibling);
                if let AppMode::Detail(ref detail) = self.mode {
                    let siblings = detail.siblings.clone();
                    let current_idx = detail.sibling_index;
                    if siblings.len() > 1 {
                        let next_idx = if forward {
                            (current_idx + 1) % siblings.len()
                        } else {
                            (current_idx + siblings.len() - 1) % siblings.len()
                        };
                        let next_id = siblings[next_idx].clone();
                        self.enter_detail(next_id);
                        if let AppMode::Detail(ref mut detail) = self.mode {
                            detail.siblings = siblings;
                            detail.sibling_index = next_idx;
                        }
                    }
                }
            }
            Action::ExitDetail => {
                self.mode = AppMode::Table;
            }
            Action::ScrollDown => self.mutate_scroll(|scroll, _, _| {
                *scroll = scroll.saturating_add(1);
            }),
            Action::ScrollUp => self.mutate_scroll(|scroll, auto_scroll, _| {
                *scroll = scroll.saturating_sub(1);
                *auto_scroll = false;
            }),
            Action::HalfPageDown => self.mutate_scroll(|scroll, _, height| {
                *scroll = scroll.saturating_add((height / 2).max(1));
            }),
            Action::HalfPageUp => self.mutate_scroll(|scroll, auto_scroll, height| {
                *scroll = scroll.saturating_sub((height / 2).max(1));
                *auto_scroll = false;
            }),
            Action::PageDown => self.mutate_scroll(|scroll, _, height| {
                *scroll = scroll.saturating_add(height.max(1));
            }),
            Action::PageUp => self.mutate_scroll(|scroll, auto_scroll, height| {
                *scroll = scroll.saturating_sub(height.max(1));
                *auto_scroll = false;
            }),
            Action::ScrollToBottom => self.mutate_scroll(|scroll, auto_scroll, _| {
                *scroll = usize::MAX;
                *auto_scroll = true;
            }),
            Action::ScrollToTop => self.mutate_scroll(|scroll, auto_scroll, _| {
                *scroll = 0;
                *auto_scroll = false;
            }),
            Action::ScrollToResponse => {
                let offset = match &self.mode {
                    AppMode::Detail(detail) => detail.response_line_offset,
                    AppMode::ThreadDetail(detail) => detail.response_line_offset,
                    _ => None,
                };
                if let Some(offset) = offset {
                    self.mutate_scroll(|scroll, auto_scroll, _| {
                        *scroll = offset;
                        *auto_scroll = false;
                    });
                }
            }
            Action::PromptClearHistory => {
                self.mode = AppMode::ConfirmClearHistory;
            }
            Action::ClearHistory => {
                self.history.clear();
                self.history_selected = 0;
                self.invalidate_filter_cache();
                self.mode = AppMode::Table;
                self.flash = Some(("History cleared".into(), 20));
            }
            Action::CancelClear => {
                self.mode = AppMode::Table;
                self.flash = None;
            }
            Action::Flash(msg, ttl) => {
                self.flash = Some((msg, ttl));
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Action::ToggleSystemPrompt => {
                if let AppMode::Detail(ref mut detail) = self.mode {
                    detail.show_system_prompt = !detail.show_system_prompt;
                    detail.cached_lines = None;
                }
            }
            Action::YankResponse => {
                let events: Option<&[ParsedStreamEvent]> = match &self.mode {
                    AppMode::Detail(detail) => Some(&detail.events),
                    AppMode::ThreadDetail(detail) => Some(&detail.active_events),
                    _ => None,
                };
                if let Some(events) = events {
                    let last_text = events.iter().rev().find_map(|event| match event {
                        ParsedStreamEvent::AssistantText { text } if !text.is_empty() => {
                            Some(text.clone())
                        }
                        _ => None,
                    });
                    match last_text {
                        Some(text) => match Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
                            Ok(()) => self.flash = Some(("Copied to clipboard".into(), 20)),
                            Err(e) => self.flash = Some((format!("Clipboard error: {e}"), 20)),
                        },
                        None => self.flash = Some(("No assistant response to copy".into(), 20)),
                    }
                }
            }
            Action::StartFilter => {
                self.filter_editing = true;
                self.focus = Focus::History;
            }
            Action::FilterInput(c) => {
                self.filter_text.push(c);
                self.invalidate_filter_cache();
                self.clamp_history_selection();
            }
            Action::FilterBackspace => {
                self.filter_text.pop();
                self.invalidate_filter_cache();
                self.clamp_history_selection();
            }
            Action::FilterAccept => {
                self.filter_editing = false;
            }
            Action::FilterCancel => {
                self.filter_editing = false;
                self.filter_text.clear();
                self.invalidate_filter_cache();
                self.clamp_history_selection();
            }
            Action::PromptKillProcess(pid) => {
                self.mode = AppMode::ConfirmKillProcess(pid);
            }
            Action::KillProcess(pid) => {
                use std::process::Command;
                let result = Command::new("kill").arg(pid.to_string()).status();
                match result {
                    Ok(status) if status.success() => {
                        self.flash = Some((format!("Sent SIGTERM to PID {pid}"), 20));
                    }
                    _ => {
                        self.flash = Some((format!("Failed to kill PID {pid}"), 20));
                    }
                }
                self.mode = AppMode::Table;
            }
            Action::CancelKill => {
                self.mode = AppMode::Table;
                self.flash = None;
            }
        }
    }

    pub(crate) fn build_row_infos(&self) -> Vec<RowInfo> {
        self.active_order
            .iter()
            .cloned()
            .map(|run_id| RowInfo { run_id })
            .collect()
    }

    fn mutate_scroll(&mut self, f: impl Fn(&mut usize, &mut bool, usize)) {
        let height = self.detail_inner_height;
        match &mut self.mode {
            AppMode::Detail(detail) => f(&mut detail.scroll, &mut detail.auto_scroll, height),
            AppMode::ThreadDetail(detail) => f(&mut detail.scroll, &mut detail.auto_scroll, height),
            _ => {}
        }
    }

    fn clamp_history_selection(&mut self) {
        let count = self.build_history_display_rows().len();
        if count == 0 {
            self.history_selected = 0;
        } else if self.history_selected >= count {
            self.history_selected = count - 1;
        }
    }
}
