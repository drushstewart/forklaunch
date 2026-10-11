//! Content ownership lets regeneration prune obsolete modules without deleting edits.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::*;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ownership {
    format_version: u8,
    #[serde(default)]
    registry_exported: bool,
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
    registry_exported: bool,
) -> Result<()> {
    let directory = package(name, target);
    let previous = read_ownership(root, name, target)?
        .map(|record| record.files)
        .unwrap_or_default();
    let mut owned: BTreeMap<_, _> = files
        .iter()
        .filter(|(path, _)| path.starts_with(&directory))
        .map(|(path, content)| (path.clone(), hash(content.as_bytes())))
        .collect();
    for (relative, expected) in previous {
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
    stage_record(root, files, name, target, registry_exported, owned)
}

fn marker(name: &str, target: &str) -> PathBuf {
    PathBuf::from(format!(
        ".forklaunch/socket-sdk-outputs/{name}/{target}.json"
    ))
}

fn read_ownership(root: &Path, name: &str, target: &str) -> Result<Option<Ownership>> {
    let marker = marker(name, target);
    validate_destination(root, &marker)?;
    match fs::File::open(root.join(&marker)) {
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
            let directory = (target != "asyncapi").then(|| package(name, target));
            if target == "asyncapi" && previous.files.len() > 1 {
                bail!("Invalid socket AsyncAPI ownership record");
            }
            for (relative, expected) in &previous.files {
                if directory
                    .as_ref()
                    .is_some_and(|directory| !relative.starts_with(directory))
                    || expected.len() != 64
                    || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    bail!("Invalid socket SDK ownership entry");
                }
                validate_destination(root, relative)?;
            }
            Ok(Some(previous))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn stage_asyncapi(
    root: &Path,
    files: &mut BTreeMap<PathBuf, String>,
    removals: &mut BTreeMap<PathBuf, String>,
    name: &str,
    output: &Path,
    registry_exported: bool,
) -> Result<()> {
    let normalized: PathBuf = output
        .components()
        .filter(|component| !matches!(component, std::path::Component::CurDir))
        .collect();
    let output = normalized.as_path();
    let content = files
        .get(output)
        .context("Staged socket AsyncAPI missing")?;
    let owned = BTreeMap::from([(output.to_path_buf(), hash(content.as_bytes()))]);
    if let Some(previous) = read_ownership(root, name, "asyncapi")? {
        for (relative, expected) in previous.files {
            if relative != output && unchanged(root, &relative, &expected)? {
                removals.insert(relative, expected);
            }
        }
    }
    stage_record(root, files, name, "asyncapi", registry_exported, owned)
}

pub(super) fn stage_empty_asyncapi(
    root: &Path,
    files: &mut BTreeMap<PathBuf, String>,
    removals: &mut BTreeMap<PathBuf, String>,
    name: &str,
) -> Result<()> {
    if let Some(previous) = read_ownership(root, name, "asyncapi")? {
        if previous.registry_exported {
            for (relative, expected) in previous.files {
                if unchanged(root, &relative, &expected)? {
                    removals.insert(relative, expected);
                }
            }
            stage_record(root, files, name, "asyncapi", true, BTreeMap::new())?;
        }
    }
    Ok(())
}

/// Explicit project deletion retires its generated outputs even for offline
/// artifacts. Hashes retain application edits, custom compiler files and aliases.
pub(super) fn cleanup_service(root: &Path, name: &str) -> Result<bool> {
    let mut removals = BTreeMap::new();
    let mut rust_owned = false;
    for target in ["typescript", "rust", "asyncapi"] {
        let Some(previous) = read_ownership(root, name, target)? else {
            continue;
        };
        rust_owned |= target == "rust";
        removals.extend(previous.files);
        let marker = marker(name, target);
        removals.insert(marker.clone(), hash(&fs::read(root.join(marker))?));
    }
    remove_unchanged(root, &removals)?;
    Ok(rust_owned)
}

fn stage_record(
    root: &Path,
    files: &mut BTreeMap<PathBuf, String>,
    name: &str,
    target: &str,
    registry_exported: bool,
    owned: BTreeMap<PathBuf, String>,
) -> Result<()> {
    let marker = marker(name, target);
    validate_destination(root, &marker)?;
    stage(
        files,
        marker,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&Ownership {
                format_version: 1,
                registry_exported,
                files: owned,
            })?
        ),
    )
}

fn read_json(root: &Path, relative: &Path, limit: u64) -> Result<Option<Value>> {
    validate_destination(root, relative)?;
    let file = match fs::File::open(root.join(relative)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("Socket ownership input exceeds its size limit");
    }
    Ok(Some(serde_json::from_slice(&bytes)?))
}

