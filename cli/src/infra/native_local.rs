//! In-place native Redis/S3 wiring, with private local restoration records.
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use super::{
    capabilities::{CapabilityEdit, ensure_native_dependency, merge_human_delta},
    in_place,
};
use crate::{
    constants::Infrastructure,
    core::{
        docker::{
            DockerCompose, DockerService, add_redis_to_docker_compose, add_s3_to_docker_compose,
        },
        manifest::{ProjectEntry, application::ApplicationManifestData},
    },
};

const REDIS: &str = include_str!("../templates/rust/infrastructure/redis.rs");
const S3: &str = include_str!("../templates/rust/infrastructure/s3.rs");
const STATE_DIR: &str = ".forklaunch/native-capabilities";

pub(crate) fn requested(matches: &ArgMatches) -> Option<Vec<String>> {
    matches
        .try_contains_id("infrastructure")
        .unwrap_or(false)
        .then(|| {
            matches
                .get_many::<String>("infrastructure")
                .into_iter()
                .flatten()
                .cloned()
                .collect()
        })
}

pub(crate) fn reconcile(
    matches: &ArgMatches,
    root: &Path,
    service: &str,
    desired: &[String],
    replace: bool,
) -> Result<()> {
    for wanted in [Infrastructure::Redis, Infrastructure::S3] {
        let id = wanted.to_string();
        let manifest: ApplicationManifestData = toml::from_str(&std::fs::read_to_string(
            root.join(".forklaunch/manifest.toml"),
        )?)?;
        let project = manifest
            .projects
            .iter()
            .find(|project| project.name == service)
            .context("Native infrastructure project is missing")?;
        let present = project
            .resources
            .as_ref()
            .is_some_and(|resources| match wanted {
                Infrastructure::Redis => resources.cache.as_deref() == Some(id.as_str()),
                Infrastructure::S3 => resources.object_store.as_deref() == Some(id.as_str()),
            });
        let adding = desired.contains(&id);
        if adding != present && (adding || replace) {
            change(matches, root, &manifest, project, wanted, adding)?;
        }
    }
    Ok(())
}

pub(crate) fn service_path(root: &Path, project: &ProjectEntry) -> Result<PathBuf> {
    let default = format!("crates/{}", project.name);
    let relative = Path::new(
        project
            .metadata
            .as_ref()
            .and_then(|meta| meta.crate_path.as_deref())
            .unwrap_or(&default),
    );
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("Native infrastructure crate path must remain within the application");
    }
    let path = root.join(relative);
    if !path.canonicalize()?.starts_with(root.canonicalize()?) {
        bail!("Native infrastructure crate path must remain within the application");
    }
    Ok(path)
}

