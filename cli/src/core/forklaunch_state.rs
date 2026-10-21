//! Consolidated machine-owned state under an application's `.forklaunch/`.
//!
//! The root directory holds four kinds of files:
//! - `manifest.toml`: the application manifest, including the `[framework]`
//!   dependency selection (formerly `framework-dependencies.toml`).
//! - `generated.json`: receipts for generated files, keyed by purpose. Files that
//!   are only compared for hand edits are stored as sha256 digests.
//! - `build.json`: build plans read by the generated build scripts.
//! - `tools/`: the build scripts and the private contract compiler crate.
//! - `local-state.json`: gitignored local undo state (see `infra::native_local`).
//!
//! Older applications kept each receipt and plan in its own file. Readers here
//! still understand those names; the first write migrates them in place.
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::{Map, Value as JsonValue, json};
use sha2::{Digest, Sha256};

pub(crate) const MANIFEST: &str = ".forklaunch/manifest.toml";
pub(crate) const GENERATED: &str = ".forklaunch/generated.json";
pub(crate) const BUILD: &str = ".forklaunch/build.json";
pub(crate) const LOCAL_STATE: &str = ".forklaunch/local-state.json";
pub(crate) const PRODUCTION_BUILD: &str = ".forklaunch/tools/production-build.mjs";
pub(crate) const CONTRACT_BUILD: &str = ".forklaunch/tools/contract-build.mjs";

/// Sections of `generated.json`.
pub(crate) const RUST_IMAGE: &str = "rustImage";
pub(crate) const TYPESCRIPT_IMAGE: &str = "typescriptImage";
pub(crate) const TYPESCRIPT_CONTRACTS: &str = "typescriptContracts";
pub(crate) const TYPESCRIPT_SDK_SERVICES: &str = "typescriptSdkServices";
pub(crate) const SDK_HTTP_SOURCES: &str = "sdkHttpSources";

pub(crate) const PRODUCTION_BUILD_SOURCE: &str =
    include_str!("../templates/application/production-build.mjs");
const PRODUCTION_BUILD_LEGACY_SOURCE: &str =
    include_str!("forklaunch_state/production_build_legacy.mjs");

/// File names used before consolidation; readable during migration only.
pub(crate) mod legacy {
    pub(crate) const FRAMEWORK: &str = ".forklaunch/framework-dependencies.toml";
    pub(crate) const RUST_IMAGE: &str = ".forklaunch/language-images.json";
    pub(crate) const TYPESCRIPT_IMAGE: &str = ".forklaunch/typescript-image.json";
    pub(crate) const TYPESCRIPT_SDK: &str = ".forklaunch/native-typescript-sdk.json";
    pub(crate) const HTTP_SOURCES: &str = ".forklaunch/sdk/http-sources.json";
    pub(crate) const PRODUCTION_PLAN: &str = ".forklaunch/typescript-build.json";
    pub(crate) const PRODUCTION_SCRIPT: &str = ".forklaunch/typescript-build.mjs";
    pub(crate) const CONTRACT_PLANS: &str = ".forklaunch/typescript-build";
}

pub(crate) fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Digest of a JSON value in its canonical pretty form, independent of how the
/// containing file is formatted.
pub(crate) fn json_sha256(value: &JsonValue) -> String {
    sha256_hex(serde_json::to_string_pretty(value).unwrap_or_default())
}

/// Whether `path` exists with content whose digest is `expected`.
pub(crate) fn file_matches(path: &Path, expected: Option<&str>) -> bool {
    expected.is_some_and(|expected| {
        fs::read(path).is_ok_and(|bytes| sha256_hex(bytes) == expected)
    })
}

/// State files are regular files inside the application; a symlink could point
/// reads or writes outside it.
fn ensure_regular(path: &Path) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        anyhow::ensure!(
            !metadata.file_type().is_symlink() && metadata.is_file(),
            "{} must be a regular file",
            path.display()
        );
        anyhow::ensure!(metadata.len() <= 16 * 1024 * 1024, "{} exceeds 16 MiB", path.display());
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Option<JsonValue>> {
    ensure_regular(path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes)
                .with_context(|| format!("Failed to parse {}", path.display()))?,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_object(path: &Path) -> Result<Map<String, JsonValue>> {
    match read_json(path)? {
        Some(JsonValue::Object(map)) => Ok(map),
        Some(_) => anyhow::bail!("{} must be a JSON object", path.display()),
        None => Ok(Map::new()),
    }
}

fn write_object(path: &Path, value: &Map<String, JsonValue>) -> Result<()> {
    ensure_regular(path)?;
    if value.is_empty() {
        if path.is_file() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    let content = render_object(value)?;
    if fs::read_to_string(path).ok().as_deref() == Some(content.as_str()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))
}

fn render_object(value: &Map<String, JsonValue>) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}

