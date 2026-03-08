pub(crate) enum Action {
    Quit,
    ToggleFocus,
    MoveDown,
    MoveUp,
    EnterDetail(String),
    ExitDetail,
    ScrollDown,
    ScrollUp,
    ScrollToBottom,
    Flash(String, u8),
}