#[derive(Serialize, Deserialize)]
struct Ownership {
    original_cargo: String,
    last_cargo: String,
    original_lib: String,
    last_lib: String,
    original_env: Option<String>,
    last_env: Option<String>,
    original_source: Option<String>,
    export_added: bool,
    original_service_env: BTreeMap<String, String>,
    last_service_env: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct SharedOwnership {
    original: Option<DockerService>,
    last: Option<DockerService>,
}

fn state_path(root: &Path, service: &str, id: &str) -> PathBuf {
    root.join(STATE_DIR)
        .join(format!("{service}.infra-{id}.json"))
}

pub(crate) fn change(
    matches: &ArgMatches,
    root: &Path,
    manifest: &ApplicationManifestData,
    project: &ProjectEntry,
    wanted: Infrastructure,
    adding: bool,
) -> Result<()> {
    let id = wanted.to_string();
    let (resource, backend, feature, generated) = match wanted {
        Infrastructure::Redis => ("cache", "redis", "redis-client", REDIS),
        Infrastructure::S3 => ("object_store", "minio", "aws-sdk", S3),
    };
    let dependency = format!("forklaunch-infrastructure-{id}");
    let path = service_path(root, project)?;
    let mut edit = CapabilityEdit::new(
        root,
        &manifest.app_name,
        &project.name,
        &path,
        vec![],
        vec![],
    );
    edit.native = true;
    let cargo_path = path.join("Cargo.toml");
    let lib_path = path.join("src/lib.rs");
    let source_path = path.join(format!("src/infrastructure/{id}.rs"));
    let env_path = path.join(".env.local");
    let record_path = state_path(root, &project.name, &id);
    let export = format!("#[path = \"infrastructure/{id}.rs\"]\npub mod {id}_infrastructure;\n");
    let cargo_text = edit
        .read(&cargo_path)?
        .context("Native service Cargo.toml is missing")?;
    let mut cargo: DocumentMut = cargo_text.parse()?;
    let mut lib = edit
        .read(&lib_path)?
        .context("Native service src/lib.rs is missing")?;
    let source = edit.read(&source_path)?;
    let current_env = edit.read(&env_path)?;
    let compose_path = root.join("docker-compose.yaml");
    let compose_text = edit.read(&compose_path)?;
    let mut compose = compose_text
        .as_deref()
        .map(serde_yml::from_str::<DockerCompose>)
        .transpose()?;
    let service_env = compose
        .as_ref()
        .and_then(|compose| compose.services.get(&project.name))
        .and_then(|service| service.environment.clone())
        .unwrap_or_default();
    let recovered = !adding && edit.read(&record_path)?.is_none();
    let mut ownership = edit
        .read(&record_path)?
        .map(|text| serde_json::from_str::<Ownership>(&text))
        .transpose()?
        .unwrap_or_else(|| Ownership {
            original_cargo: cargo_text.clone(),
            last_cargo: cargo_text.clone(),
            original_lib: lib.clone(),
            last_lib: lib.clone(),
            original_env: current_env.clone(),
            last_env: current_env.clone(),
            original_source: if recovered && source.as_deref() == Some(generated) {
                None
            } else {
                source.clone()
            },
            export_added: recovered || !lib.contains(&export),
            original_service_env: service_env.clone().into_iter().collect(),
            last_service_env: BTreeMap::new(),
        });
    let bucket = format!("{}-{}-dev", manifest.app_name, project.name);
    let env: Vec<(&str, &str)> = match wanted {
        Infrastructure::Redis => vec![("REDIS_URL", "redis://localhost:6379")],
        Infrastructure::S3 => vec![
            ("S3_URL", "http://localhost:9000"),
            ("S3_BUCKET", &bucket),
            ("S3_REGION", "us-east-1"),
            ("S3_ACCESS_KEY_ID", "minioadmin"),
            ("S3_SECRET_ACCESS_KEY", "minioadmin"),
        ],
    };
    if adding {
        let target = root
            .join(".forklaunch/framework-rs/crates")
            .join(&dependency);
        let relative = crate::core::relative_path::get_relative_path(&path, &target);
        ensure_native_dependency(
            &mut cargo,
            &dependency,
            &relative.to_string_lossy(),
            Some(feature),
        )?;
        ensure_native_dependency(
            &mut cargo,
            &dependency,
            &relative.to_string_lossy(),
            Some("encryption"),
        )?;
        if source.is_none() {
            edit.write(source_path.clone(), generated.into());
        }
        if ownership.export_added {
            if let Some(index) = lib.find("pub mod config;\n") {
                lib.insert_str(index + "pub mod config;\n".len(), &export);
            } else {
                lib.push_str(if lib.ends_with('\n') { "" } else { "\n" });
                lib.push_str(&export);
            }
        }
        edit.ensure_env_local(&env)?;
    } else {
        let retained_source = source.is_some() && source.as_deref() != Some(generated);
        if !retained_source && ownership.original_source.is_none() {
            edit.delete(source_path);
            if ownership.export_added {
                lib = if lib == ownership.last_lib {
                    ownership.original_lib.clone()
                } else {
                    lib.replace(&export, "")
                };
            }
        }
        // An edited factory can still reference the adapter; leave its dependency available.
        if !retained_source && !recovered {
            restore_dependency(&mut cargo, &dependency, &ownership)?;
        }
        if !recovered {
            let keys: Vec<_> = env.iter().map(|(key, _)| *key).collect();
            let restored = restore_env(
                current_env.as_deref(),
                ownership.original_env.as_deref(),
                ownership.last_env.as_deref(),
                &keys,
            );
            match restored {
                Some(text) => edit.write(env_path.clone(), text),
                None => edit.delete(env_path.clone()),
            }
        }
    }
    if let Some(compose) = &mut compose {
        let shared_path = root.join(STATE_DIR).join(format!(".infra-{id}.json"));
        let current = compose.services.get(backend).cloned();
        let mut shared = edit
            .read(&shared_path)?
            .map(|text| serde_json::from_str::<SharedOwnership>(&text))
            .transpose()?;
        if let Some(shared) = shared.as_mut() {
            if serde_json::to_value(&current)? != serde_json::to_value(&shared.last)? {
                if shared.original.is_none() {
                    shared.original = current.clone();
                } else {
                    let mut baseline = serde_json::to_value(&shared.original)?;
                    merge_human_delta(
                        &mut baseline,
                        &serde_json::to_value(&shared.last)?,
                        &serde_json::to_value(&current)?,
                    );
                    shared.original = serde_json::from_value(baseline)?;
                }
            }
        } else if adding {
            shared = Some(SharedOwnership {
                original: current,
                last: None,
            });
        }
        let mut environment = service_env;
        if adding {
            match wanted {
                Infrastructure::Redis => {
                    add_redis_to_docker_compose(&manifest.app_name, compose, &mut environment, 0)?;
                }
                Infrastructure::S3 => {
                    add_s3_to_docker_compose(
                        &manifest.app_name,
                        &project.name,
                        compose,
                        &mut environment,
                    )?;
                }
            }
            // Existing deployment settings are application-owned.
            for (key, value) in &ownership.original_service_env {
                environment.insert(key.clone(), value.clone());
            }
            ownership.last_service_env = environment.clone().into_iter().collect();
        } else if !recovered {
            for (key, _) in &env {
                if environment.get(*key) == ownership.last_service_env.get(*key) {
                    if let Some(value) = ownership.original_service_env.get(*key) {
                        environment.insert((*key).into(), value.clone());
                    } else {
                        environment.shift_remove(*key);
                    }
                }
            }
        }
        if let Some(service) = compose.services.get_mut(&project.name) {
            service.environment = Some(environment);
        }
        let remaining = manifest.projects.iter().any(|other| {
            other.name != project.name
                && other
                    .resources
                    .as_ref()
                    .is_some_and(|resources| match wanted {
                        Infrastructure::Redis => resources.cache.as_deref() == Some("redis"),
                        Infrastructure::S3 => resources.object_store.as_deref() == Some("s3"),
                    })
        });
        if !adding && !remaining {
            if let Some(shared) = shared {
                if let Some(original) = shared.original {
                    compose.services.insert(backend.into(), original);
                } else if !crate::core::docker::service_still_referenced(compose, backend) {
                    compose.services.shift_remove(backend);
                }
                edit.delete(shared_path);
            }
        } else if let Some(mut shared) = shared {
            shared.last = compose.services.get(backend).cloned();
            edit.write(shared_path, serde_json::to_string_pretty(&shared)? + "\n");
        }
        let header: String = compose_text
            .as_deref()
            .unwrap_or_default()
            .split_inclusive('\n')
            .take_while(|line| line.starts_with('#') || line.trim().is_empty())
            .collect();
        edit.write(
            compose_path,
            format!("{header}{}", serde_yml::to_string(compose)?),
        );
    }
    let updated_cargo = cargo.to_string();
    if updated_cargo != cargo_text {
        edit.write(cargo_path, updated_cargo.clone());
    }
    ownership.last_lib = lib.clone();
    edit.write(lib_path, lib);
    let manifest_path = root.join(".forklaunch/manifest.toml");
    let manifest_text = edit.read(&manifest_path)?.context("Manifest is missing")?;
    edit.write(
        manifest_path,
        in_place::set_manifest_resource(
            &manifest_text,
            &project.name,
            resource,
            adding.then_some(id.as_str()),
        )?,
    );
    if adding {
        ownership.last_cargo = updated_cargo;
        ownership.last_env = edit.read(&env_path)?;
        edit.write(
            record_path,
            serde_json::to_string_pretty(&ownership)? + "\n",
        );
        let ignore = root.join(STATE_DIR).join(".gitignore");
        let mut text = edit.read(&ignore)?.unwrap_or_default();
        if !text.lines().any(|line| line == "/*.json") {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("/*.json\n");
            edit.write(ignore, text);
        }
    } else {
        edit.delete(record_path);
    }
    if matches.get_flag("dryrun") {
        for (path, writes) in edit.changed_paths() {
            println!(
                "{} {}",
                if writes { "write " } else { "delete" },
                path.strip_prefix(root).unwrap_or(&path).display()
            );
        }
        return Ok(());
    }
    if adding {
        crate::init::rust_project::ensure_native_infrastructure_sources(root)?;
    }
    edit.commit()?;
    println!(
        "{} {id} {} {}",
        if adding { "Added" } else { "Removed" },
        if adding { "to" } else { "from" },
        project.name
    );
    Ok(())
}

fn restore_dependency(cargo: &mut DocumentMut, name: &str, state: &Ownership) -> Result<()> {
    let current: toml::Value = toml::from_str(&cargo.to_string())?;
    let last: toml::Value = toml::from_str(&state.last_cargo)?;
    if current.get("dependencies").and_then(|deps| deps.get(name))
        == last.get("dependencies").and_then(|deps| deps.get(name))
    {
        let original: DocumentMut = state.original_cargo.parse()?;
        if let Some(item) = original.get("dependencies").and_then(|deps| deps.get(name)) {
            cargo["dependencies"][name] = item.clone();
        } else if let Some(deps) = cargo
            .get_mut("dependencies")
            .and_then(|deps| deps.as_table_like_mut())
        {
            deps.remove(name);
        }
    }
    Ok(())
}

fn restore_env(
    current: Option<&str>,
    original: Option<&str>,
    last: Option<&str>,
    keys: &[&str],
) -> Option<String> {
    if current == last {
        return original.map(str::to_owned);
    }
    let current = current?;
    let line_for = |source: &str, key: &str| {
        source
            .split_inclusive('\n')
            .find(|line| in_place::env_key(line) == Some(key))
            .map(str::to_owned)
    };
    let mut output = current.to_owned();
    for key in keys {
        let current_line = line_for(&output, key);
        let previous_line = last.and_then(|source| line_for(source, key));
        if current_line == previous_line {
            if let Some(line) = current_line {
                output = output.replacen(
                    &line,
                    &original
                        .and_then(|source| line_for(source, key))
                        .unwrap_or_default(),
                    1,
                );
            }
        }
    }
    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

pub(crate) fn rename_ownership(root: &Path, old: &str, new: &str) -> Result<()> {
    for id in ["redis", "s3"] {
        let source = state_path(root, old, id);
        if !source.exists() {
            continue;
        }
        let mut state: Ownership = serde_json::from_str(&std::fs::read_to_string(&source)?)?;
        state.original_cargo =
            crate::init::rust_project::renamed_rust_package_source(&state.original_cargo, new);
        state.last_cargo =
            crate::init::rust_project::renamed_rust_package_source(&state.last_cargo, new);
        let mut edit = CapabilityEdit::new(root, "", new, root, vec![], vec![]);
        edit.native = true;
        edit.write(
            state_path(root, new, id),
            serde_json::to_string_pretty(&state)? + "\n",
        );
        edit.delete(source);
        edit.commit()?;
    }
    Ok(())
}

pub(crate) fn delete_ownership(root: &Path, service: &str) -> Result<()> {
    let manifest: toml::Value = toml::from_str(&std::fs::read_to_string(
        root.join(".forklaunch/manifest.toml"),
    )?)?;
    let compose_path = root.join("docker-compose.yaml");
    let mut compose = if compose_path.exists() {
        Some(serde_yml::from_str::<DockerCompose>(
            &std::fs::read_to_string(&compose_path)?,
        )?)
    } else {
        None
    };
    for id in ["redis", "s3"] {
        let path = state_path(root, service, id);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let resource = if id == "redis" {
            "cache"
        } else {
            "object_store"
        };
        let backend = if id == "redis" { "redis" } else { "minio" };
        let remaining = manifest
            .get("projects")
            .and_then(toml::Value::as_array)
            .is_some_and(|projects| {
                projects.iter().any(|project| {
                    project
                        .get("resources")
                        .and_then(|resources| resources.get(resource))
                        .and_then(toml::Value::as_str)
                        == Some(id)
                })
            });
        let shared_path = root.join(STATE_DIR).join(format!(".infra-{id}.json"));
        if !remaining && shared_path.exists() {
            let mut shared: SharedOwnership =
                serde_json::from_str(&std::fs::read_to_string(&shared_path)?)?;
            if let Some(compose) = compose.as_mut() {
                let current = compose.services.get(backend).cloned();
                if serde_json::to_value(&current)? != serde_json::to_value(&shared.last)? {
                    if shared.original.is_none() {
                        shared.original = current;
                    } else {
                        let mut original = serde_json::to_value(&shared.original)?;
                        merge_human_delta(
                            &mut original,
                            &serde_json::to_value(&shared.last)?,
                            &serde_json::to_value(current)?,
                        );
                        shared.original = serde_json::from_value(original)?;
                    }
                }
                if let Some(original) = shared.original {
                    compose.services.insert(backend.into(), original);
                } else if !crate::core::docker::service_still_referenced(compose, backend) {
                    compose.services.shift_remove(backend);
                }
            }
            std::fs::remove_file(shared_path)?;
        }
    }
    if let Some(compose) = compose {
        std::fs::write(compose_path, serde_yml::to_string(&compose)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_restoration_preserves_edits_and_explicit_deletion() {
        let original = "export REDIS_URL = \"redis://original:6379\"\nUSER_SETTING=original\n";
        let last = format!("{original}S3_URL=http://minio:9000\n");
        let current = "export REDIS_URL = \"redis://edited:6379\"\nUSER_SETTING=edited\nS3_URL=http://minio:9000\nNEW_SETTING=preserved\n";
        assert_eq!(restore_env(Some(current),Some(original),Some(&last),&["REDIS_URL","S3_URL"]),Some("export REDIS_URL = \"redis://edited:6379\"\nUSER_SETTING=edited\nNEW_SETTING=preserved\n".into()));
        assert_eq!(
            restore_env(
                Some("USER_SETTING=edited\n"),
                Some(original),
                Some(&last),
                &["REDIS_URL", "S3_URL"]
            ),
            Some("USER_SETTING=edited\n".into())
        );
    }

    #[test]
    fn unchanged_environment_restores_original_bytes_or_absence() {
        let original = "USER_SETTING=quoted value";
        let last = "USER_SETTING=quoted value\nS3_URL=http://minio:9000\n";
        assert_eq!(
            restore_env(Some(last), Some(original), Some(last), &["S3_URL"]),
            Some(original.into())
        );
        assert_eq!(
            restore_env(
                Some("REDIS_URL=redis://redis:6379\n"),
                None,
                Some("REDIS_URL=redis://redis:6379\n"),
                &["REDIS_URL"]
            ),
            None
        );
    }
}