// ---------------------------------------------------------------------------
// [framework] in manifest.toml

/// The selected framework dependencies: the manifest `[framework]` table, or a
/// not-yet-migrated `framework-dependencies.toml`. `None` means no selection
/// (an application still using a vendored framework snapshot).
pub(crate) fn framework(root: &Path) -> Option<toml::Value> {
    let manifest = fs::read_to_string(root.join(MANIFEST))
        .ok()
        .and_then(|source| toml::from_str::<toml::Value>(&source).ok());
    if let Some(table) = manifest.as_ref().and_then(|manifest| manifest.get("framework"))
        && table.is_table()
    {
        return Some(table.clone());
    }
    fs::read_to_string(root.join(legacy::FRAMEWORK))
        .ok()
        .and_then(|source| toml::from_str::<toml::Value>(&source).ok())
}

pub(crate) fn has_framework(root: &Path) -> bool {
    framework(root).is_some()
}

/// Record the framework selection in the manifest, keeping its other content and
/// formatting, and remove the legacy file.
pub(crate) fn write_framework(root: &Path, config: &toml::Table) -> Result<()> {
    let path = root.join(MANIFEST);
    let source = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let mut document: toml_edit::DocumentMut = source.parse()?;
    let mut table = toml_edit::Table::new();
    for key in ["version", "blueprint_version", "path"] {
        if let Some(value) = config.get(key).and_then(toml::Value::as_str) {
            table.insert(key, toml_edit::value(value));
        }
    }
    document.insert("framework", toml_edit::Item::Table(table));
    let updated = document.to_string();
    if updated != source {
        fs::write(&path, updated)?;
    }
    let legacy = root.join(legacy::FRAMEWORK);
    if legacy.is_file() {
        fs::remove_file(legacy)?;
    }
    Ok(())
}

fn migrate_framework(root: &Path) -> Result<()> {
    let legacy = root.join(legacy::FRAMEWORK);
    if !legacy.is_file() || !root.join(MANIFEST).is_file() {
        return Ok(());
    }
    let config: toml::Value = toml::from_str(&fs::read_to_string(&legacy)?)
        .with_context(|| format!("Failed to parse {}", legacy.display()))?;
    let table = config.as_table().cloned().unwrap_or_default();
    write_framework(root, &table)
}

// ---------------------------------------------------------------------------
// generated.json

fn legacy_rust_image(root: &Path) -> Result<Option<JsonValue>> {
    let Some(previous) = read_json(&root.join(legacy::RUST_IMAGE))? else {
        return Ok(None);
    };
    let mut section = previous.get("rust").cloned().unwrap_or_else(|| json!({}));
    if let Some(dockerfile) = previous.get("rustDockerfile").and_then(JsonValue::as_str) {
        section["dockerfileSha256"] = sha256_hex(dockerfile).into();
    }
    Ok(Some(section))
}

fn legacy_typescript_image(root: &Path) -> Result<Option<JsonValue>> {
    let Some(previous) = read_json(&root.join(legacy::TYPESCRIPT_IMAGE))? else {
        return Ok(None);
    };
    let mut section = Map::new();
    for key in ["dockerfilePath", "runtime", "image", "inputs", "processes"] {
        if let Some(value) = previous.get(key) {
            section.insert(key.into(), value.clone());
        }
    }
    if let Some(dockerfile) = previous.get("dockerfile").and_then(JsonValue::as_str) {
        section.insert("dockerfileSha256".into(), sha256_hex(dockerfile).into());
    }
    if let Some(ignore) = previous.get("ignore").and_then(JsonValue::as_str) {
        section.insert("ignoreSha256".into(), sha256_hex(ignore).into());
    }
    if let Some(config) = previous.get("productionConfig").and_then(JsonValue::as_str) {
        // The receipt held the exact text of the old plan file. A parseable copy
        // becomes the canonical digest; anything else can never match again.
        let digest = serde_json::from_str::<JsonValue>(config)
            .map(|value| json_sha256(&value))
            .unwrap_or_else(|_| sha256_hex(config));
        section.insert("productionSha256".into(), digest.into());
    }
    Ok(Some(JsonValue::Object(section)))
}

