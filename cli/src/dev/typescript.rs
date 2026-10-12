//! Export TypeScript contracts using the service image's installed runtime and dependencies.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use super::contracts::{quote, resolved_output};
use crate::core::application_output::output_path;

fn artifact(root: &Path, project: &toml::Value, field: &str, default: String) -> Result<PathBuf> {
    let configured = project
        .get("metadata")
        .and_then(|metadata| {
            metadata.get(field).or_else(|| {
                metadata.get(if field == "openapiPath" {
                    "openapi_path"
                } else {
                    "contract_artifact_path"
                })
            })
        })
        .and_then(toml::Value::as_str)
        .unwrap_or(&default);
    let path = resolved_output(&output_path(root, Path::new(configured))?)?;
    if path.exists() && !path.is_file() {
        bail!("Development artifact must be a file");
    }
    Ok(path)
}

fn script(scratch: &Path, output: &Path, runtime: &str) -> Result<String> {
    let scratch = quote(scratch.to_str().context("Invalid scratch path")?);
    let parent = quote(
        output
            .parent()
            .context("Artifact parent missing")?
            .to_str()
            .context("Invalid artifact path")?,
    );
    let output = quote(output.to_str().context("Invalid artifact path")?);
    let export = if runtime == "bun" {
        "bun server.ts"
    } else {
        "node ./node_modules/tsx/dist/cli.mjs --tsconfig tsconfig.json server.ts"
    };
    Ok(format!(
        r#"mkdir -p {scratch} && export_dir=$(mktemp -d {scratch}/dev-ts-export-XXXXXX) && trap 'rm -rf "$export_dir"' EXIT HUP INT TERM && FORKLAUNCH_MODE=openapi FORKLAUNCH_OPENAPI_OUTPUT="$export_dir/openapi.json" {export} && test -s "$export_dir/openapi.json" && mkdir -p {parent} && (cmp -s "$export_dir/openapi.json" {output} || mv -f "$export_dir/openapi.json" {output}) && rm -f "$export_dir/openapi.json" && rmdir "$export_dir" && trap - EXIT HUP INT TERM && exec "$@""#
    ))
}

