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
}
