pub(crate) enum Action {
    Quit,
    ToggleFocus,
    MoveDown,
    MoveUp,
    EnterDetail(String),
    ExitDetail,
    ScrollDown,
    ScrollUp,
    HalfPageDown,
    HalfPageUp,
    ScrollToBottom,
    Flash(String, u8),
    PromptClearHistory,
    ClearHistory,
    CancelClear,
    ToggleHelp,
    YankResponse,
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
}
