//! In-place native Redis/S3 wiring. Removal is stateless: it takes back what
//! `infra add` would write for the service today, and keeps anything edited.
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use indexmap::IndexMap;
use toml_edit::DocumentMut;

use super::{
    capabilities::{
        CapabilityEdit, dependency_in_use, ensure_native_dependency, release_dependency,
    },
    in_place,
};
use crate::{
    constants::Infrastructure,
    core::{
        docker::{DockerCompose, add_redis_to_docker_compose, add_s3_to_docker_compose},
        manifest::{ProjectEntry, application::ApplicationManifestData},
    },
};

const REDIS: &str = include_str!("../templates/rust/infrastructure/redis.rs");
const S3: &str = include_str!("../templates/rust/infrastructure/s3.rs");

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
    edit.format_rust = adding;
    let cargo_path = path.join("Cargo.toml");
    let source_root = if path.join("lib.rs").is_file() { path.clone() } else { path.join("src") };
    let lib_path = source_root.join("lib.rs");
    let source_path = source_root.join(format!("infrastructure/{id}.rs"));
    let export = format!("#[path = \"infrastructure/{id}.rs\"]\npub mod {id}_infrastructure;\n");
    let cargo_text = edit
        .read(&cargo_path)?
        .context("Native service Cargo.toml is missing")?;
    let mut cargo: DocumentMut = cargo_text.parse()?;
    let mut lib = edit
        .read(&lib_path)?
        .context("Native service library export file is missing")?;
    let source = edit.read(&source_path)?;
    let compose_path = root.join("docker-compose.yaml");
    let compose_text = edit.read(&compose_path)?;
    let mut compose = compose_text
        .as_deref()
        .map(serde_yml::from_str::<DockerCompose>)
        .transpose()?;
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
    let add_dependency = |cargo: &mut DocumentMut| -> Result<()> {
        let target = root.join(".forklaunch/framework-rs/crates").join(&id);
        let relative = crate::core::relative_path::get_relative_path(&path, &target);
        for feature in [feature, "encryption"] {
            ensure_native_dependency(cargo, &dependency, &relative.to_string_lossy(), Some(feature))?;
        }
        Ok(())
    };
    if adding {
        add_dependency(&mut cargo)?;
        if source.is_none() {
            edit.write(source_path.clone(), generated.into());
        }
        if !lib.contains(&export) {
            insert_module_export(&mut lib, &export, &format!("{id}_infrastructure"));
        }
        edit.ensure_env_local(&env)?;
    } else {
        // An edited factory is the developer's, and can still reference the
        // adapter; it keeps its module export and dependency.
        let kept = source.as_deref().is_some_and(|text| {
            !edit.is_generated(&source_path, text, generated)
        });
        if kept {
            edit.note_kept(&source_path);
        } else {
            edit.delete(source_path.clone());
            lib = lib.replace(&export, "");
            if !dependency_in_use(&path, &cargo, &dependency, std::slice::from_ref(&source_path)) {
                release_dependency(&mut edit, &cargo_path, &mut cargo, &dependency, add_dependency)?;
            }
        }
        edit.release_env_local(&env)?;
    }
    if let Some(compose) = &mut compose {
        let add_backend = |compose: &mut DockerCompose, environment: &mut IndexMap<String, String>| match wanted {
            Infrastructure::Redis => {
                add_redis_to_docker_compose(&manifest.app_name, compose, environment, 0).map(|_| ())
            }
            Infrastructure::S3 => {
                add_s3_to_docker_compose(&manifest.app_name, &project.name, compose, environment)
                    .map(|_| ())
            }
        };
        let mut environment = compose
            .services
            .get(&project.name)
            .and_then(|service| service.environment.clone())
            .unwrap_or_default();
        let mut generated_compose = DockerCompose::default();
        let mut generated_env = IndexMap::new();
        add_backend(&mut generated_compose, &mut generated_env)?;
        if adding {
            // Values the service already sets are the application's.
            let existing = environment.clone();
            add_backend(compose, &mut environment)?;
            environment.extend(existing);
        } else {
            for (key, value) in &generated_env {
                match environment.get(key) {
                    Some(current) if current == value => {
                        environment.shift_remove(key);
                    }
                    Some(_) => edit.note_kept_setting(&compose_path, &format!("{} {key}", project.name)),
                    None => {}
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
            release_backend(&mut edit, compose, &generated_compose, backend)?;
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
    let updated_cargo = if adding {
        crate::init::rust_project::dependencies::render_for_path(&cargo_path, &cargo.to_string())?
    } else {
        cargo.to_string()
    };
    if updated_cargo != cargo_text {
        edit.write(cargo_path, updated_cargo);
    }
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

/// The shared compose backend (`redis`, `minio`) `infra add` creates, removed
/// once no project uses it: only while nothing else references it and it is
/// still the generated service.
fn release_backend(
    edit: &mut CapabilityEdit,
    compose: &mut DockerCompose,
    generated: &DockerCompose,
    backend: &str,
) -> Result<()> {
    let Some(current) = compose.services.get(backend) else {
        return Ok(());
    };
    if crate::core::docker::service_still_referenced(compose, backend) {
        return Ok(());
    }
    // `init --infrastructure` and `infra add` serialize the same backend with
    // and without empty fields (`environment: {}`); neither is a user edit.
    if without_empty(serde_json::to_value(current)?)
        == without_empty(serde_json::to_value(generated.services.get(backend))?)
    {
        compose.services.shift_remove(backend);
        // And the named volumes it brought, once no other service mounts them.
        for (name, volume) in &generated.volumes {
            let mounted = compose.services.values().any(|service| {
                service.volumes.iter().flatten().any(|mount| {
                    mount.split_once(':').map_or(mount.as_str(), |(source, _)| source) == name
                })
            });
            if !mounted
                && compose.volumes.get(name).map(serde_json::to_value).transpose()?
                    == Some(serde_json::to_value(volume)?)
            {
                compose.volumes.shift_remove(name);
            }
        }
    } else {
        let compose_path = edit.app_root.join("docker-compose.yaml");
        edit.note_kept_setting(&compose_path, &format!("service {backend}"));
    }
    Ok(())
}

/// A compose value without null, empty-object and empty-array fields.
fn without_empty(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, without_empty(value)))
                .filter(|(_, value)| match value {
                    serde_json::Value::Null => false,
                    serde_json::Value::Object(map) => !map.is_empty(),
                    serde_json::Value::Array(items) => !items.is_empty(),
                    _ => true,
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(without_empty).collect())
        }
        other => other,
    }
}

/// Insert where rustfmt's module ordering expects it, so `cargo fmt --check`
/// passes on the generated library without reformatting user code.
fn insert_module_export(lib: &mut String, export: &str, name: &str) {
    let lines: Vec<&str> = lib.split_inclusive('\n').collect();
    let module = |line: &str| {
        line.trim_end()
            .strip_prefix("pub mod ")
            .and_then(|rest| rest.strip_suffix(';'))
            .map(str::to_owned)
    };
    let declarations: Vec<(usize, String)> =
        lines.iter().enumerate().filter_map(|(index, line)| Some((index, module(line)?))).collect();
    let attributes_before = |mut index: usize| {
        while index > 0 && lines[index - 1].starts_with("#[") {
            index -= 1;
        }
        index
    };
    let at = match declarations.iter().find(|(_, existing)| existing.as_str() > name) {
        Some((index, _)) => attributes_before(*index),
        None => match declarations.last() {
            Some((index, _)) => index + 1,
            None => {
                lib.push_str(if lib.is_empty() || lib.ends_with('\n') { "" } else { "\n" });
                lib.push_str(export);
                return;
            }
        },
    };
    let offset: usize = lines[..at].iter().map(|line| line.len()).sum();
    lib.insert_str(offset, export);
}

/// After `delete service`: release the shared compose backends the deleted
/// service was the last user of (see [`release_backend`]).
pub(crate) fn release_deleted_service(root: &Path) -> Result<()> {
    let compose_path = root.join("docker-compose.yaml");
    if !compose_path.exists() {
        return Ok(());
    }
    let manifest: ApplicationManifestData =
        toml::from_str(&std::fs::read_to_string(root.join(".forklaunch/manifest.toml"))?)?;
    let text = std::fs::read_to_string(&compose_path)?;
    let mut compose: DockerCompose = serde_yml::from_str(&text)?;
    let before = serde_yml::to_string(&compose)?;
    let mut edit = CapabilityEdit::new(root, &manifest.app_name, "", root, vec![], vec![]);
    for wanted in [Infrastructure::Redis, Infrastructure::S3] {
        let id = wanted.to_string();
        let remaining = manifest.projects.iter().any(|project| {
            project.resources.as_ref().is_some_and(|resources| match wanted {
                Infrastructure::Redis => resources.cache.as_deref() == Some(id.as_str()),
                Infrastructure::S3 => resources.object_store.as_deref() == Some(id.as_str()),
            })
        });
        if remaining {
            continue;
        }
        let mut generated = DockerCompose::default();
        let mut environment = IndexMap::new();
        let backend = match wanted {
            Infrastructure::Redis => {
                add_redis_to_docker_compose(&manifest.app_name, &mut generated, &mut environment, 0)?;
                "redis"
            }
            Infrastructure::S3 => {
                add_s3_to_docker_compose(&manifest.app_name, "", &mut generated, &mut environment)?;
                "minio"
            }
        };
        release_backend(&mut edit, &mut compose, &generated, backend)?;
    }
    let after = serde_yml::to_string(&compose)?;
    if after != before {
        let header: String = text
            .split_inclusive('\n')
            .take_while(|line| line.starts_with('#') || line.trim().is_empty())
            .collect();
        std::fs::write(compose_path, format!("{header}{after}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_exports_follow_rustfmt_module_order() {
        let mut lib = "pub mod application;\npub mod config;\npub mod domain;\npub mod registrations;\npub mod server;\npub use contracts as sdk;\n".to_owned();
        for id in ["s3", "redis"] {
            let export = format!("#[path = \"infrastructure/{id}.rs\"]\npub mod {id}_infrastructure;\n");
            insert_module_export(&mut lib, &export, &format!("{id}_infrastructure"));
        }
        assert_eq!(lib, "pub mod application;\npub mod config;\npub mod domain;\n#[path = \"infrastructure/redis.rs\"]\npub mod redis_infrastructure;\npub mod registrations;\n#[path = \"infrastructure/s3.rs\"]\npub mod s3_infrastructure;\npub mod server;\npub use contracts as sdk;\n");
    }

    #[test]
    fn shared_backends_leave_only_in_their_generated_form() {
        let root = tempfile::tempdir().unwrap();
        let mut generated = DockerCompose::default();
        add_redis_to_docker_compose("app", &mut generated, &mut IndexMap::new(), 0).unwrap();
        let mut edit = CapabilityEdit::new(root.path(), "app", "alpha", root.path(), vec![], vec![]);
        let mut compose = generated.clone();
        release_backend(&mut edit, &mut compose, &generated, "redis").unwrap();
        assert!(!compose.services.contains_key("redis"));
        let mut edited = generated.clone();
        edited.services.get_mut("redis").unwrap().image = Some("human/redis".into());
        release_backend(&mut edit, &mut edited, &generated, "redis").unwrap();
        assert!(edited.services.contains_key("redis"));
        let mut referenced = generated.clone();
        let user = crate::core::docker::DockerService {
            environment: Some([("CACHE".to_owned(), "redis://redis:6379/1".to_owned())].into_iter().collect()),
            ..Default::default()
        };
        referenced.services.insert("user".into(), user);
        release_backend(&mut edit, &mut referenced, &generated, "redis").unwrap();
        assert!(referenced.services.contains_key("redis"));
        // `init --infrastructure` writes the same backend with `environment: {}`.
        let mut initialized = generated.clone();
        initialized.services.get_mut("redis").unwrap().environment = Some(Default::default());
        release_backend(&mut edit, &mut initialized, &generated, "redis").unwrap();
        assert!(!initialized.services.contains_key("redis"));
    }
}