pub(super) fn enable(
    root: &Path,
    manifest: &toml::Value,
    active: &BTreeSet<String>,
    compose: &mut Value,
    native: &Value,
) -> Result<Value> {
    let root = root.canonicalize()?;
    let projects = manifest
        .get("projects")
        .and_then(toml::Value::as_array)
        .context("Projects missing")?;
    let modules = manifest
        .get("modules_path")
        .and_then(toml::Value::as_str)
        .unwrap_or(".");
    let modules = if modules == "." {
        root.clone()
    } else {
        output_path(&root, Path::new(modules))?
    };
    let runtime = manifest
        .get("runtime")
        .and_then(toml::Value::as_str)
        .unwrap_or("node");
    let mount = Path::new("/forklaunch-dev-artifacts");
    let mut owners = BTreeMap::new();
    for change in native
        .as_object()
        .context("Native changes missing")?
        .values()
    {
        let name = change["command"][0]
            .as_str()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .context("Native binary missing")?;
        let project = projects
            .iter()
            .find(|project| project.get("name").and_then(toml::Value::as_str) == Some(name))
            .context("Native project missing")?;
        for (field, default) in [
            (
                "openapiPath",
                format!(".forklaunch/openapi/{name}/openapi.json"),
            ),
            (
                "contractArtifactPath",
                format!(".forklaunch/contracts/{name}/contract.json"),
            ),
        ] {
            owners.insert(artifact(&root, project, field, default)?, name.to_owned());
        }
    }
    let mut plans = Vec::new();
    let mut changes = serde_json::Map::new();
    for project in projects {
        let Some(name) = project.get("name").and_then(toml::Value::as_str) else {
            continue;
        };
        if !active.contains(name)
            || native.get(name).is_some()
            || !matches!(
                project.get("type").and_then(toml::Value::as_str),
                Some("Service" | "Worker")
            )
            || crate::core::openapi_export::OBSERVABILITY_SERVICES.contains(&name)
        {
            continue;
        }
        if project
            .get("metadata")
            .and_then(|metadata| metadata.get("language"))
            .and_then(toml::Value::as_str)
            == Some("rust")
        {
            continue;
        }
        let directory = output_path(&root, &modules.strip_prefix(&root)?.join(name))?;
        if !directory.join("server.ts").is_file() {
            bail!("TypeScript development export requires {name}/server.ts");
        }
        let destination = artifact(
            &root,
            project,
            "openapiPath",
            format!(".forklaunch/openapi/{name}/openapi.json"),
        )?;
        if let Some(previous) = owners.insert(destination.clone(), name.to_owned()) {
            bail!("Development artifact collision between {previous} and {name}");
        }
        let mut service = compose["services"][name].clone();
        let workdir = PathBuf::from(
            service["working_dir"]
                .as_str()
                .context("TypeScript working directory missing")?,
        );
        anyhow::ensure!(
            workdir.is_absolute()
                && workdir.file_name().and_then(|part| part.to_str()) == Some(name),
            "TypeScript export requires a project working directory ending in {name}"
        );
        let workspace = workdir
            .parent()
            .context("TypeScript workspace directory missing")?;
        anyhow::ensure!(
            workspace != Path::new("/")
                && !workspace.starts_with(mount)
                && !mount.starts_with(workspace),
            "TypeScript workspace conflicts with development artifact mount"
        );
        let mut arguments = service["entrypoint"]
            .as_array()
            .filter(|values| !values.is_empty())
            .context("TypeScript development requires an explicit array entrypoint")?
            .clone();
        if let Some(command) = service.get("command").filter(|value| !value.is_null()) {
            arguments.extend(
                command
                    .as_array()
                    .context("TypeScript development command must be an array")?
                    .iter()
                    .cloned(),
            );
        }
        anyhow::ensure!(
            arguments.iter().all(Value::is_string),
            "TypeScript launch arguments must be strings"
        );
        let volumes = service["volumes"]
            .as_array_mut()
            .context("TypeScript service volumes missing")?;
        for (source, target) in [(&root, mount), (&modules, workspace)] {
            if let Some(existing) = volumes
                .iter()
                .find(|volume| volume["target"].as_str() == target.to_str())
            {
                anyhow::ensure!(
                    existing["type"] == "bind"
                        && existing["source"].as_str().map(Path::new) == Some(source.as_path()),
                    "TypeScript development mount conflicts with existing volume"
                );
            } else {
                volumes.push(json!({"type":"bind","source":source,"target":target}));
            }
        }
        // Keep dependencies and generated runtime code private to each container.
        // Multiple services can rebuild the same source workspace concurrently.
        let mut private_paths = vec![workspace.join("node_modules")];
        for entry in fs::read_dir(&modules)? {
            let entry = entry?;
            if entry.path().join("package.json").is_file() {
                for directory in ["node_modules", "lib", "dist"] {
                    private_paths.push(workspace.join(entry.file_name()).join(directory));
                }
            }
        }
        for target in private_paths {
            if !volumes
                .iter()
                .any(|volume| volume["target"].as_str() == target.to_str())
            {
                volumes.push(json!({"type":"volume","target":target}));
            }
        }
        let output = mount.join(destination.strip_prefix(&root)?);
        let scratch = resolved_output(&output_path(&root, Path::new(".forklaunch"))?)?;
        let build = if runtime == "bun" {
            "bun run tsc -b --force"
        } else {
            "pnpm exec tsc -b --force"
        };
        let startup = format!(
            "(cd {} && {build}) && {}",
            quote(workspace.to_str().context("Invalid workspace path")?),
            script(&mount.join(scratch.strip_prefix(&root)?), &output, runtime)?
        )
        .replace('$', "$$");
        let mut entrypoint = vec![json!("sh"), json!("-c"), json!(startup), json!("--")];
        entrypoint.extend(arguments);
        service["entrypoint"] = json!(entrypoint);
        service["command"] = json!([]);
        // Workspace binding makes shared source changes visible to exporters and servers.
        let develop = service
            .as_object_mut()
            .context("TypeScript service must be an object")?
            .entry("develop")
            .or_insert(json!({}));
        let watches = develop
            .as_object_mut()
            .context("TypeScript development settings must be an object")?
            .entry("watch")
            .or_insert(json!([]))
            .as_array_mut()
            .context("TypeScript development watches must be an array")?;
        watches.push(json!({"action":"restart","path":modules,"ignore":["node_modules/","**/node_modules/","dist/","**/dist/","lib/","**/lib/","target/","**/target/",".git/","**/.git/",".forklaunch/","sdks/","**/*.tsbuildinfo"]}));
        changes.insert(name.to_owned(), json!({"entrypoint":service["entrypoint"],"command":service["command"],"develop":service["develop"],"volumes":service["volumes"]}));
        plans.push((name.to_owned(), service));
    }
    for (name, service) in plans {
        compose["services"][name] = service;
    }
    Ok(Value::Object(changes))
}