fn legacy_contract_plans(root: &Path) -> Result<Vec<(String, JsonValue, Option<String>)>> {
    let directory = root.join(legacy::CONTRACT_PLANS);
    let mut plans = Vec::new();
    let Ok(entries) = fs::read_dir(&directory) else {
        return Ok(plans);
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let text = fs::read_to_string(&path)?;
        let value: JsonValue = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        let receipt = fs::read_to_string(path.with_extension("sha256"))
            .ok()
            .map(|digest| digest.trim().to_owned());
        // An owned plan's receipt is the digest of its exact text; re-key it to
        // the canonical digest. A customized plan keeps its stale receipt, so
        // ownership checks keep refusing to overwrite it.
        let digest = receipt.map(|receipt| {
            if receipt == sha256_hex(&text) {
                json_sha256(&value)
            } else {
                receipt
            }
        });
        plans.push((name.to_owned(), value, digest));
    }
    plans.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(plans)
}

fn legacy_generated(root: &Path) -> Result<Map<String, JsonValue>> {
    let mut sections = Map::new();
    if let Some(section) = legacy_rust_image(root)? {
        sections.insert(RUST_IMAGE.into(), section);
    }
    if let Some(section) = legacy_typescript_image(root)? {
        sections.insert(TYPESCRIPT_IMAGE.into(), section);
    }
    let contracts: Map<String, JsonValue> = legacy_contract_plans(root)?
        .into_iter()
        .filter_map(|(name, _, digest)| Some((name, JsonValue::String(digest?))))
        .collect();
    if !contracts.is_empty() {
        sections.insert(TYPESCRIPT_CONTRACTS.into(), JsonValue::Object(contracts));
    }
    if let Some(services) = read_json(&root.join(legacy::TYPESCRIPT_SDK))? {
        sections.insert(TYPESCRIPT_SDK_SERVICES.into(), services);
    }
    if let Some(sources) = read_json(&root.join(legacy::HTTP_SOURCES))? {
        sections.insert(SDK_HTTP_SOURCES.into(), sources);
    }
    Ok(sections)
}

/// Current receipts, including any still held in legacy files. Never writes.
pub(crate) fn generated(root: &Path) -> Result<Map<String, JsonValue>> {
    let mut current = read_object(&root.join(GENERATED))?;
    for (key, value) in legacy_generated(root)? {
        current.entry(key).or_insert(value);
    }
    Ok(current)
}

pub(crate) fn generated_section(root: &Path, key: &str) -> Result<Option<JsonValue>> {
    Ok(generated(root)?.remove(key))
}

/// Migrate legacy files, then apply `update` to `generated.json`.
pub(crate) fn update_generated(
    root: &Path,
    update: impl FnOnce(&mut Map<String, JsonValue>) -> Result<()>,
) -> Result<()> {
    migrate(root)?;
    let path = root.join(GENERATED);
    let mut current = read_object(&path)?;
    update(&mut current)?;
    write_object(&path, &current)
}

