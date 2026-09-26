//! L2 markdown project memory: plain files under a project dir (output, not runtime state).

pub fn save(dir: &std::path::Path, name: &str, body: &str) -> Result<(), String> {
    if name.contains('/') || name.contains("..") {
        return Err("refusing path escape in memory name".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{name}.md")), body).map_err(|e| e.to_string())
}

pub fn load(dir: &std::path::Path, name: &str) -> Result<String, String> {
    if name.contains('/') || name.contains("..") {
        return Err("refusing path escape in memory name".into());
    }
    std::fs::read_to_string(dir.join(format!("{name}.md")).as_path()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_escape_refused() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "plan", "# hello").unwrap();
        assert_eq!(load(dir.path(), "plan").unwrap(), "# hello");
        assert!(save(dir.path(), "../evil", "x").is_err());
    }
}