#[cfg(test)]
mod tests {

    use super::*;
    #[cfg(unix)]
    #[test]
    fn failed_typescript_exports_preserve_artifacts_and_do_not_start_service() {
        use std::{os::unix::fs::PermissionsExt, process::Command};
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("bun");
        fs::write(&runtime, "#!/bin/sh\nprintf '%s' \"$DOCUMENT\" > \"$FORKLAUNCH_OPENAPI_OUTPUT\"\nexit \"$EXPORT_STATUS\"\n").unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
        let output = root.path().join("quoted ' artifacts/openapi.json");
        let scratch = root.path().join("scratch");
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(&output, "original").unwrap();
        let modified = fs::metadata(&output).unwrap().modified().unwrap();
        let startup = script(&scratch, &output, "bun").unwrap();
        let run = |document: &str, status: &str| {
            Command::new("sh")
                .args(["-c", &startup, "--", "printf", "started"])
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        root.path().display(),
                        std::env::var("PATH").unwrap()
                    ),
                )
                .env("DOCUMENT", document)
                .env("EXPORT_STATUS", status)
                .output()
                .unwrap()
        };
        for (document, status) in [("partial", "1"), ("", "0")] {
            let result = run(document, status);
            assert!(!result.status.success());
            assert!(result.stdout.is_empty());
            assert_eq!(fs::read_to_string(&output).unwrap(), "original");
            assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
        }
        assert!(run("original", "0").status.success());
        assert_eq!(fs::metadata(&output).unwrap().modified().unwrap(), modified);
        let result = run("updated", "0");
        assert!(result.status.success());
        assert_eq!(result.stdout, b"started");
        assert_eq!(fs::read_to_string(&output).unwrap(), "updated");
        assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    }

    #[test]
    fn plans_contained_mixed_exports_without_writing_and_rejects_collisions() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("modules/api")).unwrap();
        fs::write(root.path().join("modules/api/server.ts"), "").unwrap();
        fs::write(root.path().join("modules/api/package.json"), "{}").unwrap();
        let mut manifest: toml::Value = toml::from_str("modules_path='modules'\nruntime='bun'\n[[projects]]\nname='api'\ntype='Service'\n[projects.metadata]\nopenapiPath='custom/api.json'\n").unwrap();
        let base = json!({"services":{"api":{"working_dir":"/test/api","entrypoint":["bun","run","dev"],"volumes":[]}}});
        let mut planned = base.clone();
        let changes = enable(
            root.path(),
            &manifest,
            &BTreeSet::from(["api".into()]),
            &mut planned,
            &json!({}),
        )
        .unwrap();
        assert!(
            changes["api"]["entrypoint"][2]
                .as_str()
                .unwrap()
                .contains("bun server.ts")
        );
        assert!(
            changes["api"]["entrypoint"][2]
                .as_str()
                .unwrap()
                .contains("$$export_dir")
        );
        assert_eq!(changes["api"]["entrypoint"][4], "bun");
        let volumes = changes["api"]["volumes"].as_array().unwrap();
        for target in ["/test/node_modules", "/test/api/lib", "/test/api/dist"] {
            assert!(
                volumes
                    .iter()
                    .any(|volume| volume["type"] == "volume" && volume["target"] == target)
            );
        }

        assert!(!root.path().join("custom").exists());
        let mut collision = manifest.clone();
        collision["projects"].as_array_mut().unwrap().push(toml::from_str("name='native-api'\ntype='Service'\n[metadata]\nlanguage='rust'\nopenapiPath='custom/api.json'\n").unwrap());
        let mut untouched = base.clone();
        assert!(
            enable(
                root.path(),
                &collision,
                &BTreeSet::from(["api".into()]),
                &mut untouched,
                &json!({"native-api":{"command":["/app/target/debug/native-api"]}})
            )
            .is_err()
        );
        assert_eq!(untouched, base);
        manifest["projects"][0]["metadata"]["openapiPath"] =
            toml::Value::String("../outside".into());
        let mut planned = base.clone();
        assert!(
            enable(
                root.path(),
                &manifest,
                &BTreeSet::from(["api".into()]),
                &mut planned,
                &json!({})
            )
            .is_err()
        );
        assert_eq!(planned, base);
    }
}
