//! Application-local blueprint crates. Only crates reachable from the
//! application's own manifests, plus their path dependencies, are written.
//! A crate that is no longer reachable is removed only while it still matches
//! the generated source; edited copies are kept and reported.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

static BLUEPRINT_CRATES: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/src/templates/rust/blueprint-workspace/crates");
const BLUEPRINT_CARGO: &str =
    include_str!("../../templates/rust/blueprint-workspace/Cargo.toml.template");
const DESTINATION: &str = ".forklaunch/blueprint-rs";
const MANIFEST: &str = "Cargo.toml.template";

/// Embedded source of one blueprint crate, relative to `crates/`.
pub(super) fn crate_source(name: &str) -> Option<&'static Dir<'static>> {
    BLUEPRINT_CRATES.get_dir(name)
}

/// Resolve `..` without touching the file system; dependencies may not exist yet.
pub(super) fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    normalized
}

/// The embedded crate (relative to `crates/`) that contains a dependency path.
fn crate_of(root: &Path, dependency: &Path) -> Option<String> {
    let crates = normalize(&root.join(DESTINATION).join("crates"));
    let relative = normalize(dependency).strip_prefix(&crates).ok()?.to_path_buf();
    relative
        .ancestors()
        .find(|candidate| BLUEPRINT_CRATES.get_file(candidate.join(MANIFEST)).is_some())
        .map(|candidate| candidate.to_string_lossy().replace('\\', "/"))
        .filter(|candidate| !candidate.is_empty())
}

fn path_dependencies(directory: &Path, source: &str) -> Vec<PathBuf> {
    let Ok(manifest) = toml::from_str::<Value>(source) else {
        return Vec::new();
    };
    let mut tables = Vec::new();
    for scope in ["dependencies", "build-dependencies", "dev-dependencies"] {
        tables.extend(manifest.get(scope));
        for target in manifest
            .get("target")
            .and_then(Value::as_table)
            .into_iter()
            .flat_map(|targets| targets.values())
        {
            tables.extend(target.get(scope));
        }
    }
    tables
        .into_iter()
        .filter_map(Value::as_table)
        .flat_map(|table| table.values())
        .filter_map(|dependency| dependency.get("path")?.as_str())
        .map(|path| directory.join(path))
        .collect()
}

fn application_manifests(root: &Path, directory: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            // Generated framework, blueprint and parser sources are dependencies,
            // not application manifests that select blueprint crates.
            let generated = [DESTINATION, ".forklaunch/framework-rs", "vendor"]
                .iter()
                .any(|skip| path == root.join(skip));
            if !generated && !matches!(name.as_ref(), "target" | "node_modules" | ".git" | "dist") {
                application_manifests(root, &path, found)?;
            }
        } else if name == "Cargo.toml" {
            found.push(path);
        }
    }
    Ok(())
}

/// Blueprint crates selected by application manifests on disk and by
/// manifests a caller is about to write.
fn referenced(root: &Path, pending: &[RenderedTemplate]) -> Result<BTreeSet<String>> {
    let mut manifests = Vec::new();
    application_manifests(root, root, &mut manifests)?;
    let mut sources = manifests
        .into_iter()
        .filter(|path| !pending.iter().any(|template| &template.path == path))
        .map(|path| Ok((read_to_string(&path)?, path)))
        .collect::<Result<Vec<_>>>()?;
    sources.extend(
        pending
            .iter()
            .filter(|template| template.path.file_name().is_some_and(|name| name == "Cargo.toml"))
            .map(|template| (template.content.clone(), template.path.clone())),
    );
    Ok(sources
        .iter()
        .flat_map(|(source, path)| path_dependencies(path.parent().unwrap_or(root), source))
        .filter_map(|dependency| crate_of(root, &dependency))
        .collect())
}