pub(crate) fn set_generated_section(root: &Path, key: &str, value: Option<JsonValue>) -> Result<()> {
    update_generated(root, |sections| {
        match value {
            Some(value) => {
                sections.insert(key.into(), value);
            }
            None => {
                sections.remove(key);
            }
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// build.json

fn legacy_build(root: &Path) -> Result<Map<String, JsonValue>> {
    let mut plan = Map::new();
    if let Some(production) = read_json(&root.join(legacy::PRODUCTION_PLAN))? {
        plan.insert("production".into(), production);
    }
    let contracts: Map<String, JsonValue> = legacy_contract_plans(root)?
        .into_iter()
        .map(|(name, value, _)| (name, value))
        .collect();
    if !contracts.is_empty() {
        plan.insert("contracts".into(), JsonValue::Object(contracts));
    }
    Ok(plan)
}

/// Current build plans, including any still held in legacy files. Never writes.
pub(crate) fn build_plan(root: &Path) -> Result<Map<String, JsonValue>> {
    let mut current = read_object(&root.join(BUILD))?;
    for (key, value) in legacy_build(root)? {
        match (current.get_mut(&key), value) {
            (Some(JsonValue::Object(existing)), JsonValue::Object(legacy)) => {
                for (name, plan) in legacy {
                    existing.entry(name).or_insert(plan);
                }
            }
            (None, value) => {
                current.insert(key, value);
            }
            _ => {}
        }
    }
    Ok(current)
}

pub(crate) fn production_plan(root: &Path) -> Result<Option<JsonValue>> {
    Ok(build_plan(root)?.remove("production"))
}

pub(crate) fn contract_plan(root: &Path, name: &str) -> Result<Option<JsonValue>> {
    Ok(build_plan(root)?
        .get("contracts")
        .and_then(|contracts| contracts.get(name))
        .cloned())
}

/// Migrate legacy files, then apply `update` to `build.json`.
pub(crate) fn update_build(
    root: &Path,
    update: impl FnOnce(&mut Map<String, JsonValue>) -> Result<()>,
) -> Result<()> {
    migrate(root)?;
    let path = root.join(BUILD);
    let mut current = read_object(&path)?;
    update(&mut current)?;
    current.retain(|_, value| !value.as_object().is_some_and(Map::is_empty));
    write_object(&path, &current)
}

/// Render `build.json` and `generated.json` after `update` without writing, for
/// commands that stage every file before any is written. The result includes
/// legacy plans and receipts, so writing it completes their migration.
pub(crate) fn staged_contract_updates(
    root: &Path,
    update: impl FnOnce(&mut Map<String, JsonValue>, &mut Map<String, JsonValue>) -> Result<()>,
) -> Result<[(PathBuf, String); 2]> {
    let mut build = build_plan(root)?;
    let mut generated = generated(root)?;
    update(&mut build, &mut generated)?;
    Ok([
        (root.join(BUILD), render_object(&build)?),
        (root.join(GENERATED), render_object(&generated)?),
    ])
}

// ---------------------------------------------------------------------------
// Migration

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_dir_if_empty(path: &Path) {
    if fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none()) {
        let _ = fs::remove_dir(path);
    }
}

fn migrate_receipts(root: &Path) -> Result<()> {
    let legacy_files = [
        legacy::RUST_IMAGE,
        legacy::TYPESCRIPT_IMAGE,
        legacy::TYPESCRIPT_SDK,
        legacy::HTTP_SOURCES,
        legacy::PRODUCTION_PLAN,
    ];
    let has_plans = root.join(legacy::CONTRACT_PLANS).is_dir();
    if !has_plans && legacy_files.iter().all(|file| !root.join(file).exists()) {
        return Ok(());
    }
    let generated_path = root.join(GENERATED);
    let mut generated = read_object(&generated_path)?;
    for (key, value) in legacy_generated(root)? {
        generated.entry(key).or_insert(value);
    }
    let build_path = root.join(BUILD);
    let mut build = read_object(&build_path)?;
    for (key, value) in legacy_build(root)? {
        match (build.get_mut(&key), value) {
            (Some(JsonValue::Object(existing)), JsonValue::Object(legacy)) => {
                for (name, plan) in legacy {
                    existing.entry(name).or_insert(plan);
                }
            }
            (None, value) => {
                build.insert(key, value);
            }
            _ => {}
        }
    }
    write_object(&generated_path, &generated)?;
    write_object(&build_path, &build)?;
    for file in legacy_files {
        remove_file_if_exists(&root.join(file))?;
    }
    remove_dir_if_empty(&root.join(".forklaunch/sdk"));
    if has_plans {
        let directory = root.join(legacy::CONTRACT_PLANS);
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json" || extension == "sha256")
            {
                fs::remove_file(path)?;
            }
        }
        remove_dir_if_empty(&directory);
    }
    Ok(())
}

fn migrate_production_script(root: &Path) -> Result<()> {
    let legacy = root.join(legacy::PRODUCTION_SCRIPT);
    let Ok(source) = fs::read_to_string(&legacy) else {
        return Ok(());
    };
    let destination = root.join(PRODUCTION_BUILD);
    if !destination.exists() {
        fs::create_dir_all(destination.parent().context("Tools directory")?)?;
        if source == PRODUCTION_BUILD_LEGACY_SOURCE || source == PRODUCTION_BUILD_SOURCE {
            fs::write(&destination, PRODUCTION_BUILD_SOURCE)?;
        } else {
            // Keep the customization where the build now looks for it; the
            // generated build script refuses to replace it until reconciled.
            fs::write(&destination, &source)?;
            eprintln!(
                "Moved customized {} to {}; it now reads build.json#production instead of a config argument.",
                legacy.display(),
                destination.display()
            );
        }
    }
    fs::remove_file(legacy)?;
    Ok(())
}

/// The generated contract runner moves to build.json with the plans it reads.
fn migrate_contract_runner(root: &Path) -> Result<()> {
    let path = root.join(CONTRACT_BUILD);
    if fs::read_to_string(&path).is_ok_and(|source| {
        source == crate::sdk::typescript_build::PREVIOUS_RUNNER
            || source == crate::sdk::typescript_build::LEGACY_RUNNER
    }) {
        fs::write(path, crate::sdk::typescript_build::RUNNER)?;
    }
    Ok(())
}

/// Point an owned TypeScript image and workspace build script at the
/// consolidated files. Only text the CLI generated (by its receipt) is edited.
fn migrate_typescript_image(root: &Path) -> Result<()> {
    let manifest: toml::Value = match fs::read_to_string(root.join(MANIFEST)) {
        Ok(source) => toml::from_str(&source)?,
        Err(_) => return Ok(()),
    };
    let modules = manifest.get("modules_path").and_then(toml::Value::as_str).unwrap_or("src");
    let workspace = root.join(modules);
    let package_path = workspace.join("package.json");
    if let Ok(source) = fs::read_to_string(&package_path) {
        let mut package: JsonValue = serde_json::from_str(&source)?;
        let mut changed = false;
        let relative = |path: &str| pathdiff::diff_paths(root.join(path), &workspace).map(|path| path.display().to_string());
        if let (Some(old_helper), Some(old_plan), Some(helper)) = (
            relative(legacy::PRODUCTION_SCRIPT),
            relative(legacy::PRODUCTION_PLAN),
            relative(PRODUCTION_BUILD),
        ) {
            for (build, runtime) in [("pnpm build", "node"), ("bun run build", "bun")] {
                let legacy = format!("{build} && {runtime} {old_helper} {old_plan}");
                if package["scripts"]["build:production"].as_str() == Some(legacy.as_str()) {
                    package["scripts"]["build:production"] = json!(format!("{build} && {runtime} {helper}"));
                    changed = true;
                }
            }
        }
        if changed {
            fs::write(&package_path, format!("{}\n", serde_json::to_string_pretty(&package)?))?;
        }
    }
    let path = root.join(GENERATED);
    let mut generated = read_object(&path)?;
    // The production plan bundled the vendored universal SDK as a workspace package.
    let build_path = root.join(BUILD);
    let mut build = read_object(&build_path)?;
    if !workspace.join(".forklaunch/universal-sdk").exists()
        && let Some(production) = build.get_mut("production")
    {
        let previous = json_sha256(production);
        if let Some(packages) = production.get_mut("packages").and_then(JsonValue::as_array_mut) {
            let before = packages.len();
            packages.retain(|package| package.as_str() != Some(".forklaunch/universal-sdk"));
            if packages.len() != before {
                let next = json_sha256(production);
                if let Some(image) = generated.get_mut(TYPESCRIPT_IMAGE)
                    && image["productionSha256"].as_str() == Some(previous.as_str())
                {
                    image["productionSha256"] = next.into();
                }
                write_object(&build_path, &build)?;
                write_object(&path, &generated)?;
            }
        }
    }
    let Some(image) = generated.get_mut(TYPESCRIPT_IMAGE) else { return Ok(()) };
    let file = image["dockerfilePath"].as_str().unwrap_or("Dockerfile.typescript").to_owned();
    if !matches!(file.as_str(), "Dockerfile" | "Dockerfile.typescript") {
        return Ok(());
    }
    let docker_path = root.join(&file);
    let customized = !file_matches(&docker_path, image["dockerfileSha256"].as_str());
    let docker = fs::read_to_string(&docker_path).unwrap_or_default();
    let universal = format!("{modules}/{}/", ".forklaunch/universal-sdk");
    let retired = |input: &str| {
        input == legacy::PRODUCTION_PLAN
            || input == legacy::PRODUCTION_SCRIPT
            || (input.starts_with(&format!("{}/", legacy::CONTRACT_PLANS)) && input.ends_with(".json"))
            || (input == universal && !root.join(&universal).exists())
    };
    let copy = |input: &str| format!("COPY {}", serde_json::to_string(&[input.to_owned(), format!("/app/{input}")]).unwrap_or_default());
    let mut inputs: Vec<String> = image["inputs"]
        .as_array()
        .map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    if !inputs.iter().any(|input| retired(input)) {
        return Ok(());
    }
    if customized {
        if inputs.iter().any(|input| retired(input) && docker.lines().any(|line| line == copy(input))) {
            eprintln!(
                "warning: {} was customized, so its COPY lines were not migrated. Copy {BUILD} and {PRODUCTION_BUILD} instead of the retired .forklaunch/typescript-build* files.",
                docker_path.display()
            );
        }
        // Warn once: the receipt no longer lists the retired inputs.
        inputs.retain(|input| !retired(input));
        image["inputs"] = json!(inputs);
        return write_object(&path, &generated);
    }
    let mut lines: Vec<String> = docker.lines().map(str::to_owned).collect();
    let Some(first) = lines.iter().position(|line| inputs.iter().any(|input| retired(input) && *line == copy(input))) else {
        return Ok(());
    };
    lines.retain(|line| !inputs.iter().any(|input| retired(input) && *line == copy(input)));
    inputs.retain(|input| !retired(input));
    let mut added = Vec::new();
    for input in [BUILD, PRODUCTION_BUILD] {
        if !inputs.iter().any(|existing| existing == input) {
            inputs.push(input.to_owned());
            added.push(input);
        }
    }
    inputs.sort();
    // Keep the consolidated inputs in the sorted position the generator uses.
    let block_start = lines[..first.min(lines.len())]
        .iter()
        .rposition(|line| !line.starts_with("COPY [\".forklaunch/"))
        .map_or(0, |index| index + 1);
    let mut block_end = block_start;
    while block_end < lines.len() && lines[block_end].starts_with("COPY [\".forklaunch/") {
        block_end += 1;
    }
    let mut block: Vec<String> = lines[block_start..block_end].to_vec();
    block.extend(added.iter().map(|input| copy(input)));
    block.sort();
    lines.splice(block_start..block_end, block);
    let mut updated = lines.join("\n");
    if docker.ends_with('\n') {
        updated.push('\n');
    }
    fs::write(&docker_path, &updated)?;
    image["dockerfileSha256"] = sha256_hex(&updated).into();
    image["inputs"] = json!(inputs);
    write_object(&path, &generated)
}

/// Whether `source` is the generated production build script (current or the
/// pre-consolidation version, which a later reconcile replaces).
pub(crate) fn owned_production_script(source: &str) -> bool {
    source == PRODUCTION_BUILD_SOURCE || source == PRODUCTION_BUILD_LEGACY_SOURCE
}

/// Move every legacy `.forklaunch` file into the consolidated layout. Idempotent;
/// does nothing for an application already using it.
pub(crate) fn migrate(root: &Path) -> Result<()> {
    if !root.join(".forklaunch").is_dir() {
        return Ok(());
    }
    migrate_framework(root)?;
    migrate_receipts(root)?;
    migrate_production_script(root)?;
    migrate_contract_runner(root)?;
    crate::init::rust_project::migrate_vendored_universal_sdk(root)?;
    migrate_typescript_image(root)?;
    // Per-record undo files become one gitignored local-state.json.
    crate::infra::local_state::commit(root, Default::default())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
        fs::write(
            root.path().join(MANIFEST),
            "# keep this header\napp_name = \"demo\"\nmodules_path = \"src\"\n\n[[projects]]\nname = \"api\"\n",
        )
        .unwrap();
        root
    }

    fn put(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn framework_file_moves_into_manifest_table() {
        let root = app();
        put(
            root.path(),
            legacy::FRAMEWORK,
            "# Generated by ForkLaunch\nblueprint_version = \"0.1.0\"\nversion = \"0.1.2\"\n",
        );
        assert_eq!(
            framework(root.path()).unwrap()["version"].as_str(),
            Some("0.1.2"),
            "legacy file is readable before migration"
        );
        migrate(root.path()).unwrap();
        assert!(!root.path().join(legacy::FRAMEWORK).exists());
        let manifest = fs::read_to_string(root.path().join(MANIFEST)).unwrap();
        assert!(manifest.starts_with("# keep this header\n"));
        assert!(manifest.contains("[[projects]]"));
        let value: toml::Value = toml::from_str(&manifest).unwrap();
        assert_eq!(value["framework"]["version"].as_str(), Some("0.1.2"));
        assert_eq!(value["framework"]["blueprint_version"].as_str(), Some("0.1.0"));
        assert!(value["framework"].get("path").is_none());
        migrate(root.path()).unwrap();
        assert_eq!(fs::read_to_string(root.path().join(MANIFEST)).unwrap(), manifest);
    }

    #[test]
    fn receipts_fold_into_generated_json_as_hashes() {
        let root = app();
        put(root.path(), legacy::RUST_IMAGE, &json!({
            "rustDockerfile": "FROM rust\n",
            "rust": {"image":"demo-rust:local","inputs":["Cargo.toml"],"binaries":["api"],"processes":["api"]}
        }).to_string());
        let production = json!({"workspace":"../src","packages":["api"],"processes":[]});
        let production_text = serde_json::to_string_pretty(&production).unwrap();
        put(root.path(), legacy::TYPESCRIPT_IMAGE, &json!({
            "productionConfig": production_text, "dockerfilePath": "Dockerfile.typescript",
            "runtime": "node", "dockerfile": "FROM node\n", "ignore": "**/node_modules/\n",
            "image": "demo-typescript:local", "inputs": ["src/api/"],
            "processes": {"api": {"directory":"api","entry":"server"}}
        }).to_string());
        put(root.path(), legacy::TYPESCRIPT_SDK, "[\"notes\"]");
        put(root.path(), legacy::HTTP_SOURCES, "{\"formatVersion\":1,\"typescriptServices\":[\"iam\"]}");
        let before = generated(root.path()).unwrap();
        migrate(root.path()).unwrap();
        for file in [legacy::RUST_IMAGE, legacy::TYPESCRIPT_IMAGE, legacy::TYPESCRIPT_SDK, legacy::HTTP_SOURCES] {
            assert!(!root.path().join(file).exists(), "{file} was not removed");
        }
        assert!(!root.path().join(".forklaunch/sdk").exists());
        let after = generated(root.path()).unwrap();
        assert_eq!(before, after, "reads agree before and after migration");
        assert_eq!(after[RUST_IMAGE]["dockerfileSha256"], sha256_hex("FROM rust\n"));
        assert_eq!(after[RUST_IMAGE]["image"], "demo-rust:local");
        assert!(after[RUST_IMAGE].get("rustDockerfile").is_none());
        let image = &after[TYPESCRIPT_IMAGE];
        assert_eq!(image["dockerfileSha256"], sha256_hex("FROM node\n"));
        assert_eq!(image["ignoreSha256"], sha256_hex("**/node_modules/\n"));
        assert_eq!(image["productionSha256"], json_sha256(&production));
        assert!(image.get("dockerfile").is_none() && image.get("productionConfig").is_none());
        assert_eq!(image["processes"]["api"]["entry"], "server");
        assert_eq!(after[TYPESCRIPT_SDK_SERVICES], json!(["notes"]));
        assert_eq!(after[SDK_HTTP_SOURCES]["typescriptServices"], json!(["iam"]));
        assert!(file_matches(&{ put(root.path(), "Dockerfile.rust", "FROM rust\n"); root.path().join("Dockerfile.rust") }, after[RUST_IMAGE]["dockerfileSha256"].as_str()));
    }

    #[test]
    fn build_plans_fold_into_build_json_and_keep_ownership() {
        let root = app();
        let production = json!({"workspace":"../src","packages":[],"processes":[]});
        put(root.path(), legacy::PRODUCTION_PLAN, &serde_json::to_string_pretty(&production).unwrap());
        let owned = json!({"name":"iam","service":"src/iam"});
        let owned_text = serde_json::to_string_pretty(&owned).unwrap();
        put(root.path(), ".forklaunch/typescript-build/iam.json", &owned_text);
        put(root.path(), ".forklaunch/typescript-build/iam.sha256", &sha256_hex(&owned_text));
        let custom_text = "{\"name\":\"billing\",\"custom\":true}";
        put(root.path(), ".forklaunch/typescript-build/billing.json", custom_text);
        put(root.path(), ".forklaunch/typescript-build/billing.sha256", &sha256_hex("{\"name\":\"billing\"}"));
        migrate(root.path()).unwrap();
        assert!(!root.path().join(legacy::CONTRACT_PLANS).exists());
        assert!(!root.path().join(legacy::PRODUCTION_PLAN).exists());
        let plan = build_plan(root.path()).unwrap();
        assert_eq!(plan["production"], production);
        assert_eq!(plan["contracts"]["iam"], owned);
        assert_eq!(plan["contracts"]["billing"]["custom"], true);
        let receipts = generated(root.path()).unwrap();
        assert_eq!(receipts[TYPESCRIPT_CONTRACTS]["iam"], json_sha256(&owned));
        assert_ne!(
            receipts[TYPESCRIPT_CONTRACTS]["billing"],
            json_sha256(&plan["contracts"]["billing"]),
            "a customized plan stays unowned"
        );
    }

    #[test]
    fn production_script_moves_to_tools() {
        let root = app();
        put(root.path(), legacy::PRODUCTION_SCRIPT, PRODUCTION_BUILD_LEGACY_SOURCE);
        migrate(root.path()).unwrap();
        assert!(!root.path().join(legacy::PRODUCTION_SCRIPT).exists());
        assert_eq!(fs::read_to_string(root.path().join(PRODUCTION_BUILD)).unwrap(), PRODUCTION_BUILD_SOURCE);

        let root = app();
        put(root.path(), legacy::PRODUCTION_SCRIPT, "// custom\n");
        migrate(root.path()).unwrap();
        assert_eq!(fs::read_to_string(root.path().join(PRODUCTION_BUILD)).unwrap(), "// custom\n");
        assert!(!owned_production_script("// custom\n"));
    }

    #[test]
    fn previous_contract_runner_is_replaced_and_custom_runner_kept() {
        let root = app();
        put(root.path(), CONTRACT_BUILD, crate::sdk::typescript_build::PREVIOUS_RUNNER);
        migrate(root.path()).unwrap();
        assert_eq!(fs::read_to_string(root.path().join(CONTRACT_BUILD)).unwrap(), crate::sdk::typescript_build::RUNNER);
        put(root.path(), CONTRACT_BUILD, "// custom\n");
        migrate(root.path()).unwrap();
        assert_eq!(fs::read_to_string(root.path().join(CONTRACT_BUILD)).unwrap(), "// custom\n");
    }

    #[test]
    fn owned_typescript_image_and_build_script_follow_the_new_files() {
        let root = app();
        let copy = |input: &str| format!("COPY {}\n", serde_json::to_string(&[input.to_owned(), format!("/app/{input}")]).unwrap());
        let legacy_inputs = [".forklaunch/tools/contract-build.mjs", legacy::PRODUCTION_PLAN, legacy::PRODUCTION_SCRIPT, ".forklaunch/typescript-build/iam.json", "src/.forklaunch/universal-sdk/", "src/core/"];
        let docker = format!("FROM node AS builder\nWORKDIR /app\n{}RUN pnpm run build:production\n", legacy_inputs.iter().map(|input| copy(input)).collect::<String>());
        put(root.path(), "Dockerfile.typescript", &docker);
        let plan = json!({"workspace":"../src","packages":[".forklaunch/universal-sdk","core"]});
        put(root.path(), legacy::PRODUCTION_PLAN, &plan.to_string());
        put(root.path(), legacy::TYPESCRIPT_IMAGE, &json!({"dockerfilePath":"Dockerfile.typescript","dockerfile":docker,"inputs":legacy_inputs,"processes":{},"productionConfig":plan.to_string()}).to_string());
        put(root.path(), "src/package.json", r#"{"scripts":{"build:production":"pnpm build && node ../.forklaunch/typescript-build.mjs ../.forklaunch/typescript-build.json"}}"#);
        migrate(root.path()).unwrap();
        let updated = fs::read_to_string(root.path().join("Dockerfile.typescript")).unwrap();
        let expected = format!("FROM node AS builder\nWORKDIR /app\n{}{}{}{}RUN pnpm run build:production\n", copy(BUILD), copy(CONTRACT_BUILD), copy(PRODUCTION_BUILD), copy("src/core/"));
        assert_eq!(updated, expected);
        let image = &generated(root.path()).unwrap()[TYPESCRIPT_IMAGE];
        assert_eq!(image["dockerfileSha256"], sha256_hex(&expected));
        let production = production_plan(root.path()).unwrap().unwrap();
        assert_eq!(production["packages"], json!(["core"]), "the retired vendored package leaves the plan");
        assert_eq!(image["productionSha256"], json_sha256(&production));
        assert_eq!(image["inputs"], json!([BUILD, CONTRACT_BUILD, PRODUCTION_BUILD, "src/core/"]));
        let package: JsonValue = serde_json::from_str(&fs::read_to_string(root.path().join("src/package.json")).unwrap()).unwrap();
        assert_eq!(package["scripts"]["build:production"], "pnpm build && node ../.forklaunch/tools/production-build.mjs");

        // A hand-edited image is left alone.
        let root = app();
        put(root.path(), "Dockerfile.typescript", &format!("# mine\n{docker}"));
        put(root.path(), legacy::TYPESCRIPT_IMAGE, &json!({"dockerfile":docker,"inputs":legacy_inputs}).to_string());
        migrate(root.path()).unwrap();
        assert_eq!(fs::read_to_string(root.path().join("Dockerfile.typescript")).unwrap(), format!("# mine\n{docker}"));
    }

    #[test]
    fn updates_migrate_first_and_empty_files_are_removed() {
        let root = app();
        put(root.path(), legacy::TYPESCRIPT_SDK, "[\"notes\"]");
        set_generated_section(root.path(), RUST_IMAGE, Some(json!({"image":"x"}))).unwrap();
        assert!(!root.path().join(legacy::TYPESCRIPT_SDK).exists());
        let receipts = generated(root.path()).unwrap();
        assert_eq!(receipts[TYPESCRIPT_SDK_SERVICES], json!(["notes"]));
        set_generated_section(root.path(), RUST_IMAGE, None).unwrap();
        set_generated_section(root.path(), TYPESCRIPT_SDK_SERVICES, None).unwrap();
        assert!(!root.path().join(GENERATED).exists());
    }
}
