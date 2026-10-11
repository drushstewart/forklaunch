//! Content ownership lets regeneration prune obsolete modules without deleting edits.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::*;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ownership {
    format_version: u8,
    files: BTreeMap<PathBuf, String>,
}

fn package(name: &str, target: &str) -> PathBuf {
    match target {
        "rust" => PathBuf::from(format!("sdks/rust/{name}-websocket-sdk")),
        "typescript" => PathBuf::from(format!("sdks/typescript-websocket/{name}")),
        _ => unreachable!("validated SDK target"),
    }
}

fn hash(content: &[u8]) -> String {
    Sha256::digest(content)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// Aliases are user-owned: pruning through an internal symlink could remove an
// unrelated file even when its bytes happen to match a previous generated file.
fn unchanged(root: &Path, relative: &Path, expected: &str) -> Result<bool> {
    validate_destination(root, relative)?;
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= 64 * 1024 * 1024 => {}
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    }
    if path.canonicalize()? != path {
        return Ok(false);
    }
    Ok(hash(&fs::read(path)?) == expected)
}

pub(super) fn stage_package(
    root: &Path,
    files: &mut BTreeMap<PathBuf, String>,
    removals: &mut BTreeMap<PathBuf, String>,
    name: &str,
    target: &str,
) -> Result<()> {
    let directory = package(name, target);
    let marker = PathBuf::from(format!(
        ".forklaunch/socket-sdk-outputs/{name}/{target}.json"
    ));
    validate_destination(root, &marker)?;
    let previous = match fs::File::open(root.join(&marker)) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                bail!("Socket SDK ownership record exceeds 1 MiB");
            }
            let previous: Ownership =
                serde_json::from_slice(&bytes).context("Invalid socket SDK ownership record")?;
            if previous.format_version != 1 || previous.files.len() > 4096 {
                bail!("Unsupported socket SDK ownership record");
            }
            previous.files
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(error.into()),
    };
    let mut owned: BTreeMap<_, _> = files
        .iter()
        .filter(|(path, _)| path.starts_with(&directory))
        .map(|(path, content)| (path.clone(), hash(content.as_bytes())))
        .collect();
    for (relative, expected) in previous {
        if !relative.starts_with(&directory)
            || expected.len() != 64
            || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("Invalid socket SDK ownership entry");
        }
        validate_destination(root, &relative)?;
        if owned.contains_key(&relative) {
            continue;
        }
        // Keep the original hash when users edit compiler options: checking
        // generated code must respect those options, and later cleanup still
        // recognizes that the edited configuration belongs to the user.
        if target == "typescript" && relative == directory.join("tsconfig.json") {
            owned.insert(relative, expected);
        } else if unchanged(root, &relative, &expected)? {
            removals.insert(relative, expected);
        }
    }
    stage(
        files,
        marker,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&Ownership {
                format_version: 1,
                files: owned,
            })?
        ),
    )
}

pub(super) fn remove_unchanged(root: &Path, removals: &BTreeMap<PathBuf, String>) -> Result<()> {
    for (relative, expected) in removals {
        if unchanged(root, relative, expected)? {
            fs::remove_file(root.join(relative))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_staged(root: &Path, files: &BTreeMap<PathBuf, String>) {
        for (relative, content) in files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
    }

    #[test]
    fn prunes_only_unchanged_obsolete_modules_and_keeps_owned_config() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let base = package("chat", "typescript");
        let obsolete = base.join("src/socket_old.ts");
        let edited = base.join("src/socket_edited.ts");
        let config = base.join("tsconfig.json");
        let mut first = BTreeMap::from([
            (obsolete.clone(), "old".into()),
            (edited.clone(), "original".into()),
            (config.clone(), "{}".into()),
        ]);
        stage_package(
            &root,
            &mut first,
            &mut BTreeMap::new(),
            "chat",
            "typescript",
        )
        .unwrap();
        write_staged(&root, &first);
        fs::write(root.join(&edited), "user edits").unwrap();
        let mut next = BTreeMap::from([(base.join("src/socket_new.ts"), "new".into())]);
        let mut removals = BTreeMap::new();
        stage_package(&root, &mut next, &mut removals, "chat", "typescript").unwrap();
        assert_eq!(removals.keys().collect::<Vec<_>>(), vec![&obsolete]);
        assert!(root.join(&obsolete).exists(), "staging must be read-only");
        remove_unchanged(&root, &removals).unwrap();
        write_staged(&root, &next);
        assert!(!root.join(obsolete).exists());
        assert_eq!(fs::read_to_string(root.join(edited)).unwrap(), "user edits");
        assert_eq!(fs::read_to_string(root.join(config)).unwrap(), "{}");
        let mut repeat = BTreeMap::from([(base.join("src/socket_new.ts"), "new".into())]);
        stage_package(
            &root,
            &mut repeat,
            &mut BTreeMap::new(),
            "chat",
            "typescript",
        )
        .unwrap();
        assert_eq!(next, repeat, "regeneration should be stable");
    }

    #[test]
    fn rechecks_ownership_before_removal_and_rejects_escape_entries() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let relative = package("chat", "rust").join("src/socket_old.rs");
        let mut first = BTreeMap::from([(relative.clone(), "original".into())]);
        stage_package(&root, &mut first, &mut BTreeMap::new(), "chat", "rust").unwrap();
        write_staged(&root, &first);
        let mut removals = BTreeMap::new();
        stage_package(&root, &mut BTreeMap::new(), &mut removals, "chat", "rust").unwrap();
        fs::write(root.join(&relative), "concurrent edits").unwrap();
        remove_unchanged(&root, &removals).unwrap();
        assert_eq!(
            fs::read_to_string(root.join(relative)).unwrap(),
            "concurrent edits"
        );
        let marker = root.join(".forklaunch/socket-sdk-outputs/chat/rust.json");
        fs::write(marker, serde_json::to_vec(&json!({"formatVersion":1,"files":{"sdks/rust/chat-websocket-sdk/../../../escape":hash(b"x")}})).unwrap()).unwrap();
        assert!(stage_package(&root, &mut BTreeMap::new(), &mut removals, "chat", "rust").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn does_not_prune_internal_symlink_aliases() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let relative = package("chat", "rust").join("src/socket_old.rs");
        let mut files = BTreeMap::from([(relative.clone(), "original".into())]);
        stage_package(&root, &mut files, &mut BTreeMap::new(), "chat", "rust").unwrap();
        write_staged(&root, &files);
        fs::rename(root.join(&relative), root.join("user.rs")).unwrap();
        std::os::unix::fs::symlink(root.join("user.rs"), root.join(&relative)).unwrap();
        let mut removals = BTreeMap::new();
        stage_package(&root, &mut BTreeMap::new(), &mut removals, "chat", "rust").unwrap();
        assert!(removals.is_empty());
        assert!(root.join(relative).exists());
    }
}