/// Add the blueprint crates each required crate depends on by path.
fn closure(seeds: BTreeSet<String>) -> BTreeSet<String> {
    let virtual_root = Path::new("/application");
    let mut required = BTreeSet::new();
    let mut pending: Vec<String> = seeds.into_iter().collect();
    while let Some(name) = pending.pop() {
        if !required.insert(name.clone()) {
            continue;
        }
        let Some(source) = BLUEPRINT_CRATES
            .get_file(Path::new(&name).join(MANIFEST))
            .and_then(|file| file.contents_utf8())
        else {
            continue;
        };
        let directory = virtual_root.join(DESTINATION).join("crates").join(&name);
        pending.extend(
            path_dependencies(&directory, source)
                .iter()
                .filter_map(|dependency| crate_of(virtual_root, dependency)),
        );
    }
    required
}

fn embedded_crates() -> Vec<String> {
    fn visit(dir: &Dir<'_>, found: &mut Vec<String>) {
        if dir.get_file(dir.path().join(MANIFEST)).is_some() {
            found.push(dir.path().to_string_lossy().replace('\\', "/"));
            return;
        }
        for child in dir.dirs() {
            visit(child, found);
        }
    }
    let mut found = Vec::new();
    for child in BLUEPRINT_CRATES.dirs() {
        visit(child, &mut found);
    }
    found
}

fn write_crates(root: &Path, destination: &Path, crates: &BTreeSet<String>) -> Result<()> {
    for name in crates {
        let source = BLUEPRINT_CRATES
            .get_dir(name)
            .with_context(|| format!("Embedded blueprint crate {name} is missing"))?;
        write_embedded_sources(source, &destination.join("crates"))?;
    }
    if root.join("vendor/framework/Cargo.toml").exists() {
        relocate_blueprint_framework(destination)?;
    }
    if dependencies::configured(root) {
        dependencies::rewrite_tree(root, destination)?;
    }
    Ok(())
}

/// Members are the template's crates that are present, plus any member the
/// application added itself; Cargo rejects members whose manifest is missing.
fn write_workspace(destination: &Path) -> Result<()> {
    let template: toml_edit::DocumentMut = BLUEPRINT_CARGO.parse()?;
    let path = destination.join("Cargo.toml");
    let mut workspace: toml_edit::DocumentMut = match read_to_string(&path) {
        Ok(source) => source.parse()?,
        Err(_) => BLUEPRINT_CARGO.parse()?,
    };
    let known: Vec<String> = template["workspace"]["members"]
        .as_array()
        .context("Expected packaged blueprint members")?
        .iter()
        .filter_map(|member| member.as_str().map(str::to_owned))
        .collect();
    let present = |member: &str| destination.join(member).join("Cargo.toml").is_file();
    let mut members = toml_edit::Array::new();
    let existing = workspace["workspace"]["members"]
        .as_array()
        .context("Expected embedded blueprint members")?
        .iter()
        .filter_map(|member| member.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    for member in existing.iter().chain(&known) {
        if present(member) && !members.iter().any(|value| value.as_str() == Some(member)) {
            members.push(member.as_str());
        }
    }
    workspace["workspace"]["members"] = toml_edit::value(members);
    write(path, workspace.to_string())?;
    Ok(())
}

fn exclude_from_application_workspace(root: &Path) -> Result<()> {
    let cargo_path = root.join("Cargo.toml");
    let mut cargo: toml_edit::DocumentMut = read_to_string(&cargo_path)?.parse()?;
    let excludes = cargo["workspace"]
        .as_table_mut()
        .context("Expected a Cargo workspace")?
        .entry("exclude")
        .or_insert(toml_edit::value(toml_edit::Array::new()))
        .as_array_mut()
        .context("Expected workspace.exclude array")?;
    if !excludes.iter().any(|value| value.as_str() == Some(DESTINATION)) {
        excludes.push(DESTINATION);
        write(cargo_path, cargo.to_string())?;
    }
    Ok(())
}

/// Write the blueprint crates required by the application and by `pending`
/// manifests. Applications that reference no blueprint crate get no copy.
pub(super) fn ensure(root: &Path, dryrun: bool, pending: &[RenderedTemplate]) -> Result<()> {
    if dryrun || dependencies::external_blueprint(root) {
        return Ok(());
    }
    let destination = root.join(DESTINATION);
    let required = closure(referenced(root, pending)?);
    if required.is_empty() && !destination.exists() {
        return Ok(());
    }
    create_dir_all(&destination)?;
    write_crates(root, &destination, &required)?;
    write_workspace(&destination)?;
    exclude_from_application_workspace(root)
}

fn files(directory: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    fn visit(base: &Path, directory: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "target") {
                    visit(base, &path, found)?;
                }
            } else {
                found.insert(path.strip_prefix(base)?.to_path_buf(), std::fs::read(&path)?);
            }
        }
        Ok(())
    }
    let mut found = BTreeMap::new();
    visit(directory, directory, &mut found)?;
    Ok(found)
}

