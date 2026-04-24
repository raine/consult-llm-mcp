const TEMPLATE: &str = r#"# consult-llm user config
# See: https://github.com/raine/consult-llm-mcp#configuration

# default_model: gemini
# allowed_models: [gemini, openai]
# extra_models: []

# gemini:
#   backend: gemini-cli
# openai:
#   backend: codex-cli
#   reasoning_effort: high
# opencode:
#   default_provider: copilot
"#;

pub fn run() -> anyhow::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let dir = home.join(".consult-llm");
    let path = dir.join("config.yaml");
    if path.exists() {
        println!("{} already exists", path.display());
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, TEMPLATE)?;
    println!("wrote {}", path.display());
    Ok(())
}
