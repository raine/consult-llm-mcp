use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use inquire::error::InquireError;

const SKILL_SOURCES: &[(&str, &str)] = &[
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

#[derive(Debug, Clone)]
struct Skill {
    id: &'static str,
    description: String,
    content: &'static str,
}

static SKILLS: LazyLock<Vec<Skill>> = LazyLock::new(|| {
    SKILL_SOURCES
        .iter()
        .map(|(id, content)| Skill {
            id,
            description: parse_description(content).unwrap_or_default(),
            content,
        })
        .collect()
});

fn parse_description(content: &str) -> Option<String> {
    let body = content.strip_prefix("---\n")?;
    let (frontmatter, _) = body.split_once("\n---")?;
    let value: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    value
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
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
    vec![
        Platform::new("Claude Code", PlatformArg::Claude, home.join(".claude")),
        Platform::new(
            "OpenCode",
            PlatformArg::Opencode,
            home.join(".config").join("opencode"),
        ),
        Platform::new("Codex", PlatformArg::Codex, home.join(".codex")),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillStatus {
    Installed,
    Missing,
    Modified,
}

impl SkillStatus {
    fn for_skill(skill: &Skill, platform: &Platform) -> Self {
        let dest = platform.skills_dir.join(skill.id).join("SKILL.md");
        match fs::read(&dest) {
            Ok(bytes) if bytes == skill.content.as_bytes() => Self::Installed,
            Ok(_) => Self::Modified,
            Err(_) => Self::Missing,
        }
    }
}

pub fn run(args: InstallSkillsArgs) -> anyhow::Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let platforms = all_platforms(&home);

    let detected: Vec<&Platform> = platforms
        .iter()
        .filter(|p| match &args.platform {
            Some(target) => p.arg == *target,
            None => p.parent.is_dir(),
        })
        .collect();

    if detected.is_empty() {
        anyhow::bail!(
            "no supported platforms detected (expected ~/.claude, ~/.config/opencode, or ~/.codex)"
        );
    }

    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    if interactive {
        run_interactive(&detected, &home)
    } else {
        run_batch(&detected, &home)
    }
}

fn use_color() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var("NO_COLOR")
            .map(|v| v.is_empty())
            .unwrap_or(true)
}

fn run_batch(targets: &[&Platform], home: &Path) -> anyhow::Result<()> {
    let color = use_color();
    let mut any_failed = false;

    for platform in targets {
        println!("==> {}", platform.name);
        for skill in SKILLS.iter() {
            if !install_skill(skill, platform, color, home) {
                any_failed = true;
            }
        }
        println!();
    }

    if any_failed {
        anyhow::bail!("some skills failed to install");
    }
    Ok(())
}

fn run_interactive(detected: &[&Platform], home: &Path) -> anyhow::Result<()> {
    let selected_platforms: Vec<&Platform> = if detected.len() == 1 {
        vec![detected[0]]
    } else {
        let options: Vec<PlatformChoice> = detected
            .iter()
            .enumerate()
            .map(|(idx, p)| PlatformChoice { idx, name: p.name })
            .collect();
        let chosen = match inquire::MultiSelect::new("Install to which platforms?", options)
            .with_page_size(10)
            .prompt()
        {
            Ok(c) => c,
            Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        if chosen.is_empty() {
            println!("No platforms selected.");
            return Ok(());
        }
        chosen.into_iter().map(|c| detected[c.idx]).collect()
    };

    let multi_platform = selected_platforms.len() > 1;
    let max_id_len = SKILLS.iter().map(|s| s.id.len()).max().unwrap_or(0);

    let items: Vec<SkillItem> = SKILLS
        .iter()
        .enumerate()
        .map(|(idx, skill)| {
            let per_platform: Vec<(&'static str, SkillStatus)> = selected_platforms
                .iter()
                .map(|p| (p.name, SkillStatus::for_skill(skill, p)))
                .collect();
            SkillItem {
                idx,
                id: skill.id,
                description: skill.description.clone(),
                per_platform,
                show_platform: multi_platform,
                id_pad: max_id_len,
                color: use_color(),
            }
        })
        .collect();

    // Default-select only when at least one selected platform is missing the
    // skill AND no selected platform has a locally modified copy. This avoids
    // silently overwriting modifications when one platform is also missing.
    let defaults: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let any_missing = item
                .per_platform
                .iter()
                .any(|(_, s)| *s == SkillStatus::Missing);
            let any_modified = item
                .per_platform
                .iter()
                .any(|(_, s)| *s == SkillStatus::Modified);
            any_missing && !any_modified
        })
        .map(|(i, _)| i)
        .collect();

    let chosen = match inquire::MultiSelect::new("Select skills to install", items)
        .with_default(&defaults)
        .with_page_size(SKILLS.len().min(15))
        .prompt()
    {
        Ok(c) => c,
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    if chosen.is_empty() {
        println!("No skills selected.");
        return Ok(());
    }

    let color = use_color();
    let mut any_failed = false;

    for platform in &selected_platforms {
        println!("==> {}", platform.name);
        for item in &chosen {
            let skill = &SKILLS[item.idx];
            if !install_skill(skill, platform, color, home) {
                any_failed = true;
            }
        }
        println!();
    }

    if any_failed {
        anyhow::bail!("some skills failed to install");
    }
    Ok(())
}

#[derive(Clone)]
struct PlatformChoice {
    idx: usize,
    name: &'static str,
}

impl std::fmt::Display for PlatformChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

#[derive(Clone)]
struct SkillItem {
    idx: usize,
    id: &'static str,
    description: String,
    per_platform: Vec<(&'static str, SkillStatus)>,
    show_platform: bool,
    id_pad: usize,
    color: bool,
}

impl SkillItem {
    fn render_status(&self) -> String {
        // Group platform names by status. When only one platform is selected,
        // omit the platform name and just show the bare label.
        use std::collections::BTreeMap;
        let mut groups: BTreeMap<u8, Vec<&'static str>> = BTreeMap::new();
        for (name, status) in &self.per_platform {
            let key = match status {
                SkillStatus::Missing => 0u8,
                SkillStatus::Modified => 1,
                SkillStatus::Installed => 2,
            };
            groups.entry(key).or_default().push(name);
        }
        let parts: Vec<String> = groups
            .into_iter()
            .map(|(key, names)| {
                let (label, code) = match key {
                    0 => ("missing", 33u8),
                    1 => ("modified", 31),
                    _ => ("installed", 32),
                };
                let body = if self.show_platform {
                    format!("{}: {}", label, names.join(", "))
                } else {
                    label.to_string()
                };
                if self.color {
                    format!("\x1b[{code}m{body}\x1b[0m")
                } else {
                    body
                }
            })
            .collect();
        parts.join(" | ")
    }
}

impl std::fmt::Display for SkillItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let desc = if self.description.is_empty() {
            String::new()
        } else {
            format!("  {}", self.description)
        };
        write!(
            f,
            "{:<width$}  {}{}",
            self.id,
            self.render_status(),
            desc,
            width = self.id_pad
        )
    }
}

/// Returns true on success.
fn install_skill(skill: &Skill, platform: &Platform, color: bool, home: &Path) -> bool {
    let skill_dir = platform.skills_dir.join(skill.id);
    let dest = skill_dir.join("SKILL.md");

    let up_to_date = fs::read(&dest).is_ok_and(|b| b == skill.content.as_bytes());
    if up_to_date {
        print_line("up-to-date", &dest, color, None, home);
        return true;
    }

    if let Err(e) = fs::create_dir_all(&skill_dir) {
        eprintln!("  error creating {}: {e}", shrink_path(&skill_dir, home));
        return false;
    }
    if let Err(e) = fs::write(&dest, skill.content.as_bytes()) {
        eprintln!("  error writing {}: {e}", shrink_path(&dest, home));
        return false;
    }

    print_line("written", &dest, color, Some(32), home);
    true
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
