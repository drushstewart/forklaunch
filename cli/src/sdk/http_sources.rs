//! Remember imported TypeScript services so a missing export cannot silently
//! remove them from the shared SDK during an unrelated Rust lifecycle update.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use crate::core::forklaunch_state::{self, SDK_HTTP_SOURCES};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sources {
    format_version: u8,
    typescript_services: BTreeSet<String>,
}

/// Tracked in `generated.json#sdkHttpSources` (formerly `.forklaunch/sdk/http-sources.json`).
pub(super) fn read(root: &Path) -> Result<BTreeSet<String>> {
    let Some(value) = forklaunch_state::generated_section(root, SDK_HTTP_SOURCES)
        .context("Failed to read HTTP SDK source state")?
    else {
        return Ok(BTreeSet::new());
    };
    let sources: Sources =
        serde_json::from_value(value).context("Failed to read HTTP SDK source state")?;
    if sources.format_version != 1 {
        bail!("Unsupported HTTP SDK source state version");
    }
    Ok(sources.typescript_services)
}

pub(super) fn write(root: &Path, services: &BTreeSet<String>) -> Result<()> {
    let current = forklaunch_state::generated_section(root, SDK_HTTP_SOURCES)?;
    if services.is_empty() && current.is_none() {
        return Ok(());
    }
    let next = serde_json::to_value(Sources {
        format_version: 1,
        typescript_services: services.clone(),
    })?;
    if current.as_ref() == Some(&next) && !root.join(forklaunch_state::legacy::HTTP_SOURCES).exists() {
        return Ok(());
    }
    forklaunch_state::set_generated_section(root, SDK_HTTP_SOURCES, Some(next))
        .context("Failed to save HTTP SDK source state")
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
            crate::core::openapi_export::raw_contract_export_enabled(project)
                && matches!(
                    project.get("type").and_then(toml::Value::as_str),
                    Some("Service" | "Worker")
                )
                && matches!(
                    project
                        .get("metadata")
                        .and_then(|metadata| metadata.get("language"))
                        .and_then(toml::Value::as_str),
                    None | Some("rust" | "typescript" | "ts")
                )
                && project
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|name| {
                        selected.is_none_or(|selected| selected == name)
                            && (!is_typescript(project)
                                || !crate::core::openapi_export::OBSERVABILITY_SERVICES
                                    .contains(&name))
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
    let explicit_contract = project
        .get("metadata")
        .and_then(|metadata| metadata.get("contractArtifactPath"))
        .and_then(toml::Value::as_str)
        .is_some();
    let (source, metadata_key, directory, filename) =
        if source == "openapi" || (is_typescript(project) && !explicit_contract) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typescript_explicit_portable_contract_overrides_legacy_default() {
        let root = tempfile::tempdir().unwrap();
        let project: toml::Value = toml::from_str(
            r#"
name = "notes"
type = "Service"
[metadata]
language = "typescript"
contractArtifactPath = ".forklaunch/contracts/notes/contract.json"
"#,
        )
        .unwrap();
        let (source, path) = artifact(root.path(), &project, "contract").unwrap();
        assert_eq!(source, "contract");
        assert!(path.ends_with("contracts/notes/contract.json"));
        assert_eq!(
            artifact(root.path(), &project, "openapi").unwrap().0,
            "openapi"
        );
        let legacy: toml::Value = toml::from_str("name = 'notes'\ntype = 'Service'").unwrap();
        assert_eq!(
            artifact(root.path(), &legacy, "contract").unwrap().0,
            "openapi"
        );
    }
}
