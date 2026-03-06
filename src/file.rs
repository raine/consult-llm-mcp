use std::fs;
use std::path::PathBuf;

pub fn process_files(files: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    let cwd = std::env::current_dir()?;
    let resolved: Vec<PathBuf> = files
        .iter()
        .map(|f| {
            let p = PathBuf::from(f);
            if p.is_absolute() { p } else { cwd.join(f) }
        })
        .collect();

    let missing: Vec<&str> = files
        .iter()
        .zip(&resolved)
        .filter(|(_, r)| !r.exists())
        .map(|(orig, _)| orig.as_str())
        .collect();

    if !missing.is_empty() {
        anyhow::bail!("Files not found: {}", missing.join(", "));
    }

    let mut result = Vec::new();
    for (orig, resolved_path) in files.iter().zip(&resolved) {
        let content = fs::read_to_string(resolved_path)?;
        result.push((orig.clone(), content));
    }
    Ok(result)
}
