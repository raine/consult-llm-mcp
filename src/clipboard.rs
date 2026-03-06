use arboard::Clipboard;

pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let mut clipboard =
        Clipboard::new().map_err(|e| anyhow::anyhow!("Failed to copy prompt to clipboard: {e}"))?;
    clipboard
        .set_text(text)
        .map_err(|e| anyhow::anyhow!("Failed to copy prompt to clipboard: {e}"))?;
    Ok(())
}
