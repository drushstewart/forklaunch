//! Private compiler host for applications consuming framework crates from a registry.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::REGISTRY_COMPILER;

const ENTRY: &str = include_str!(
    "../../templates/rust/framework/crates/core/examples/typescript_contract_build.rs"
);
const MANIFEST: &str = r#"[package]
name = "forklaunch-contract-compiler"
version = "0.0.0"
edition = "2024"
publish = false
autobins = false
autolib = false
autoexamples = false
autotests = false
autobenches = false
build = false

[[bin]]
name = "forklaunch-contract-compiler"
path = "main.rs"

[dependencies]
anyhow = "1"
serde_json = "1"
forklaunch-core = { path = "../../../vendor/framework/crates/core", features = ["build", "typescript-source"] }
"#;

pub(super) struct Plan {
    pub(super) compiler: &'static str,
    files: BTreeMap<PathBuf, String>,
}

impl Plan {
    pub(super) fn write(self) -> Result<()> {
        for (path, source) in self.files {
            super::super::write_if_changed(path, source)?;
        }
        Ok(())
    }
}

fn output(root: &Path, relative: &Path) -> Result<PathBuf> {
    let path = crate::core::application_output::output_path(root, relative)?;
    let root = root.canonicalize()?;
    let mut current = root.clone();
    for part in path.strip_prefix(root)?.components() {
        current.push(part);
        ensure!(
            !fs::symlink_metadata(&current).is_ok_and(|entry| entry.file_type().is_symlink()),
            "Generated contract compiler paths cannot be symlinks"
        );
    }
    Ok(path)
}

fn sources(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    ensure!(
        crate::core::forklaunch_state::has_framework(root),
        "Pinned framework compiler sources or an explicit framework dependency selection are required for build hooks"
    );
    let manifest = output(root, &Path::new(REGISTRY_COMPILER).join("Cargo.toml"))?;
    let source = crate::init::rust_project::dependencies::render_for_path(&manifest, MANIFEST)?;
    let value: toml::Value = toml::from_str(&source)?;
    let core = &value["dependencies"]["forklaunch-core"];
    ensure!(
        core.get("version")
            .and_then(toml::Value::as_str)
            .is_some_and(|version| version.starts_with('='))
            || core
                .get("path")
                .and_then(toml::Value::as_str)
                .is_some_and(|path| Path::new(path).is_absolute()),
        "Contract compiler must use the application's selected framework dependency"
    );
    let entry = output(root, &Path::new(REGISTRY_COMPILER).join("main.rs"))?;
    ensure!(
        !root.join(REGISTRY_COMPILER).join("Cargo.lock").exists(),
        "Contract compiler must use the root workspace lockfile"
    );
    let rustfmt = output(root, &Path::new(REGISTRY_COMPILER).join("rustfmt.toml"))?;
    let mut files = BTreeMap::from([(manifest, source), (entry.clone(), ENTRY.to_owned())]);
    // The host mirrors framework source; application formatting must not rewrite it.
    if !rustfmt.exists() {
        files.insert(rustfmt, super::GENERATED_RUSTFMT.to_owned());
    }
    for (path, expected) in &mut files {
        if path.exists() {
            let existing = fs::read_to_string(path)?;
            // `cargo fmt --all` formats this workspace member too. Accept only
            // the identical parsed Rust program, preserving its existing bytes;
            // changed code, attributes and literals still fail ownership checks.
            let same_program = path == &entry
                && existing.len() <= ENTRY.len() * 2 + 4096
                && syn::parse_file(&existing)
                    .ok()
                    .zip(syn::parse_file(ENTRY).ok())
                    .is_some_and(|(actual, generated)| {
                        prettyplease::unparse(&actual) == prettyplease::unparse(&generated)
                    });
            ensure!(
                existing == *expected || same_program,
                "Customized contract compiler {} must be reconciled before installing hooks",
                path.display()
            );
            if same_program {
                *expected = existing;
            }
        }
    }
    Ok(files)
}

/// Check compiler ownership before service SDK preparation mutates shared files.
pub(super) fn preflight(root: &Path) -> Result<()> {
    if snapshot(root).is_none()
        && (crate::core::forklaunch_state::has_framework(root)
            || root.join(REGISTRY_COMPILER).exists())
    {
        sources(root)?;
    }
    Ok(())
}

fn snapshot(root: &Path) -> Option<&'static str> {
    ["vendor/framework", ".forklaunch/framework-rs"]
        .into_iter()
        .find(|path| root.join(path).join("Cargo.toml").is_file())
}

fn workspace(root: &Path) -> Result<(PathBuf, toml_edit::DocumentMut)> {
    let path = output(root, Path::new("Cargo.toml"))?;
    let document: toml_edit::DocumentMut = fs::read_to_string(&path)
        .context("Contract compiler requires the generated root Cargo workspace")?
        .parse()?;
    ensure!(
        document.get("workspace").is_some(),
        "Contract compiler requires a root Cargo workspace"
    );
    if let Some(excludes) = document["workspace"]
        .get("exclude")
        .and_then(toml_edit::Item::as_array)
    {
        for pattern in excludes.iter().filter_map(toml_edit::Value::as_str) {
            ensure!(
                !glob::Pattern::new(pattern)?.matches(REGISTRY_COMPILER),
                "Contract compiler is excluded by the application's Cargo workspace"
            );
        }
    }
    Ok((path, document))
}

/// Stage all changes before writing; existing generated source must match exactly.
pub(super) fn plan(root: &Path) -> Result<Plan> {
    if let Some(compiler) = snapshot(root) {
        ensure!(
            root.join(compiler).join("Cargo.lock").is_file(),
            "Pinned framework compiler sources are required for build hooks"
        );
        return Ok(Plan {
            compiler,
            files: BTreeMap::new(),
        });
    }
    let mut files = sources(root)?;
    let (path, mut document) = workspace(root)?;
    let members = document["workspace"]["members"]
        .as_array_mut()
        .context("Contract compiler requires explicit workspace.members")?;
    let count = members
        .iter()
        .filter(|entry| entry.as_str() == Some(REGISTRY_COMPILER))
        .count();
    ensure!(
        count <= 1,
        "Duplicate contract compiler workspace membership"
    );
    if count == 0 {
        members.push(REGISTRY_COMPILER);
    }
    files.insert(path, document.to_string());
    Ok(Plan {
        compiler: REGISTRY_COMPILER,
        files,
    })
}

/// Image planning and lifecycle operations accept only the complete owned host.
pub(super) fn validate(root: &Path) -> Result<()> {
    for (path, expected) in sources(root)? {
        ensure!(
            fs::read_to_string(&path).ok().as_deref() == Some(&expected),
            "Generated contract compiler is incomplete: {}",
            path.display()
        );
    }
    let (_, document) = workspace(root)?;
    let members = document["workspace"]["members"]
        .as_array()
        .context("Contract compiler requires explicit workspace.members")?;
    ensure!(
        members
            .iter()
            .filter(|entry| entry.as_str() == Some(REGISTRY_COMPILER))
            .count()
            == 1,
        "Generated contract compiler must be a root Cargo workspace member"
    );
    Ok(())
}
