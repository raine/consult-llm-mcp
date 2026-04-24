use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const SKILLS: &[(&str, &str)] = &[
    ("collab", include_str!("../../../skills/collab/SKILL.md")),
    (
        "collab-vs",
        include_str!("../../../skills/collab-vs/SKILL.md"),
    ),
    ("consult", include_str!("../../../skills/consult/SKILL.md")),
    (
        "consult-llm",
        include_str!("../../../skills/consult-llm/SKILL.md"),
    ),
    ("debate", include_str!("../../../skills/debate/SKILL.md")),
    (
        "debate-vs",
        include_str!("../../../skills/debate-vs/SKILL.md"),
    ),
];

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq)]
pub enum PlatformArg {
    Claude,
    Opencode,
    Codex,
}

#[derive(clap::Args, Debug)]
pub struct InstallSkillsArgs {
    /// Target a specific platform (default: all detected)
    #[arg(long = "platform", value_enum)]
    pub platform: Option<PlatformArg>,
}

struct Platform {
    name: &'static str,
    arg: PlatformArg,
    /// Parent dir checked for auto-detection (e.g. ~/.claude)
    parent: PathBuf,
    /// Skills dir to install into (e.g. ~/.claude/skills)
    skills_dir: PathBuf,
}

impl Platform {
    fn new(name: &'static str, arg: PlatformArg, parent: PathBuf) -> Self {
        let skills_dir = parent.join("skills");
        Self {
            name,
            arg,
            parent,
            skills_dir,
        }
    }
}

fn all_platforms(home: &Path) -> Vec<Platform> {
    let config_dir = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
    vec![
        Platform::new("Claude Code", PlatformArg::Claude, home.join(".claude")),
        Platform::new(
            "OpenCode",
            PlatformArg::Opencode,
            config_dir.join("opencode"),
        ),
        Platform::new("Codex", PlatformArg::Codex, home.join(".codex")),
    ]
}

pub fn run(args: InstallSkillsArgs) -> anyhow::Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let platforms = all_platforms(&home);

    let targets: Vec<&Platform> = platforms
        .iter()
        .filter(|p| match &args.platform {
            Some(target) => p.arg == *target,
            None => p.parent.is_dir(),
        })
        .collect();

    if targets.is_empty() {
        anyhow::bail!(
            "no supported platforms detected (expected ~/.claude, ~/.config/opencode, or ~/.codex)"
        );
    }

    let color = std::io::stdout().is_terminal()
        && std::env::var("NO_COLOR")
            .map(|v| v.is_empty())
            .unwrap_or(true);
    let mut any_failed = false;

    for platform in targets {
        let failed = install_platform(platform, color, &home);
        if failed {
            any_failed = true;
        }
    }

    if any_failed {
        anyhow::bail!("some skills failed to install");
    }

    Ok(())
}

/// Returns true if any skill failed to install.
fn install_platform(platform: &Platform, color: bool, home: &Path) -> bool {
    println!("==> {}", platform.name);
    let mut failed = false;

    for (name, content) in SKILLS {
        let skill_dir = platform.skills_dir.join(name);
        let dest = skill_dir.join("SKILL.md");

        let up_to_date = fs::read(&dest).is_ok_and(|b| b == content.as_bytes());

        if up_to_date {
            print_line("up-to-date", &dest, color, None, home);
            continue;
        }

        if let Err(e) = fs::create_dir_all(&skill_dir) {
            eprintln!("  error creating {}: {e}", shrink_path(&skill_dir, home));
            failed = true;
            continue;
        }
        if let Err(e) = fs::write(&dest, content.as_bytes()) {
            eprintln!("  error writing {}: {e}", shrink_path(&dest, home));
            failed = true;
            continue;
        }

        print_line("written", &dest, color, Some(32), home);
    }

    println!();
    failed
}

fn print_line(status: &str, path: &Path, color: bool, ansi_color: Option<u8>, home: &Path) {
    let display = shrink_path(path, home);
    if color {
        if let Some(code) = ansi_color {
            println!("  \x1b[{code}m{status:<12}\x1b[0m {display}");
        } else {
            println!("  \x1b[2m{status:<12}\x1b[0m {display}");
        }
    } else {
        println!("  {status:<12} {display}");
    }
}

fn shrink_path(path: &Path, home: &Path) -> String {
    path.strip_prefix(home)
        .map(|rel| format!("~/{}", rel.display()))
        .unwrap_or_else(|_| path.display().to_string())
}
