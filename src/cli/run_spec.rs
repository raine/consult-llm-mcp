#[derive(Debug)]
pub struct RunSpec {
    pub model: String,
    pub thread_id: Option<String>,
    pub prompt_file: String,
}

impl std::str::FromStr for RunSpec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let mut model: Option<String> = None;
        let mut thread_id: Option<String> = None;
        let mut prompt_file: Option<String> = None;

        let mut remaining = s;
        while !remaining.is_empty() {
            let (key, rest) = remaining
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("expected key=value in --run {:?}", s))?;
            // Value ends at the next comma or end of string.
            // This means prompt-file paths containing commas are not supported.
            let (value, next) = match rest.find(',') {
                Some(i) => (&rest[..i], &rest[i + 1..]),
                None => (rest, ""),
            };
            match key.trim() {
                "model" => {
                    anyhow::ensure!(model.is_none(), "duplicate 'model' key in --run {:?}", s);
                    anyhow::ensure!(!value.is_empty(), "'model' value is empty in --run {:?}", s);
                    model = Some(value.to_string());
                }
                "thread" => {
                    anyhow::ensure!(
                        thread_id.is_none(),
                        "duplicate 'thread' key in --run {:?}",
                        s
                    );
                    anyhow::ensure!(
                        !value.trim().is_empty(),
                        "'thread' value is empty in --run {:?}",
                        s
                    );
                    thread_id = Some(value.to_string());
                }
                "prompt-file" => {
                    anyhow::ensure!(
                        prompt_file.is_none(),
                        "duplicate 'prompt-file' key in --run {:?}",
                        s
                    );
                    anyhow::ensure!(
                        !value.is_empty(),
                        "'prompt-file' value is empty in --run {:?}",
                        s
                    );
                    prompt_file = Some(value.to_string());
                }
                other => anyhow::bail!("unknown key {:?} in --run {:?}", other, s),
            }
            remaining = next;
        }

        Ok(RunSpec {
            model: model.ok_or_else(|| anyhow::anyhow!("--run missing 'model=' in {:?}", s))?,
            prompt_file: prompt_file
                .ok_or_else(|| anyhow::anyhow!("--run missing 'prompt-file=' in {:?}", s))?,
            thread_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_spec_full() {
        let s: RunSpec = "model=gemini,thread=abc123,prompt-file=/tmp/g.md"
            .parse()
            .unwrap();
        assert_eq!(s.model, "gemini");
        assert_eq!(s.thread_id.as_deref(), Some("abc123"));
        assert_eq!(s.prompt_file, "/tmp/g.md");
    }

    #[test]
    fn run_spec_no_thread() {
        let s: RunSpec = "model=openai,prompt-file=/tmp/p.md".parse().unwrap();
        assert_eq!(s.model, "openai");
        assert!(s.thread_id.is_none());
        assert_eq!(s.prompt_file, "/tmp/p.md");
    }

    #[test]
    fn run_spec_duplicate_model_key() {
        let err = "model=gemini,model=openai,prompt-file=/tmp/p.md"
            .parse::<RunSpec>()
            .unwrap_err();
        assert!(err.to_string().contains("duplicate 'model'"));
    }

    #[test]
    fn run_spec_missing_model() {
        let err = "prompt-file=/tmp/p.md".parse::<RunSpec>().unwrap_err();
        assert!(err.to_string().contains("missing 'model='"));
    }

    #[test]
    fn run_spec_missing_prompt_file() {
        let err = "model=gemini".parse::<RunSpec>().unwrap_err();
        assert!(err.to_string().contains("missing 'prompt-file='"));
    }

    #[test]
    fn run_spec_unknown_key() {
        let err = "model=gemini,foo=bar,prompt-file=/tmp/p.md"
            .parse::<RunSpec>()
            .unwrap_err();
        assert!(err.to_string().contains("unknown key"));
    }

    #[test]
    fn run_spec_empty_model_value() {
        let err = "model=,prompt-file=/tmp/p.md"
            .parse::<RunSpec>()
            .unwrap_err();
        assert!(err.to_string().contains("'model' value is empty"));
    }

    #[test]
    fn run_spec_empty_prompt_file_value() {
        let err = "model=gemini,prompt-file=".parse::<RunSpec>().unwrap_err();
        assert!(err.to_string().contains("'prompt-file' value is empty"));
    }

    #[test]
    fn run_spec_empty_thread_value() {
        let err = "model=gemini,thread=,prompt-file=/tmp/p.md"
            .parse::<RunSpec>()
            .unwrap_err();
        assert!(err.to_string().contains("'thread' value is empty"));
    }
}
