pub(crate) enum Action {
    Quit,
    ToggleFocus,
    MoveDown,
    MoveUp,
    EnterDetail(String),
    /// Enter thread detail view for a thread_id
    EnterThreadDetail(String),
    ExitDetail,
    /// Jump to previous turn in thread detail
    PrevTurn,
    /// Jump to next turn in thread detail
    NextTurn,
    ScrollDown,
    ScrollUp,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    ScrollToBottom,
    ScrollToTop,
    Flash(String, u8),
    PromptClearHistory,
    ClearHistory,
    CancelClear,
    ToggleHelp,
    YankResponse,
    ToggleSystemPrompt,
    /// Open filter input (or re-open with current text)
    StartFilter,
    /// User typed a character in filter input
    FilterInput(char),
    /// Backspace in filter input
    FilterBackspace,
    /// Enter: dismiss filter input but keep filter active
    FilterAccept,
    /// Esc: dismiss filter input and clear the filter
    FilterCancel,
    /// Switch to next sibling consultation (same project, similar start time)
    NextSibling,
    /// Switch to previous sibling consultation
    PrevSibling,
}