pub(super) fn registry_exported(root: &Path, name: &str, artifact: &Path) -> Result<bool> {
    let Some(marker) = read_json(
        root,
        Path::new(&format!(
            ".forklaunch/socket-contracts/{name}/registry-export.json"
        )),
        4096,
    )?
    else {
        return Ok(false);
    };
    let output = marker["outputPath"]
        .as_str()
        .context("Invalid registry export path")?;
    let expected = marker["sha256"]
        .as_str()
        .context("Invalid registry export hash")?;
    validate_destination(root, Path::new(output))?;
    Ok(marker["formatVersion"] == 1
        && root.join(output).canonicalize()? == artifact
        && unchanged(root, Path::new(output), expected)?)
}

/// A missing artifact alone may be a transient editor operation. Only prune
/// after a previously owned registry export and a fresh, explicitly empty
/// framework contract agree that the last registered socket was removed.
pub(super) fn stage_empty_registry(
    root: &Path,
    files: &mut BTreeMap<PathBuf, String>,
    removals: &mut BTreeMap<PathBuf, String>,
    name: &str,
    target: &str,
    project: &toml::Value,
) -> Result<bool> {
    let Some(previous) = read_ownership(root, name, target)? else {
        return Ok(false);
    };
    if !previous.registry_exported {
        return Ok(false);
    }
    let contract = project
        .get("metadata")
        .and_then(|metadata| metadata.get("contractArtifactPath"))
        .and_then(toml::Value::as_str)
        .map_or_else(
            || format!(".forklaunch/contracts/{name}/contract.json"),
            str::to_owned,
        );
    let Some(contract) = read_json(root, Path::new(&contract), 16 * 1024 * 1024)? else {
        return Ok(false);
    };
    if contract["generator"] != "framework-rs"
        || contract["service"]["name"] != name
        || contract.get("sockets").is_some_and(|sockets| {
            !sockets
                .as_object()
                .is_some_and(|sockets| sockets.is_empty())
        })
    {
        return Ok(false);
    }
    for (relative, expected) in previous.files {
        if unchanged(root, &relative, &expected)? {
            removals.insert(relative, expected);
        }
    }
    stage_record(root, files, name, target, true, BTreeMap::new())?;
    Ok(true)
}

