//! Keep registered OpenAPI artifacts current while retaining the requested export layout.
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::core::application_output::output_path;

pub(super) struct Outputs(BTreeMap<String, (PathBuf, PathBuf)>);

fn identity(path: &Path) -> Result<PathBuf> {
    let mut existing = path;
    let mut suffix = Vec::new();
    while !existing.exists() {
        if fs::symlink_metadata(existing).is_ok() {
            bail!("OpenAPI output contains a dangling symlink");
        }
        suffix.push(existing.file_name().context("Invalid OpenAPI output")?);
        existing = existing.parent().context("OpenAPI output parent missing")?;
    }
    let mut resolved = existing.canonicalize()?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

impl Outputs {
    pub(super) fn plan<'a>(
        root: &Path,
        directory: &Path,
        projects: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    ) -> Result<Self> {
        let canonical_root = root.canonicalize()?;
        let mut outputs = BTreeMap::new();
        let mut owners = BTreeMap::new();
        for (name, configured) in projects {
            let mut components = Path::new(name).components();
            if !matches!(components.next(), Some(Component::Normal(_)))
                || components.next().is_some()
                || name.contains('\\')
            {
                bail!("OpenAPI service name must be one path component");
            }
            let exported = directory.join(name).join("openapi.json");
            let registered = if let Some(configured) = configured {
                let path = Path::new(configured);
                let relative = if path.is_absolute() {
                    path.strip_prefix(&canonical_root)
                        .or_else(|_| path.strip_prefix(root))
                        .context("OpenAPI artifact must stay inside the application")?
                } else {
                    path
                };
                output_path(root, relative)?
            } else {
                exported.clone()
            };
            for path in [&exported, &registered] {
                if path.exists() && !path.is_file() {
                    bail!("OpenAPI output must be a file: {}", path.display());
                }
                let resolved = identity(path)?;
                if let Some(previous) = owners.insert(resolved, name.to_owned())
                    && previous != name
                {
                    bail!("OpenAPI output collision between {previous} and {name}");
                }
            }
            outputs.insert(name.to_owned(), (exported, registered));
        }
        Ok(Self(outputs))
    }

    pub(super) fn refresh(&self, name: &str) -> Result<()> {
        let (exported, registered) = self.0.get(name).context("Missing OpenAPI output plan")?;
        if identity(exported)? == identity(registered)? {
            return Ok(());
        }
        let parent = registered
            .parent()
            .context("OpenAPI artifact parent missing")?;
        fs::create_dir_all(parent)?;
        // Publish a complete document so SDK watchers cannot observe a partial copy.
        let temporary = tempfile::NamedTempFile::new_in(parent)?;
        fs::copy(exported, temporary.path())?;
        temporary
            .persist(registered)
            .with_context(|| format!("Failed to refresh {}", registered.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_artifact_refresh_preserves_requested_export_layout() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("release-openapi");
        let registered = root.path().join("schemas/api.json");
        let plan = Outputs::plan(root.path(), &directory, [("api", registered.to_str())]).unwrap();
        fs::create_dir_all(directory.join("api")).unwrap();
        fs::write(directory.join("api/openapi.json"), "fresh").unwrap();
        plan.refresh("api").unwrap();
        assert_eq!(fs::read_to_string(&registered).unwrap(), "fresh");
        assert_eq!(
            fs::read_to_string(directory.join("api/openapi.json")).unwrap(),
            "fresh"
        );
        fs::write(&registered, "stale").unwrap();
        plan.refresh("api").unwrap();
        assert_eq!(fs::read_to_string(registered).unwrap(), "fresh");
    }

    #[test]
    fn output_preflight_rejects_escape_and_cross_service_collisions() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(".forklaunch/openapi");
        for path in ["../outside.json", "new/../../outside.json", ""] {
            assert!(Outputs::plan(root.path(), &directory, [("api", Some(path))]).is_err());
        }
        assert!(
            Outputs::plan(
                root.path(),
                &directory,
                [("api", Some("same.json")), ("other", Some("same.json"))]
            )
            .is_err()
        );
        assert!(
            Outputs::plan(
                root.path(),
                &directory,
                [
                    ("api", Some(".forklaunch/openapi/other/openapi.json")),
                    ("other", None)
                ]
            )
            .is_err()
        );
        assert!(Outputs::plan(root.path(), &directory, [("../api", None)]).is_err());
        assert!(!directory.exists());
    }

    #[cfg(unix)]
    #[test]
    fn output_preflight_checks_symlinks_and_refresh_preserves_same_file_aliases() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let directory = root.path().join("exports");
        symlink(external.path(), root.path().join("outside")).unwrap();
        symlink(root.path().join("missing"), root.path().join("dangling")).unwrap();
        for path in ["outside/api.json", "dangling"] {
            assert!(Outputs::plan(root.path(), &directory, [("api", Some(path))]).is_err());
        }
        fs::create_dir_all(directory.join("api")).unwrap();
        let exported = directory.join("api/openapi.json");
        fs::write(&exported, "original").unwrap();
        symlink(&exported, root.path().join("alias.json")).unwrap();
        let plan = Outputs::plan(root.path(), &directory, [("api", Some("alias.json"))]).unwrap();
        plan.refresh("api").unwrap();
        assert_eq!(fs::read_to_string(exported).unwrap(), "original");
    }
}
