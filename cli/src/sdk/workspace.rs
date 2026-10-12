//! Keep generated SDKs in the application's JavaScript workspace.
use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use serde_json::{Value as Json, json};
use serde_yml::Value as Yaml;

use crate::core::application_output::output_path;

mod bun;

fn merge_missing(destination: &mut Json, source: &Json, field: &str) -> Result<()> {
    if let Some(entries) = source.get(field) {
        let entries = entries
            .as_object()
            .with_context(|| format!("{field} must be an object"))?;
        let destination = destination
            .as_object_mut()
            .context("package.json must be an object")?
            .entry(field)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .with_context(|| format!("Root {field} must be an object"))?;
        for (key, value) in entries {
            destination
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    Ok(())
}

fn add_packages(workspace: &mut Yaml, additions: &[String]) -> Result<()> {
    let packages = workspace
        .as_mapping_mut()
        .context("PNPM workspace must be a mapping")?
        .entry(Yaml::String("packages".into()))
        .or_insert_with(|| Yaml::Sequence(vec![]))
        .as_sequence_mut()
        .context("PNPM packages must be a list")?;
    for addition in additions {
        let value = Yaml::String(addition.clone());
        if !packages.contains(&value) {
            packages.push(value);
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum SdkWorkspace {
    Http,
    WebSocket,
}

pub(super) fn include_typescript_sdk(root: &Path) -> Result<()> {
    for (path, content) in plan_typescript_sdk(root, SdkWorkspace::Http)? {
        super::write_if_changed(output_path(root, Path::new(path))?, content)?;
    }
    Ok(())
}

/// Pure planning lets socket generation validate/check every output before writes.
pub(super) fn plan_typescript_sdk(
    root: &Path,
    kind: SdkWorkspace,
) -> Result<Vec<(&'static str, String)>> {
    let manifest_path = root.join(".forklaunch/manifest.toml");
    let manifest: toml::Value = if manifest_path.is_file() {
        toml::from_str(&fs::read_to_string(manifest_path)?)?
    } else {
        toml::Value::Table(Default::default())
    };
    let bun = manifest.get("runtime").and_then(toml::Value::as_str) == Some("bun");
    let package_path = output_path(root, Path::new("package.json"))?;
    let workspace_path = output_path(root, Path::new("pnpm-workspace.yaml"))?;
    let mut package: Json = if package_path.exists() {
        serde_json::from_str(&fs::read_to_string(&package_path)?)?
    } else {
        json!({})
    };
    let mut workspace: Yaml = if workspace_path.exists() {
        serde_yml::from_str(&fs::read_to_string(&workspace_path)?)?
    } else {
        Yaml::Mapping(Default::default())
    };
    let sdk_pattern = match kind {
        SdkWorkspace::Http => "sdks/typescript",
        SdkWorkspace::WebSocket => "sdks/typescript-websocket/*",
    };
    let mut additions = vec![sdk_pattern.to_owned()];
    let mut outputs = Vec::new();
    let mut has_modules = false;
    if let Some(relative) = manifest.get("modules_path").and_then(toml::Value::as_str) {
        let relative = Path::new(relative);
        let modules_package = output_path(root, &relative.join("package.json"))?;
        if modules_package != package_path && modules_package.is_file() {
            has_modules = true;
            if bun && let Some(policy) = self::bun::install_policy(root, relative)? {
                outputs.push(("bunfig.toml", policy));
            }
            let modules: Json = serde_json::from_str(&fs::read_to_string(&modules_package)?)?;
            for field in [
                "dependencies",
                "devDependencies",
                "overrides",
                "resolutions",
            ] {
                merge_missing(&mut package, &modules, field)?;
            }
            let prefix = relative
                .components()
                .filter_map(|component| match component {
                    std::path::Component::Normal(value) => Some(value.to_string_lossy()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/");
            additions.push(format!("{prefix}/*"));
            let nested_workspace = output_path(root, &relative.join("pnpm-workspace.yaml"))?;
            let nested: Yaml = if nested_workspace.is_file() {
                serde_yml::from_str(&fs::read_to_string(nested_workspace)?)?
            } else {
                Yaml::Mapping(Default::default())
            };
            if let Some(mapping) = nested.as_mapping() {
                for (key, value) in mapping {
                    if key.as_str() != Some("packages") {
                        workspace
                            .as_mapping_mut()
                            .context("PNPM workspace must be a mapping")?
                            .entry(key.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
            // Retain nested patterns, including exclusions, with their new base.
            let patterns = if bun {
                modules.get("workspaces").cloned()
            } else {
                nested
                    .get("packages")
                    .map(serde_json::to_value)
                    .transpose()?
            };
            if let Some(patterns) = patterns {
                for pattern in patterns
                    .as_array()
                    .context("Module workspace packages must be a list")?
                {
                    let pattern = pattern
                        .as_str()
                        .context("Workspace patterns must be strings")?;
                    let (negative, path) = pattern
                        .strip_prefix('!')
                        .map_or(("", pattern), |path| ("!", path));
                    if Path::new(path).is_absolute()
                        || Path::new(path)
                            .components()
                            .any(|part| matches!(part, std::path::Component::ParentDir))
                    {
                        bail!("Module workspace patterns must stay inside the modules directory");
                    }
                    // The modules/* entry already includes direct children.
                    // Bun executes builds twice for overlapping explicit members.
                    if negative.is_empty() && path != "." && !path.contains('/') {
                        continue;
                    }
                    additions.push(format!("{negative}{prefix}/{path}"));
                }
            }
        }
    }
    let object = package
        .as_object_mut()
        .context("Root package.json must be an object")?;
    object.entry("private").or_insert(json!(true));
    object.entry("type").or_insert(json!("module"));
    if !bun {
        object
            .entry("packageManager")
            .or_insert(json!("pnpm@10.0.0"));
    }
    if bun {
        let workspaces = object
            .entry("workspaces")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context("Bun workspaces must be a list")?;
        for addition in &additions {
            let value = json!(addition);
            if !workspaces.contains(&value) {
                workspaces.push(value);
            }
        }
    }
    let scripts = object
        .entry("scripts")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("Root scripts must be an object")?;
    let (prefix, source, filter) = match kind {
        SdkWorkspace::Http => ("sdk", "contract", "@forklaunch-generated/sdk"),
        SdkWorkspace::WebSocket => ("sdk:socket", "socket-contract", "*-websocket-sdk"),
    };
    for (name, command) in [
        (
            "generate",
            format!("forklaunch sdk generate --target typescript --from {source}"),
        ),
        (
            "check",
            format!("forklaunch sdk generate --target typescript --from {source} --check"),
        ),
        (
            "watch",
            format!("forklaunch sdk watch --target typescript --from {source}"),
        ),
        (
            "typecheck",
            if bun {
                format!("bun run --filter '{filter}' build")
            } else {
                format!("pnpm --filter '{filter}' build")
            },
        ),
    ] {
        scripts
            .entry(format!("{prefix}:{name}"))
            .or_insert(json!(command));
    }
    if root.join("Cargo.toml").is_file() {
        scripts.entry("dev").or_insert(json!("forklaunch dev"));
    }
    if has_modules {
        let build = if bun {
            "bun run --filter '*' --if-present build"
        } else {
            "pnpm -r --if-present run build"
        };
        scripts
            .entry("build")
            .or_insert(json!(if root.join("Cargo.toml").is_file() {
                format!("cargo build --workspace && {build}")
            } else {
                build.to_owned()
            }));
    }
    add_packages(&mut workspace, &additions)?;
    // Parse and validate both files before publishing either update.
    let package = serde_json::to_string_pretty(&package)? + "\n";
    let workspace = serde_yml::to_string(&workspace)?;
    outputs.push(("package.json", package));
    outputs.push(("pnpm-workspace.yaml", workspace));
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(runtime: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
        fs::create_dir_all(root.path().join("src/modules")).unwrap();
        fs::write(
            root.path().join(".forklaunch/manifest.toml"),
            format!("runtime = '{runtime}'\nmodules_path = 'src/modules'\n"),
        )
        .unwrap();
        fs::write(root.path().join("src/modules/package.json"), r#"{"devDependencies":{"typescript":"custom","tsx":"version"},"workspaces":["*","core","!excluded"]}"#).unwrap();
        fs::write(root.path().join("src/modules/pnpm-workspace.yaml"), "packages: ['*', '!excluded']\nminimumReleaseAge: 1440\nallowBuilds:\n  esbuild: true\n").unwrap();
        root
    }

    #[test]
    fn merges_inline_workspaces_and_preserves_custom_settings_idempotently() {
        let root = fixture("node");
        fs::write(root.path().join("package.json"), r#"{"devDependencies":{"typescript":"override"},"scripts":{"build":"custom build","sdk:watch":"custom watch"}}"#).unwrap();
        fs::write(
            root.path().join("pnpm-workspace.yaml"),
            "packages: ['tools/*']\nminimumReleaseAge: 2880\n",
        )
        .unwrap();
        include_typescript_sdk(root.path()).unwrap();
        let package: Json =
            serde_json::from_str(&fs::read_to_string(root.path().join("package.json")).unwrap())
                .unwrap();
        assert_eq!(package["devDependencies"]["typescript"], "override");
        assert_eq!(package["devDependencies"]["tsx"], "version");
        assert_eq!(package["scripts"]["build"], "custom build");
        assert_eq!(package["scripts"]["sdk:watch"], "custom watch");
        let first = fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap();
        let workspace: Yaml = serde_yml::from_str(&first).unwrap();
        for path in [
            "tools/*",
            "sdks/typescript",
            "src/modules/*",
            "!src/modules/excluded",
        ] {
            assert!(
                workspace["packages"]
                    .as_sequence()
                    .unwrap()
                    .contains(&Yaml::String(path.into()))
            );
        }
        assert_eq!(workspace["minimumReleaseAge"].as_u64(), Some(2880));
        assert_eq!(workspace["allowBuilds"]["esbuild"].as_bool(), Some(true));
        include_typescript_sdk(root.path()).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("pnpm-workspace.yaml")).unwrap(),
            first
        );
    }

    #[test]
    fn bun_membership_and_commands_include_modules_and_sdk() {
        let root = fixture("bun");
        fs::write(root.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        include_typescript_sdk(root.path()).unwrap();
        let package: Json =
            serde_json::from_str(&fs::read_to_string(root.path().join("package.json")).unwrap())
                .unwrap();
        assert!(
            package["workspaces"]
                .as_array()
                .unwrap()
                .contains(&json!("src/modules/*"))
        );
        assert!(
            package["workspaces"]
                .as_array()
                .unwrap()
                .contains(&json!("sdks/typescript"))
        );
        assert_eq!(
            package["scripts"]["build"],
            "cargo build --workspace && bun run --filter '*' --if-present build"
        );
        assert!(
            package["scripts"]["sdk:typecheck"]
                .as_str()
                .unwrap()
                .starts_with("bun ")
        );
        assert!(package.get("packageManager").is_none());
        assert!(
            !package["workspaces"]
                .as_array()
                .unwrap()
                .contains(&json!("src/modules/core"))
        );
    }

    #[test]
    fn socket_planning_is_read_only_and_coexists_with_http_generation() {
        let root = fixture("bun");
        let files = plan_typescript_sdk(root.path(), SdkWorkspace::WebSocket).unwrap();
        assert!(!root.path().join("package.json").exists());
        assert!(!root.path().join("pnpm-workspace.yaml").exists());
        for (path, content) in files {
            fs::write(root.path().join(path), content).unwrap();
        }
        include_typescript_sdk(root.path()).unwrap();
        let package: Json =
            serde_json::from_str(&fs::read_to_string(root.path().join("package.json")).unwrap())
                .unwrap();
        assert!(
            package["workspaces"]
                .as_array()
                .unwrap()
                .contains(&json!("sdks/typescript-websocket/*"))
        );
        assert!(
            package["workspaces"]
                .as_array()
                .unwrap()
                .contains(&json!("sdks/typescript"))
        );
        assert!(
            package["scripts"]["sdk:socket:generate"]
                .as_str()
                .unwrap()
                .ends_with("--from socket-contract")
        );
        assert!(
            package["scripts"]["sdk:generate"]
                .as_str()
                .unwrap()
                .ends_with("--from contract")
        );
        assert_eq!(
            package["scripts"]["sdk:socket:typecheck"],
            "bun run --filter '*-websocket-sdk' build"
        );
    }

    #[test]
    fn invalid_workspace_does_not_partially_rewrite_package() {
        let root = fixture("node");
        let original = "{\"private\":true}\n";
        fs::write(root.path().join("package.json"), original).unwrap();
        fs::write(
            root.path().join("pnpm-workspace.yaml"),
            "packages: invalid\n",
        )
        .unwrap();
        assert!(include_typescript_sdk(root.path()).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("package.json")).unwrap(),
            original
        );
        fs::write(
            root.path().join(".forklaunch/manifest.toml"),
            "modules_path = '../outside'\n",
        )
        .unwrap();
        assert!(include_typescript_sdk(root.path()).is_err());
    }
}
