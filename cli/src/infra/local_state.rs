//! Local, gitignored undo state for native capability edits.
//!
//! `infra add|remove` on a native service records the files it changed so a
//! later removal can restore what the developer had. The records may contain
//! local settings, so they never enter source control. They live in one file,
//! `.forklaunch/local-state.json`, keyed `<service>/<capability>` (application
//! wide records use the `_app/` prefix). Earlier releases wrote one file per
//! record under `.forklaunch/native-capabilities/`; callers still address
//! records by those paths, and both forms are read here.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::core::forklaunch_state::LOCAL_STATE;

pub(crate) const LEGACY_DIR: &str = ".forklaunch/native-capabilities";
const IGNORE_RULE: &str = "/.forklaunch/local-state.json";

/// The record key for a path under the legacy directory, or `None` for any
/// other path. Its `.gitignore` maps to the empty key and is never stored.
pub(crate) fn key(root: &Path, path: &Path) -> Option<String> {
    let name = path.strip_prefix(root.join(LEGACY_DIR)).ok()?.to_str()?;
    if name.contains('/') {
        return None;
    }
    if name == ".gitignore" {
        return Some(String::new());
    }
    let stem = name.strip_suffix(".json")?;
    Some(if let Some(shared) = stem.strip_prefix('.') {
        format!("_app/{shared}")
    } else if let Some((service, capability)) = stem.split_once('.') {
        format!("{service}/{capability}")
    } else {
        format!("{stem}/capabilities")
    })
}

fn legacy_path(root: &Path, key: &str) -> PathBuf {
    let (owner, record) = key.split_once('/').unwrap_or((key, ""));
    let name = match (owner, record) {
        ("_app", record) => format!(".{record}.json"),
        (service, "capabilities") => format!("{service}.json"),
        (service, record) => format!("{service}.{record}.json"),
    };
    root.join(LEGACY_DIR).join(name)
}

fn load(root: &Path) -> Result<Map<String, Value>> {
    let path = root.join(LOCAL_STATE);
    match fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse {}", path.display()))?
        {
            Value::Object(map) => Ok(map),
            _ => anyhow::bail!("{} must be a JSON object", path.display()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(error.into()),
    }
}

/// One record as the text its owner wrote, from the consolidated file or a
/// not-yet-migrated legacy file.
pub(crate) fn read(root: &Path, key: &str) -> Result<Option<String>> {
    if key.is_empty() {
        return Ok(None);
    }
    if let Some(value) = load(root)?.get(key) {
        return Ok(Some(serde_json::to_string_pretty(value)? + "\n"));
    }
    let legacy = legacy_path(root, key);
    match fs::read_to_string(&legacy) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {legacy:?}")),
    }
}

fn write_private(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
    }
    options.open(path)?.write_all(content.as_bytes())?;
    Ok(())
}

fn ensure_ignored(root: &Path) -> Result<()> {
    let path = root.join(".gitignore");
    let current = fs::read_to_string(&path).unwrap_or_default();
    if current
        .lines()
        .any(|line| matches!(line.trim(), IGNORE_RULE | ".forklaunch/local-state.json"))
    {
        return Ok(());
    }
    let separator = if current.is_empty() || current.ends_with('\n') { "" } else { "\n" };
    fs::write(
        path,
        format!("{current}{separator}# ForkLaunch local undo state (may hold local settings)\n{IGNORE_RULE}\n"),
    )?;
    Ok(())
}

/// Fold every legacy record file into the consolidated file and remove the
/// legacy directory's generated files.
fn migrate(root: &Path, state: &mut Map<String, Value>) -> Result<bool> {
    let directory = root.join(LEGACY_DIR);
    let Ok(entries) = fs::read_dir(&directory) else {
        return Ok(false);
    };
    let mut migrated = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(record) = key(root, &path) else { continue };
        if !record.is_empty() {
            let value: Value = serde_json::from_str(&fs::read_to_string(&path)?)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
            state.entry(record).or_insert(value);
        }
        migrated.push(path);
    }
    for path in &migrated {
        fs::remove_file(path)?;
    }
    if fs::read_dir(&directory).is_ok_and(|mut entries| entries.next().is_none()) {
        fs::remove_dir(&directory)?;
    }
    Ok(!migrated.is_empty())
}

/// Apply staged record changes (`None` deletes) in one write.
pub(crate) fn commit(root: &Path, changes: BTreeMap<String, Option<String>>) -> Result<()> {
    let mut state = load(root)?;
    let migrated = migrate(root, &mut state)?;
    let changes: Vec<_> = changes.into_iter().filter(|(key, _)| !key.is_empty()).collect();
    if changes.is_empty() && !migrated {
        return Ok(());
    }
    for (key, content) in changes {
        match content {
            Some(content) => {
                let value: Value = serde_json::from_str(&content)
                    .with_context(|| format!("Local state record {key} must be JSON"))?;
                state.insert(key, value);
            }
            None => {
                state.remove(&key);
            }
        }
    }
    let path = root.join(LOCAL_STATE);
    if state.is_empty() {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    ensure_ignored(root)?;
    write_private(&path, &(serde_json::to_string_pretty(&state)? + "\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_legacy_record_names() {
        let root = Path::new("/app");
        let dir = root.join(LEGACY_DIR);
        assert_eq!(key(root, &dir.join("notes.infra-s3.json")).as_deref(), Some("notes/infra-s3"));
        assert_eq!(key(root, &dir.join("notes.json")).as_deref(), Some("notes/capabilities"));
        assert_eq!(key(root, &dir.join(".infra-redis.json")).as_deref(), Some("_app/infra-redis"));
        assert_eq!(key(root, &dir.join(".gateway-mock.json")).as_deref(), Some("_app/gateway-mock"));
        assert_eq!(key(root, &dir.join(".gitignore")).as_deref(), Some(""));
        assert_eq!(key(root, &root.join("src/notes.json")), None);
        for record in ["notes/infra-s3", "notes/capabilities", "_app/infra-redis"] {
            assert_eq!(key(root, &legacy_path(root, record)).as_deref(), Some(record));
        }
    }

    #[test]
    fn legacy_records_migrate_into_one_ignored_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let dir = root.join(LEGACY_DIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".gitignore"), "/*.json\n").unwrap();
        fs::write(dir.join("notes.infra-s3.json"), "{\"last_env\":\"A=1\"}").unwrap();
        fs::write(dir.join(".infra-s3.json"), "{\"original\":null}").unwrap();
        assert_eq!(read(root, "notes/infra-s3").unwrap().unwrap(), "{\"last_env\":\"A=1\"}");
        commit(root, BTreeMap::from([("notes/infra-redis".to_owned(), Some("{\"x\":1}".to_owned()))])).unwrap();
        assert!(!dir.exists());
        let state = load(root).unwrap();
        assert_eq!(state["notes/infra-s3"]["last_env"], "A=1");
        assert!(state.contains_key("_app/infra-s3"));
        assert_eq!(state["notes/infra-redis"]["x"], 1);
        assert!(fs::read_to_string(root.join(".gitignore")).unwrap().contains(IGNORE_RULE));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(root.join(LOCAL_STATE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        commit(root, BTreeMap::from([
            ("notes/infra-s3".to_owned(), None),
            ("notes/infra-redis".to_owned(), None),
            ("_app/infra-s3".to_owned(), None),
        ])).unwrap();
        assert!(!root.join(LOCAL_STATE).exists());
    }
}
