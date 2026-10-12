//! Remember imported TypeScript services so a missing export cannot silently
//! remove them from the shared SDK during an unrelated Rust lifecycle update.
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sources {
    format_version: u8,
    typescript_services: BTreeSet<String>,
}

fn destination(root: &Path) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let path = root.join(".forklaunch/sdk/http-sources.json");
    let parent = path.parent().expect("fixed SDK state path");
    let ancestor = parent
        .ancestors()
        .find(|path| path.exists())
        .context("SDK state parent does not exist")?;
    if !ancestor.canonicalize()?.starts_with(&root) {
        bail!("HTTP SDK source state must stay inside the application");
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            bail!("HTTP SDK source state must be a regular file");
        }
        Ok(metadata) if metadata.len() > 1024 * 1024 => {
            bail!("HTTP SDK source state exceeds 1 MiB");
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(path)
}

pub(super) fn read(root: &Path) -> Result<BTreeSet<String>> {
    let path = destination(root)?;
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let sources: Sources =
        serde_json::from_slice(&fs::read(path)?).context("Failed to read HTTP SDK source state")?;
    if sources.format_version != 1 {
        bail!("Unsupported HTTP SDK source state version");
    }
    Ok(sources.typescript_services)
}

pub(super) fn write(root: &Path, services: &BTreeSet<String>) -> Result<()> {
    let path = destination(root)?;
    if services.is_empty() && !path.exists() {
        return Ok(());
    }
    let parent = path.parent().expect("fixed SDK state path");
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        bail!("HTTP SDK source state must stay inside the application");
    }
    let bytes = serde_json::to_vec_pretty(&Sources {
        format_version: 1,
        typescript_services: services.clone(),
    })?;
    if bytes.len() > 1024 * 1024 {
        bail!("HTTP SDK source state exceeds 1 MiB");
    }
    match fs::read(&path) {
        Ok(existing) if existing == bytes => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(&bytes)?;
    staged
        .persist(path)
        .context("Failed to save HTTP SDK source state")?;
    Ok(())
}

pub(super) fn is_typescript(project: &toml::Value) -> bool {
    project
        .get("metadata")
        .and_then(|metadata| metadata.get("language"))
        .and_then(toml::Value::as_str)
        != Some("rust")
}

pub(super) fn projects<'a>(
    manifest: &'a toml::Value,
    selected: Option<&str>,
) -> Vec<&'a toml::Value> {
    manifest
        .get("projects")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|project| {
            matches!(
                project.get("type").and_then(toml::Value::as_str),
                Some("Service" | "Worker")
            ) && matches!(
                project
                    .get("metadata")
                    .and_then(|metadata| metadata.get("language"))
                    .and_then(toml::Value::as_str),
                None | Some("rust" | "typescript" | "ts")
            ) && project
                .get("name")
                .and_then(toml::Value::as_str)
                .is_some_and(|name| {
                    selected.is_none_or(|selected| selected == name)
                        && (!is_typescript(project)
                            || !crate::core::openapi_export::OBSERVABILITY_SERVICES.contains(&name))
                })
        })
        .collect()
}

pub(super) fn artifact(
    root: &Path,
    project: &toml::Value,
    source: &str,
) -> Result<(&'static str, PathBuf)> {
    let name = project
        .get("name")
        .and_then(toml::Value::as_str)
        .context("SDK service name is required")?;
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
        || name.contains('\\')
    {
        bail!("SDK service name must be one path component");
    }
    let (source, metadata_key, directory, filename) =
        if source == "openapi" || is_typescript(project) {
            ("openapi", "openapiPath", "openapi", "openapi.json")
        } else {
            (
                "contract",
                "contractArtifactPath",
                "contracts",
                "contract.json",
            )
        };
    let root = root.canonicalize()?;
    let path = project
        .get("metadata")
        .and_then(|metadata| metadata.get(metadata_key))
        .and_then(toml::Value::as_str)
        .map_or_else(
            || {
                root.join(".forklaunch")
                    .join(directory)
                    .join(name)
                    .join(filename)
            },
            |path| root.join(path),
        );
    let mut ancestor = path.as_path();
    while !ancestor.exists() {
        if fs::symlink_metadata(ancestor).is_ok() {
            bail!("SDK artifact contains a dangling symlink");
        }
        ancestor = ancestor
            .parent()
            .context("SDK artifact parent is missing")?;
    }
    if !ancestor.canonicalize()?.starts_with(&root) {
        bail!("SDK artifact path must stay inside the application");
    }
    Ok((source, path))
}

pub(super) fn watched_paths(
    root: &Path,
    source: &str,
    selected: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let manifest_path = root.join(".forklaunch/manifest.toml");
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(&manifest_path)?)?;
    let mut paths = vec![manifest_path];
    for project in projects(&manifest, selected) {
        paths.push(artifact(root, project, source)?.1);
    }
    Ok(paths)
}