/// Compare with exactly what this application would have been given,
/// including dependency-source rewrites applied after copying.
fn unmodified(root: &Path, name: &str) -> Result<bool> {
    let expected = tempfile::tempdir()?;
    write_crates(root, expected.path(), &BTreeSet::from([name.to_owned()]))?;
    let crate_path = Path::new("crates").join(name);
    Ok(files(&expected.path().join(&crate_path))? == files(&root.join(DESTINATION).join(&crate_path))?)
}

/// Remove generated crates the application no longer reaches. Edited crates
/// are kept, and so are the crates they depend on.
pub(super) fn prune(root: &Path) -> Result<()> {
    let destination = root.join(DESTINATION);
    if !destination.is_dir() {
        return Ok(());
    }
    let required = closure(referenced(root, &[])?);
    let present: Vec<String> = embedded_crates()
        .into_iter()
        .filter(|name| destination.join("crates").join(name).is_dir())
        .collect();
    let mut kept = required.clone();
    for name in present.iter().filter(|name| !required.contains(*name)) {
        if !unmodified(root, name)? {
            eprintln!(
                "Kept modified blueprint crate {DESTINATION}/crates/{name}; nothing in the application depends on it"
            );
            kept.insert(name.clone());
        }
    }
    let kept = closure(kept);
    for name in present.iter().filter(|name| !kept.contains(*name)) {
        let directory = destination.join("crates").join(name);
        remove_dir_all(&directory)?;
        let mut parent = directory.parent();
        while let Some(path) = parent.filter(|path| *path != destination) {
            if std::fs::read_dir(path)?.next().is_some() {
                break;
            }
            std::fs::remove_dir(path)?;
            parent = path.parent();
        }
    }
    write_workspace(&destination)
}

