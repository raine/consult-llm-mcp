use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

/// Read complete JSONL lines from a file starting at `offset`, deserializing
/// each into `T`. Stops at EOF or a partial (incomplete) line. Updates
/// `offset` to reflect the bytes consumed so the next call resumes where
/// this one left off.
pub fn read_jsonl_from_offset<T: DeserializeOwned>(path: &Path, offset: &mut u64) -> Vec<T> {
    let mut items = Vec::new();
    let Ok(file) = File::open(path) else {
        return items;
    };
    let mut reader = BufReader::new(file);
    let _ = reader.seek(SeekFrom::Start(*offset));

    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) => break,
            Ok(bytes_read) => {
                if !buf.ends_with('\n') {
                    break; // partial write — wait for next poll
                }
                *offset += bytes_read as u64;
                if let Ok(item) = serde_json::from_str(buf.trim()) {
                    items.push(item);
                }
            }
            Err(_) => break,
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_complete_lines_and_skips_partial() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut f = File::create(&path).unwrap();
        writeln!(f, r#"{{"a":1}}"#).unwrap();
        writeln!(f, r#"{{"a":2}}"#).unwrap();
        write!(f, r#"{{"a":3}}"#).unwrap(); // no trailing newline — partial
        f.flush().unwrap();

        let mut offset = 0u64;
        let items: Vec<serde_json::Value> = read_jsonl_from_offset(&path, &mut offset);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["a"], 1);
        assert_eq!(items[1]["a"], 2);
        assert!(offset > 0);

        // Complete the partial line and read again
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(f).unwrap();
        f.flush().unwrap();

        let items: Vec<serde_json::Value> = read_jsonl_from_offset(&path, &mut offset);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["a"], 3);
    }
}