pub(super) fn remove_unchanged(root: &Path, removals: &BTreeMap<PathBuf, String>) -> Result<()> {
    for (relative, expected) in removals {
        if unchanged(root, relative, expected)? {
            fs::remove_file(root.join(relative))?;
        }
    }
    let mut directories = std::collections::BTreeSet::new();
    for relative in removals.keys() {
        let mut parent = relative.parent();
        while let Some(directory) = parent {
            if directory.components().count() < 3
                || !(directory.starts_with("sdks/rust")
                    || directory.starts_with("sdks/typescript-websocket"))
            {
                break;
            }
            directories.insert(directory.to_path_buf());
            parent = directory.parent();
        }
    }
    for relative in directories.iter().rev() {
        let directory = root.join(relative);
        if directory.canonicalize().ok().as_ref() != Some(&directory) {
            continue;
        }
        match fs::remove_dir(directory) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => return Err(error.into()),
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
    fn empty_registry_cleanup_requires_positive_provenance_and_fresh_contract() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let source = package("chat", "rust").join("src/socket_chat.rs");
        let mut files = BTreeMap::from([(source.clone(), "original".into())]);
        stage_package(
            &root,
            &mut files,
            &mut BTreeMap::new(),
            "chat",
            "rust",
            false,
        )
        .unwrap();
        write_staged(&root, &files);
        let contract = root.join(".forklaunch/contracts/chat/contract.json");
        fs::create_dir_all(contract.parent().unwrap()).unwrap();
        fs::write(
            &contract,
            r#"{"generator":"framework-rs","service":{"name":"chat"}}"#,
        )
        .unwrap();
        let project = toml::Value::Table(Default::default());
        let mut staged = BTreeMap::new();
        let mut removals = BTreeMap::new();
        assert!(
            !stage_empty_registry(&root, &mut staged, &mut removals, "chat", "rust", &project)
                .unwrap()
        );
        files.retain(|path, _| path == &source);
        stage_package(
            &root,
            &mut files,
            &mut BTreeMap::new(),
            "chat",
            "rust",
            true,
        )
        .unwrap();
        write_staged(&root, &files);
        fs::write(
            &contract,
            r#"{"generator":"framework-rs","service":{"name":"chat"},"sockets":{"chat":{}}}"#,
        )
        .unwrap();
        assert!(
            !stage_empty_registry(&root, &mut staged, &mut removals, "chat", "rust", &project)
                .unwrap()
        );
        fs::write(
            &contract,
            r#"{"generator":"framework-rs","service":{"name":"chat"},"sockets":{}}"#,
        )
        .unwrap();
        assert!(
            stage_empty_registry(&root, &mut staged, &mut removals, "chat", "rust", &project)
                .unwrap()
        );
        assert!(root.join(&source).exists());
        remove_unchanged(&root, &removals).unwrap();
        write_staged(&root, &staged);
        assert!(!root.join(package("chat", "rust")).exists());
        let mut repeated = BTreeMap::new();
        assert!(
            stage_empty_registry(
                &root,
                &mut repeated,
                &mut BTreeMap::new(),
                "chat",
                "rust",
                &project
            )
            .unwrap()
        );
        assert_eq!(repeated, staged);
    }

    #[test]
    fn explicit_service_cleanup_preserves_edits_and_prunes_owned_asyncapi() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let source = package("chat", "rust").join("src/socket_chat.rs");
        let edited = package("chat", "rust").join("Cargo.toml");
        let manual = package("chat", "rust").join("src/user.rs");
        let mut files = BTreeMap::from([
            (source.clone(), "original".into()),
            (edited.clone(), "generated".into()),
        ]);
        stage_package(
            &root,
            &mut files,
            &mut BTreeMap::new(),
            "chat",
            "rust",
            false,
        )
        .unwrap();
        let api = PathBuf::from("custom/asyncapi.json");
        files.insert(api.clone(), "{}".into());
        stage_asyncapi(
            &root,
            &mut files,
            &mut BTreeMap::new(),
            "chat",
            Path::new("./custom/asyncapi.json"),
            false,
        )
        .unwrap();
        write_staged(&root, &files);
        fs::write(root.join(&edited), "user edits").unwrap();
        fs::write(root.join(&manual), "user file").unwrap();
        assert!(cleanup_service(&root, "chat").unwrap());
        assert!(!root.join(source).exists());
        assert!(!root.join(api).exists());
        assert!(!root.join(marker("chat", "rust")).exists());
        assert_eq!(fs::read_to_string(root.join(edited)).unwrap(), "user edits");
        assert_eq!(fs::read_to_string(root.join(manual)).unwrap(), "user file");
        assert!(!cleanup_service(&root, "chat").unwrap());
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
            false,
        )
        .unwrap();
        write_staged(&root, &first);
        fs::write(root.join(&edited), "user edits").unwrap();
        let mut next = BTreeMap::from([(base.join("src/socket_new.ts"), "new".into())]);
        let mut removals = BTreeMap::new();
        stage_package(&root, &mut next, &mut removals, "chat", "typescript", false).unwrap();
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
            false,
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
        stage_package(
            &root,
            &mut first,
            &mut BTreeMap::new(),
            "chat",
            "rust",
            false,
        )
        .unwrap();
        write_staged(&root, &first);
        let mut removals = BTreeMap::new();
        stage_package(
            &root,
            &mut BTreeMap::new(),
            &mut removals,
            "chat",
            "rust",
            false,
        )
        .unwrap();
        fs::write(root.join(&relative), "concurrent edits").unwrap();
        remove_unchanged(&root, &removals).unwrap();
        assert_eq!(
            fs::read_to_string(root.join(relative)).unwrap(),
            "concurrent edits"
        );
        let marker = root.join(".forklaunch/socket-sdk-outputs/chat/rust.json");
        fs::write(marker, serde_json::to_vec(&json!({"formatVersion":1,"files":{"sdks/rust/chat-websocket-sdk/../../../escape":hash(b"x")}})).unwrap()).unwrap();
        assert!(
            stage_package(
                &root,
                &mut BTreeMap::new(),
                &mut removals,
                "chat",
                "rust",
                false
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn does_not_prune_internal_symlink_aliases() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let relative = package("chat", "rust").join("src/socket_old.rs");
        let mut files = BTreeMap::from([(relative.clone(), "original".into())]);
        stage_package(
            &root,
            &mut files,
            &mut BTreeMap::new(),
            "chat",
            "rust",
            false,
        )
        .unwrap();
        write_staged(&root, &files);
        fs::rename(root.join(&relative), root.join("user.rs")).unwrap();
        std::os::unix::fs::symlink(root.join("user.rs"), root.join(&relative)).unwrap();
        let mut removals = BTreeMap::new();
        stage_package(
            &root,
            &mut BTreeMap::new(),
            &mut removals,
            "chat",
            "rust",
            false,
        )
        .unwrap();
        assert!(removals.is_empty());
        assert!(root.join(relative).exists());
    }
}