pub(super) fn relocate_blueprint_framework(directory: &Path) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            relocate_blueprint_framework(&path)?;
        } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            let source = read_to_string(&path)?;
            let updated = source.replace("/framework-rs/", "/../vendor/framework/");
            if updated != source {
                write(path, updated)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        create_dir_all(root.path().join(".forklaunch")).unwrap();
        write(root.path().join(".forklaunch/manifest.toml"), "app_name = 'app'\nmodules_path = 'src'\n").unwrap();
        write(root.path().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        root
    }

    fn worker_manifest(root: &Path) -> RenderedTemplate {
        RenderedTemplate {
            path: root.join("src/insights/Cargo.toml"),
            content: "[package]\nname = 'insights'\n[dependencies]\nworker = { package = 'forklaunch-worker-interface', path = '../../.forklaunch/blueprint-rs/crates/interfaces/worker-interface' }\n".into(),
            context: None,
        }
    }

    fn crates(root: &Path) -> BTreeSet<String> {
        embedded_crates()
            .into_iter()
            .filter(|name| root.join(DESTINATION).join("crates").join(name).is_dir())
            .collect()
    }

    #[test]
    fn only_referenced_crates_and_their_path_dependencies_are_written() {
        let root = application();
        ensure(root.path(), false, &[]).unwrap();
        assert!(!root.path().join(DESTINATION).exists(), "No blueprint reference, no blueprint copy");

        ensure(root.path(), false, &[worker_manifest(root.path())]).unwrap();
        assert_eq!(crates(root.path()), BTreeSet::from(["interfaces/worker-interface".into()]));
        let workspace: Value = toml::from_str(&read_to_string(root.path().join(DESTINATION).join("Cargo.toml")).unwrap()).unwrap();
        assert_eq!(workspace["workspace"]["members"], Value::Array(vec!["crates/interfaces/worker-interface".into()]));
        let cargo = read_to_string(root.path().join("Cargo.toml")).unwrap();
        assert!(cargo.contains(DESTINATION), "The copy stays outside the application workspace");
        for path in ["sample-app", "implementations", "client-sdk"] {
            assert!(!root.path().join(DESTINATION).join("crates").join(path).exists(), "{path}");
        }
    }

    #[test]
    fn redis_workers_receive_their_path_dependencies() {
        let root = application();
        let manifest = RenderedTemplate {
            path: root.path().join("src/jobs/Cargo.toml"),
            content: "[dependencies]\nforklaunch-worker-redis = { path = '../../.forklaunch/blueprint-rs/crates/implementations/worker/worker-redis' }\n".into(),
            context: None,
        };
        ensure(root.path(), false, &[manifest]).unwrap();
        let written = crates(root.path());
        for name in ["implementations/worker/worker-database", "interfaces/worker-interface"] {
            assert!(written.contains(name), "{name}: {written:?}");
        }
        assert!(!written.iter().any(|name| name.contains("billing") || name.contains("ecommerce") || name.contains("vendor")));
    }

    #[test]
    fn pruning_removes_only_unreferenced_unmodified_crates() {
        let root = application();
        let worker = worker_manifest(root.path());
        create_dir_all(worker.path.parent().unwrap()).unwrap();
        write(&worker.path, &worker.content).unwrap();
        let billing = RenderedTemplate {
            path: root.path().join("src/billing/Cargo.toml"),
            content: "[dependencies]\nbilling = { path = '../../.forklaunch/blueprint-rs/crates/implementations/billing/billing-base' }\ncac = { path = '../../.forklaunch/blueprint-rs/crates/implementations/cac/cac-base' }\n".into(),
            context: None,
        };
        create_dir_all(billing.path.parent().unwrap()).unwrap();
        write(&billing.path, &billing.content).unwrap();
        ensure(root.path(), false, &[]).unwrap();
        let cac = root.path().join(DESTINATION).join("crates/implementations/cac/cac-base/src/lib.rs");
        write(&cac, format!("{}\n// local change\n", read_to_string(&cac).unwrap())).unwrap();

        std::fs::remove_dir_all(billing.path.parent().unwrap()).unwrap();
        prune(root.path()).unwrap();
        let remaining = crates(root.path());
        assert!(remaining.contains("interfaces/worker-interface"));
        assert!(!remaining.contains("implementations/billing/billing-base") && !remaining.contains("interfaces/billing-interface"));
        assert!(remaining.contains("implementations/cac/cac-base"), "Edited copies are never deleted");
        assert!(remaining.contains("interfaces/cac-interface"), "Kept copies retain their path dependencies");
        assert!(!root.path().join(DESTINATION).join("crates/implementations/billing").exists());
        let workspace: Value = toml::from_str(&read_to_string(root.path().join(DESTINATION).join("Cargo.toml")).unwrap()).unwrap();
        let members = workspace["workspace"]["members"].as_array().unwrap();
        assert!(members.iter().all(|member| root.path().join(DESTINATION).join(member.as_str().unwrap()).join("Cargo.toml").is_file()));
    }
}
