//! Native capability edits retain application source and record dependency ownership.
use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml_edit::{Array, DocumentMut, InlineTable, value};

use super::{
    Capability, CapabilityEdit, GATEWAY_MOCK_SERVICE, ensure_gateway_mock, local_gateway_env,
    release_gateway_mock,
};
use crate::core::docker::{DependsOn, DockerCompose, DockerService};
const MODULE: &str = include_str!("../../templates/rust/capabilities/mod.rs");
const HANDLER: &str = include_str!("../../templates/rust/capabilities/handler.rs");
const NONCE_STORE: &str = include_str!("../../templates/rust/capabilities/nonce_store.rs");
const BEGIN: &str = "// BEGIN FORKLAUNCH MANAGED CAPABILITIES\n";
const END: &str = "// END FORKLAUNCH MANAGED CAPABILITIES\n";
const LIB_EXPORT: &str = "pub mod capabilities;\n";
const REGISTRY: &str = "    capabilities::register_contracts(&mut registry);\n";
const MOCK_STATE: &str = ".forklaunch/native-capabilities/.gateway-mock.json";

#[derive(Serialize, Deserialize)]
struct GatewayOwnership {
    original: Option<DockerService>,
    #[serde(default)]
    last: Option<DockerService>,
}

fn current_gateway_mock(edit: &CapabilityEdit) -> Result<Option<DockerService>> {
    Ok(edit
        .read(&edit.app_root.join("docker-compose.yaml"))?
        .map(|source| serde_yml::from_str::<DockerCompose>(&source))
        .transpose()?
        .and_then(|compose| compose.services.get(GATEWAY_MOCK_SERVICE).cloned()))
}

fn remember_gateway_edits(
    state: &mut GatewayOwnership,
    current: Option<DockerService>,
) -> Result<()> {
    let Some(last) = state.last.as_ref() else {
        return Ok(());
    };
    let previous = serde_json::to_value(last)?;
    let current = serde_json::to_value(current)?;
    if previous == current {
        return Ok(());
    }
    let mut original = serde_json::to_value(&state.original)?;
    if original.is_null() {
        // Retain a developer-adopted mock, excluding unchanged CLI callbacks.
        original = current.clone();
        if let Some(env) = original
            .get_mut("environment")
            .and_then(serde_json::Value::as_object_mut)
        {
            env.retain(|key, value| {
                !key.starts_with("MOCK_EVENTS_URL_")
                    || previous.get("environment").and_then(|env| env.get(key)) != Some(value)
            });
        }
    } else {
        merge_human_delta(&mut original, &previous, &current);
    }
    state.original = serde_json::from_value(original)?;
    Ok(())
}

pub(crate) fn merge_human_delta(
    baseline: &mut serde_json::Value,
    previous: &serde_json::Value,
    current: &serde_json::Value,
) {
    if previous == current {
        return;
    }
    if let (Some(previous), Some(current)) = (previous.as_object(), current.as_object()) {
        if !baseline.is_object() {
            *baseline = serde_json::json!({});
        }
        let baseline = baseline.as_object_mut().expect("object baseline");
        for key in previous
            .keys()
            .chain(current.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            if let Some(value) = current.get(key) {
                if previous.get(key) != Some(value) {
                    merge_human_delta(
                        baseline
                            .entry(key.clone())
                            .or_insert(serde_json::Value::Null),
                        previous.get(key).unwrap_or(&serde_json::Value::Null),
                        value,
                    );
                }
            } else {
                baseline.remove(key);
            }
        }
    } else {
        *baseline = current.clone();
    }
}

fn capability_owners(
    manifest: &toml::Value,
    updated: Option<(&str, &[String])>,
) -> BTreeMap<String, String> {
    let mut owners = BTreeMap::new();
    if let Some(projects) = manifest.get("projects").and_then(toml::Value::as_array) {
        for project in projects {
            let Some(service) = project.get("name").and_then(toml::Value::as_str) else {
                continue;
            };
            let features = if let Some((_, features)) = updated.filter(|(name, _)| *name == service)
            {
                features.to_vec()
            } else {
                project
                    .get("resources")
                    .and_then(|resources| resources.get("capabilities"))
                    .and_then(toml::Value::as_array)
                    .map(|features| {
                        features
                            .iter()
                            .filter_map(toml::Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            for feature in features {
                owners.entry(feature).or_insert_with(|| service.into());
            }
        }
    }
    owners
}

fn retain_callback_target(
    compose: &mut DockerCompose,
    removed: &str,
    feature: &str,
    previous: Option<String>,
    owners: &BTreeMap<String, String>,
) {
    let Some(service) = owners.get(feature) else {
        return;
    };
    let port = compose
        .services
        .get(service)
        .and_then(|service| service.environment.as_ref())
        .and_then(|env| env.get("SERVICE_PORT").or_else(|| env.get("PORT")))
        .cloned()
        .unwrap_or_else(|| "8000".into());
    let target = previous
        .filter(|url| retarget_service_url(url, removed, service).is_none())
        .unwrap_or_else(|| format!("http://{service}:{port}"));
    if let Some(mock) = compose.services.get_mut(GATEWAY_MOCK_SERVICE) {
        mock.environment
            .get_or_insert_with(Default::default)
            .insert(
                format!("MOCK_EVENTS_URL_{}", feature.to_uppercase()),
                target,
            );
    }
}

pub(crate) fn rename_native_ownership(root: &Path, old: &str, new: &str) -> Result<()> {
    let mut edit = CapabilityEdit::new(root, "", new, root, vec![], vec![]);
    edit.native = true;
    if let Some(text) = edit.read(&root.join(MOCK_STATE))? {
        let mut state: GatewayOwnership = serde_json::from_str(&text)?;
        if let Some(mock) = state.original.as_mut() {
            rename_callback_targets(mock, old, new);
        }
        if let Some(mock) = state.last.as_mut() {
            rename_callback_targets(mock, old, new);
        }
        edit.write(
            root.join(MOCK_STATE),
            serde_json::to_string_pretty(&state)? + "\n",
        );
    }
    let previous = root.join(format!(".forklaunch/native-capabilities/{old}.json"));
    if let Some(text) = edit.read(&previous)? {
        let mut ownership: Ownership = serde_json::from_str(&text)?;
        for source in [&mut ownership.original_cargo, &mut ownership.last_cargo] {
            *source = crate::init::rust_project::renamed_rust_package_source(source, new);
        }
        if let Some(mock) = ownership.original_mock.as_mut() {
            rename_callback_targets(mock, old, new);
        }
        edit.write(
            root.join(format!(".forklaunch/native-capabilities/{new}.json")),
            serde_json::to_string_pretty(&ownership)? + "\n",
        );
        edit.delete(previous);
    }
    if root.join("docker-compose.yaml").exists() {
        edit.update_compose(|compose| {
            if let Some(mock) = compose.services.get_mut(GATEWAY_MOCK_SERVICE) {
                rename_callback_targets(mock, old, new);
            }
            Ok(())
        })?;
    }
    edit.commit()
}

fn rename_callback_targets(mock: &mut DockerService, old: &str, new: &str) {
    if let Some(env) = mock.environment.as_mut() {
        for (key, value) in env {
            if !key.starts_with("MOCK_EVENTS_URL_") {
                continue;
            }
            if let Some(target) = retarget_service_url(value, old, new) {
                *value = target;
            }
        }
    }
}

fn retarget_service_url(url: &str, old: &str, new: &str) -> Option<String> {
    for scheme in ["http", "https"] {
        if let Some(suffix) = url.strip_prefix(&format!("{scheme}://{old}")) {
            if suffix.is_empty() || suffix.starts_with([':', '/', '?', '#']) {
                return Some(format!("{scheme}://{new}{suffix}"));
            }
        }
    }
    None
}

pub(crate) fn cleanup_deleted_native_service(
    root: &Path,
    name: &str,
    features: &[String],
) -> Result<()> {
    let mut edit = CapabilityEdit::new(root, "", name, root, vec![], vec![]);
    edit.native = true;
    let state_path = root.join(format!(".forklaunch/native-capabilities/{name}.json"));
    let original_mock = edit
        .read(&state_path)?
        .map(|text| serde_json::from_str::<Ownership>(&text))
        .transpose()?
        .and_then(|state| state.original_mock);
    let mut gateway_state = edit
        .read(&root.join(MOCK_STATE))?
        .map(|text| serde_json::from_str::<GatewayOwnership>(&text))
        .transpose()?;
    if let Some(state) = gateway_state.as_mut() {
        remember_gateway_edits(state, current_gateway_mock(&edit)?)?;
    }
    if !features.is_empty() && root.join("docker-compose.yaml").exists() {
        let manifest: toml::Value = toml::from_str(&std::fs::read_to_string(
            root.join(".forklaunch/manifest.toml"),
        )?)?;
        let remaining = capability_owners(&manifest, None);
        let app_empty = remaining.is_empty();
        edit.update_compose(|compose| {
            for feature in features {
                let previous = compose
                    .services
                    .get(GATEWAY_MOCK_SERVICE)
                    .and_then(|mock| mock.environment.as_ref())
                    .and_then(|env| env.get(&format!("MOCK_EVENTS_URL_{}", feature.to_uppercase())))
                    .cloned();
                release_gateway_mock(compose, name, feature, true, remaining.is_empty());
                retain_callback_target(compose, name, feature, previous, &remaining);
            }
            for (feature, service) in &remaining {
                let port = compose
                    .services
                    .get(service)
                    .and_then(|service| service.environment.as_ref())
                    .and_then(|env| env.get("SERVICE_PORT").or_else(|| env.get("PORT")))
                    .cloned()
                    .unwrap_or_else(|| "8000".into());
                if let Some(mock) = compose.services.get_mut(GATEWAY_MOCK_SERVICE) {
                    mock.environment
                        .get_or_insert_with(Default::default)
                        .entry(format!("MOCK_EVENTS_URL_{}", feature.to_uppercase()))
                        .or_insert_with(|| format!("http://{service}:{port}"));
                }
            }
            if remaining.is_empty() && !compose.services.contains_key(GATEWAY_MOCK_SERVICE) {
                if let Some(mock) = gateway_state
                    .as_ref()
                    .map(|state| state.original.clone())
                    .unwrap_or(original_mock)
                {
                    compose.services.insert(GATEWAY_MOCK_SERVICE.into(), mock);
                }
            }
            Ok(())
        })?;
        if app_empty {
            edit.delete(root.join(MOCK_STATE));
        } else if let Some(mut state) = gateway_state {
            state.last = current_gateway_mock(&edit)?;
            edit.write(
                root.join(MOCK_STATE),
                serde_json::to_string_pretty(&state)? + "\n",
            );
        }
    }
    edit.delete(state_path);
    edit.commit()
}
#[derive(Default, Serialize, Deserialize)]
struct Ownership {
    original_cargo: String,
    last_cargo: String,
    cargo_modified: bool,
    #[serde(default)]
    dependency_overrides: BTreeMap<String, String>,
    original_module: Option<String>,
    last_module: Option<String>,
    module_modified: bool,
    original_env: Option<String>,
    last_env: Option<String>,
    env_modified: bool,
    lib_export_added: bool,
    main_import_added: bool,
    registry_added: bool,
    router_added: bool,
    original_service_env: BTreeMap<String, String>,
    original_gateway_dependency: Option<DependsOn>,
    original_mock: Option<DockerService>,
}

pub(super) fn apply(
    edit: &mut CapabilityEdit,
    capability: &Capability,
    adding: bool,
) -> Result<()> {
    // Ownership snapshots may include pre-existing local settings. Keep them
    // out of source control; CapabilityEdit writes these files privately.
    let ignore_path = edit
        .app_root
        .join(".forklaunch/native-capabilities/.gitignore");
    let mut ignore = edit.read(&ignore_path)?.unwrap_or_default();
    if !ignore.lines().any(|line| line == "/*.json") {
        if !ignore.is_empty() && !ignore.ends_with('\n') {
            ignore.push('\n');
        }
        ignore.push_str("/*.json\n");
        edit.write(ignore_path, ignore);
    }
    let state_path = edit
        .app_root
        .join(".forklaunch/native-capabilities")
        .join(format!("{}.json", edit.service_name));
    let cargo_path = edit.service_path.join("Cargo.toml");
    let module_path = edit.service_path.join("src/capabilities/mod.rs");
    let nonce_store_path = edit.service_path.join("src/capabilities/nonce_store.rs");
    if !edit.service_capabilities.is_empty() && edit.read(&nonce_store_path)?.is_none() {
        edit.write(nonce_store_path.clone(),NONCE_STORE.into());
    }
    let env_path = edit.service_path.join(".env.local");
    let lib_path = edit.service_path.join("src/lib.rs");
    let main_path = edit.service_path.join("src/main.rs");
    let cargo_text = edit
        .read(&cargo_path)?
        .context("Native service Cargo.toml is missing")?;
    let mut cargo: DocumentMut = cargo_text.parse()?;
    let crate_module = cargo["package"]["name"]
        .as_str()
        .context("Native service package.name is missing")?
        .replace('-', "_");
    let import = format!("use {crate_module}::capabilities::ManagedCapabilitiesRouterExt;\n");
    let mut lib = edit
        .read(&lib_path)?
        .context("Native service src/lib.rs is missing")?;
    let mut main = edit
        .read(&main_path)?
        .context("Native service src/main.rs is missing")?;
    let current_module = edit.read(&module_path)?;
    let current_env = edit.read(&env_path)?;
    let gateway_path = edit.app_root.join(MOCK_STATE);
    if adding && edit.app_capabilities.len() == 1 && edit.read(&gateway_path)?.is_none() {
        let original = edit
            .read(&edit.app_root.join("docker-compose.yaml"))?
            .map(|source| serde_yml::from_str::<DockerCompose>(&source))
            .transpose()?
            .and_then(|compose| compose.services.get(GATEWAY_MOCK_SERVICE).cloned());
        edit.write(
            gateway_path.clone(),
            serde_json::to_string_pretty(&GatewayOwnership {
                original,
                last: None,
            })? + "\n",
        );
    }
    let mut gateway_ownership = edit
        .read(&gateway_path)?
        .map(|source| serde_json::from_str::<GatewayOwnership>(&source))
        .transpose()?;
    if let Some(state) = gateway_ownership.as_mut() {
        remember_gateway_edits(state, current_gateway_mock(edit)?)?;
    }
    let mut state = if let Some(text) = edit.read(&state_path)? {
        serde_json::from_str::<Ownership>(&text)?
    } else {
        let recovered = current_module
            .as_deref()
            .is_some_and(|source| source.contains(BEGIN));
        if !adding && !recovered {
            bail!("Native capability ownership record and generated section are missing");
        }
        // Local restoration records are deliberately ignored by Git. A clone
        // can still remove generated hooks, but retains dependencies/settings
        // whose prior ownership cannot be established without that record.
        let original_module = if recovered {
            let kept = replace_owned_block(current_module.as_deref().unwrap_or_default(), "")?;
            (!kept.trim().is_empty()).then_some(kept)
        } else {
            current_module.clone()
        };
        let compose = edit
            .read(&edit.app_root.join("docker-compose.yaml"))?
            .map(|text| serde_yml::from_str::<DockerCompose>(&text))
            .transpose()?;
        let service = compose
            .as_ref()
            .and_then(|compose| compose.services.get(&edit.service_name));
        Ownership {
            original_cargo: cargo_text.clone(),
            last_cargo: cargo_text.clone(),
            cargo_modified: false,
            dependency_overrides: BTreeMap::new(),
            original_module,
            last_module: current_module.clone(),
            module_modified: false,
            original_env: current_env.clone(),
            last_env: current_env.clone(),
            env_modified: false,
            lib_export_added: recovered || !lib.contains(LIB_EXPORT),
            main_import_added: recovered || !main.contains(&import),
            registry_added: recovered || !lib.contains(REGISTRY),
            router_added: recovered || !main.contains(".with_managed_capabilities()"),
            original_service_env: service
                .and_then(|service| service.environment.as_ref())
                .map(|env| {
                    env.iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            original_gateway_dependency: service
                .and_then(|service| service.depends_on.as_ref())
                .and_then(|depends| depends.get(GATEWAY_MOCK_SERVICE))
                .cloned(),
            original_mock: if !recovered && edit.app_capabilities.len() == 1 {
                compose
                    .as_ref()
                    .and_then(|compose| compose.services.get(GATEWAY_MOCK_SERVICE))
                    .cloned()
            } else {
                None
            },
        }
    };
    remember_dependency_edits(&mut state, &cargo_text)?;
    state.cargo_modified |= cargo_text != state.last_cargo;
    state.module_modified |= current_module != state.last_module;
    state.env_modified |= current_env != state.last_env;
    let handler = edit
        .service_path
        .join(format!("src/capabilities/{}.rs", capability.id));
    if adding {
        if edit.read(&handler)?.is_none() {
            edit.write(handler, HANDLER.into());
        }
        if !lib.contains(LIB_EXPORT) {
            lib = format!("{LIB_EXPORT}{lib}");
        }
        if !main.contains(&import) {
            main = format!("{import}{main}");
        }
        if !lib.contains(REGISTRY) {
            let start = lib
                .find("fn contract_registry(")
                .context("Native service must expose contract_registry()")?;
            let end = start
                + lib[start..]
                    .find("    registry\n}")
                    .context("Native contract registry must return its registry")?;
            lib.insert_str(end, REGISTRY);
        }
        if !main.contains(".with_managed_capabilities()") {
            let marker = ".into_router_with_default_mcp()";
            if main.matches(marker).count() != 1 {
                bail!("Native capability wiring requires one generated HTTP/MCP router mount");
            }
            main = main.replace(
                marker,
                ".with_managed_capabilities().into_router_with_default_mcp()",
            );
        }
        let depth = edit
            .service_path
            .strip_prefix(&edit.app_root)?
            .components()
            .count();
        let prefix = std::iter::repeat_n("..", depth)
            .collect::<Vec<_>>()
            .join("/");
        let prefix = if prefix.is_empty() {
            "."
        } else {
            prefix.as_str()
        };
        ensure_dependency(
            &mut cargo,
            "forklaunch-blueprint-core",
            &format!("{prefix}/.forklaunch/blueprint-rs/crates/forklaunch-blueprint-core"),
            Some("instance-gateway"),
        )?;
        if capability.id == "payments" {
            ensure_dependency(
                &mut cargo,
                "forklaunch-billing-stripe",
                &format!(
                    "{prefix}/.forklaunch/blueprint-rs/crates/implementations/billing/forklaunch-billing-stripe"
                ),
                None,
            )?;
        }
        let pairs = local_gateway_env();
        let pairs: Vec<_> = pairs
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        edit.ensure_env_local(&pairs)?;
        let service_name = edit.service_name.clone();
        let feature = capability.id;
        edit.update_compose(|compose| {
            ensure_gateway_mock(compose, &service_name, feature)?;
            let service = compose
                .services
                .get_mut(&service_name)
                .context("Native service is missing from Compose")?;
            let env = service.environment.get_or_insert_with(Default::default);
            env.entry("FORKLAUNCH_LOCAL_GATEWAY_MOCK".into())
                .or_insert_with(|| "true".into());
            let port = env
                .get("SERVICE_PORT")
                .or_else(|| env.get("PORT"))
                .cloned()
                .unwrap_or_else(|| "8000".into());
            if let Some(mock) = compose.services.get_mut(GATEWAY_MOCK_SERVICE) {
                mock.environment
                    .get_or_insert_with(Default::default)
                    .insert(
                        format!("MOCK_EVENTS_URL_{}", feature.to_uppercase()),
                        format!("http://{service_name}:{port}"),
                    );
            }
            Ok(())
        })?;
    } else {
        if edit.read(&handler)?.as_deref() == Some(HANDLER) {
            edit.delete(handler);
        }
        let service = edit.service_name.clone();
        let feature = capability.id;
        let service_empty = edit.service_capabilities.is_empty();
        let app_empty = edit.app_capabilities.is_empty();
        let manifest: toml::Value = toml::from_str(
            &edit
                .read(&edit.app_root.join(".forklaunch/manifest.toml"))?
                .context("Application manifest is missing")?,
        )?;
        let owners = capability_owners(&manifest, Some((&service, &edit.service_capabilities)));
        let global_original = gateway_ownership
            .as_ref()
            .map(|state| state.original.clone())
            .unwrap_or_else(|| state.original_mock.clone());
        edit.update_compose(|compose| {
            let current_dependency = compose
                .services
                .get(&service)
                .and_then(|target| target.depends_on.as_ref())
                .and_then(|dependencies| dependencies.get(GATEWAY_MOCK_SERVICE))
                .cloned();
            let default_dependency = crate::core::docker::DependsOn::default();
            let retained_dependency = match current_dependency {
                Some(dependency)
                    if state.original_gateway_dependency.is_some()
                        || serde_json::to_value(&dependency)?
                            != serde_json::to_value(default_dependency)? =>
                {
                    Some(dependency)
                }
                _ => None,
            };
            let removed_settings = compose
                .services
                .get(&service)
                .and_then(|target| target.environment.as_ref())
                .map(|env| {
                    super::compose_gateway_env()
                        .into_iter()
                        .filter(|(key, value)| env.get(*key) == Some(value))
                        .map(|(key, _)| key)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let previous = compose
                .services
                .get(GATEWAY_MOCK_SERVICE)
                .and_then(|mock| mock.environment.as_ref())
                .and_then(|env| env.get(&format!("MOCK_EVENTS_URL_{}", feature.to_uppercase())))
                .cloned();
            release_gateway_mock(compose, &service, feature, service_empty, app_empty);
            retain_callback_target(compose, &service, feature, previous, &owners);
            if service_empty {
                if let Some(target) = compose.services.get_mut(&service) {
                    let env = target.environment.get_or_insert_with(Default::default);
                    if !state
                        .original_service_env
                        .contains_key("FORKLAUNCH_LOCAL_GATEWAY_MOCK")
                        && env
                            .get("FORKLAUNCH_LOCAL_GATEWAY_MOCK")
                            .is_some_and(|value| value == "true")
                    {
                        env.shift_remove("FORKLAUNCH_LOCAL_GATEWAY_MOCK");
                    }
                    for key in removed_settings {
                        if let Some(value) = state.original_service_env.get(key) {
                            env.entry(key.into()).or_insert_with(|| value.clone());
                        }
                    }
                    if let Some(dependency) = &retained_dependency {
                        target
                            .depends_on
                            .get_or_insert_with(Default::default)
                            .entry(GATEWAY_MOCK_SERVICE.into())
                            .or_insert_with(|| dependency.clone());
                    }
                }
                if app_empty && !compose.services.contains_key(GATEWAY_MOCK_SERVICE) {
                    if let Some(mock) = global_original.as_ref() {
                        compose
                            .services
                            .insert(GATEWAY_MOCK_SERVICE.into(), mock.clone());
                    }
                }
            }
            Ok(())
        })?;
        if app_empty {
            edit.delete(gateway_path.clone());
        }
    }
    if !edit.app_capabilities.is_empty() {
        if let Some(mut state) = gateway_ownership {
            state.last = current_gateway_mock(edit)?;
            edit.write(gateway_path, serde_json::to_string_pretty(&state)? + "\n");
        }
    }
    if edit.service_capabilities.is_empty() {
        if edit.read(&nonce_store_path)?.as_deref()==Some(NONCE_STORE) { edit.delete(nonce_store_path); }
        if !state.cargo_modified && cargo_text == state.last_cargo {
            edit.write(cargo_path, state.original_cargo.clone());
        } else {
            restore_dependencies(&mut cargo, &state)?;
            edit.write(cargo_path, cargo.to_string());
        }
        if !state.module_modified && current_module == state.last_module {
            if let Some(original) = &state.original_module {
                edit.write(module_path, original.clone());
            } else {
                edit.delete(module_path);
            }
        } else if let Some(current) = current_module {
            let kept = replace_owned_block(&current, "")?;
            if kept.trim().is_empty() {
                edit.delete(module_path);
            } else {
                edit.write(module_path, kept);
            }
        }
        if state.lib_export_added
            && edit
                .read(&edit.service_path.join("src/capabilities/mod.rs"))?
                .is_none()
        {
            lib = lib.replace(LIB_EXPORT, "");
        }
        if state.registry_added {
            lib = lib.replace(REGISTRY, "");
        }
        if state.main_import_added {
            main = main.replace(&import, "");
        }
        if state.router_added {
            main = main.replace(".with_managed_capabilities()", "");
        }
        if !state.env_modified && current_env == state.last_env {
            if let Some(original) = &state.original_env {
                edit.write(env_path, original.clone());
            } else {
                edit.delete(env_path);
            }
        } else if let Some(current) = current_env {
            let defaults = local_gateway_env();
            let kept: String = current
                .split_inclusive('\n')
                .filter(|line| {
                    !defaults.iter().any(|(key, value)| {
                        !state
                            .original_env
                            .as_deref()
                            .unwrap_or_default()
                            .lines()
                            .any(|line| line.starts_with(&format!("{key}=")))
                            && line.trim_end() == format!("{key}={value}")
                    })
                })
                .collect();
            edit.write(env_path, kept);
        }
        edit.delete(state_path);
    } else {
        let generated = render_module(&edit.service_capabilities)?;
        let module = match current_module {
            Some(current) if current.contains(BEGIN) => replace_owned_block(&current, &generated)?,
            Some(current) => format!(
                "{current}{}{generated}",
                if current.ends_with('\n') { "" } else { "\n" }
            ),
            None => generated,
        };
        edit.write(module_path, module.clone());
        let updated = cargo.to_string();
        edit.write(cargo_path, updated.clone());
        state.last_cargo = updated;
        state.last_module = Some(module);
        state.last_env = edit.read(&env_path)?;
        edit.write(state_path, serde_json::to_string_pretty(&state)? + "\n");
    }
    edit.write(lib_path, lib);
    edit.write(main_path, main);
    Ok(())
}
pub(crate) fn ensure_dependency(
    cargo: &mut DocumentMut,
    name: &str,
    path: &str,
    feature: Option<&str>,
) -> Result<()> {
    if cargo["dependencies"].get(name).is_none() {
        let mut table = InlineTable::new();
        table.insert("path", path.into());
        cargo["dependencies"][name] = value(table);
    }
    if let Some(feature) = feature {
        if let Some(version) = cargo["dependencies"][name].as_str() {
            let mut table = InlineTable::new();
            table.insert("version", version.into());
            cargo["dependencies"][name] = value(table);
        }
        if cargo["dependencies"][name].get("features").is_none() {
            cargo["dependencies"][name]["features"] = value(Array::new());
        }
        let features = cargo["dependencies"][name]["features"]
            .as_array_mut()
            .context("Native dependency features must be an array")?;
        if !features.iter().any(|value| value.as_str() == Some(feature)) {
            features.push(feature);
        }
    }
    Ok(())
}
fn restore_dependencies(cargo: &mut DocumentMut, state: &Ownership) -> Result<()> {
    let before: DocumentMut = state.original_cargo.parse()?;
    let after: toml::Value = toml::from_str(&state.last_cargo)?;
    let current: toml::Value = toml::from_str(&cargo.to_string())?;
    for name in ["forklaunch-blueprint-core", "forklaunch-billing-stripe"] {
        let original = state
            .dependency_overrides
            .get(name)
            .map(|source| source.parse::<DocumentMut>())
            .transpose()?
            .unwrap_or_else(|| before.clone());
        if state.dependency_overrides.contains_key(name)
            || current.get("dependencies").and_then(|deps| deps.get(name))
                == after.get("dependencies").and_then(|deps| deps.get(name))
        {
            if let Some(original) = original["dependencies"].get(name) {
                cargo["dependencies"][name] = original.clone();
            } else if let Some(dependencies) = cargo["dependencies"].as_table_like_mut() {
                dependencies.remove(name);
            }
        }
    }
    Ok(())
}

fn remember_dependency_edits(state: &mut Ownership, source: &str) -> Result<()> {
    if source == state.last_cargo {
        return Ok(());
    }
    let last: toml::Value = toml::from_str(&state.last_cargo)?;
    let current: toml::Value = toml::from_str(source)?;
    for name in ["forklaunch-blueprint-core", "forklaunch-billing-stripe"] {
        let current = current.get("dependencies").and_then(|deps| deps.get(name));
        let previous = last.get("dependencies").and_then(|deps| deps.get(name));
        if current != previous {
            state
                .dependency_overrides
                .insert(name.into(), source.into());
        }
    }
    Ok(())
}
fn replace_owned_block(current: &str, replacement: &str) -> Result<String> {
    let start = current
        .find(BEGIN)
        .context("Native capability generated section is missing")?;
    let end = start
        + current[start..]
            .find(END)
            .context("Native capability generated section is incomplete")?
        + END.len();
    Ok(format!(
        "{}{}{}",
        &current[..start],
        replacement,
        &current[end..]
    ))
}
fn render_module(features: &[String]) -> Result<String> {
    let mut modules = Vec::new();
    let mut arms = Vec::new();
    let mut known = Vec::new();
    let mut factories = Vec::new();
    for feature in features {
        if super::find(feature).is_none() {
            bail!("Unknown native managed capability '{feature}'");
        }
        modules.push(format!("pub mod {feature};"));
        arms.push(format!(
            "                        \"{feature}\" => {feature}::handle(event).await,"
        ));
        known.push(format!("\"{feature}\""));
        let (module, ty) = match feature.as_str() {
            "email" => ("email_client", "EmailClient"),
            "sms" => ("sms_client", "SmsClient"),
            "whatsapp" => ("whatsapp_client", "WhatsAppClient"),
            "voice" => ("voice_client", "VoiceClient"),
            "payments" => ("payments_client", "PaymentsClient"),
            _ => unreachable!(),
        };
        if feature == "sms" {
            factories.push("pub fn sms_client() -> BlueprintResult<forklaunch_blueprint_core::sms_client::SmsClient> {\n    if ![\"PLATFORM_GATEWAY_URL\", \"INSTANCE_ID\", \"INSTANCE_HMAC_KEY\"].iter().all(|name| std::env::var(name).is_ok_and(|value| !value.is_empty())) {\n        return Ok(forklaunch_blueprint_core::sms_client::SmsClient::deferred());\n    }\n    Ok(forklaunch_blueprint_core::sms_client::SmsClient::new(instance_gateway()?))\n}".into());
        } else {
            factories.push(format!("pub fn {feature}_client() -> BlueprintResult<forklaunch_blueprint_core::{module}::{ty}> {{ Ok(forklaunch_blueprint_core::{module}::{ty}::new(instance_gateway()?)) }}"));
        }
        if feature == "payments" {
            factories.push("pub fn stripe_client() -> BlueprintResult<forklaunch_billing_stripe::client::StripeBillingClient> { Ok(forklaunch_billing_stripe::client::StripeBillingClient::with_gateway(instance_gateway()?)) }".into());
        }
    }
    Ok(MODULE
        .replace("__EVENT_MODULES__", &modules.join("\n"))
        .replace("__HANDLER_ARMS__", &arms.join("\n"))
        .replace("__KNOWN_FEATURES__", &known.join(" | "))
        .replace("__CLIENT_FACTORIES__", &factories.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacked_updates_preserve_human_dependency_features_and_removals() {
        let before = "[dependencies]\nuser = \"1\"\n";
        let generated = "[dependencies]\nuser = \"1\"\nforklaunch-blueprint-core = { path = \"owned\", features = [\"instance-gateway\"] }\n";
        for human in [
            "[dependencies]\nuser = \"1\"\nforklaunch-blueprint-core = { path = \"custom\", features = [\"instance-gateway\", \"stripe-webhook\"] }\n",
            before,
        ] {
            let mut state = Ownership {
                original_cargo: before.into(),
                last_cargo: generated.into(),
                ..Default::default()
            };
            let mut cargo: DocumentMut = human.parse().unwrap();
            remember_dependency_edits(&mut state, human).unwrap();
            // A later capability command can add a dependency back to compile
            // its handlers; removal must recover the user's prior edit.
            ensure_dependency(
                &mut cargo,
                "forklaunch-blueprint-core",
                "owned",
                Some("instance-gateway"),
            )
            .unwrap();
            state.last_cargo = cargo.to_string();
            restore_dependencies(&mut cargo, &state).unwrap();
            assert_eq!(cargo.to_string(), human);
        }
    }

    #[test]
    fn cargo_reformatting_is_not_a_human_dependency_override() {
        let before = "[dependencies]\nuser = \"1\"\n";
        let generated = "[dependencies]\nuser = \"1\"\nforklaunch-blueprint-core = { path = \"owned\", features = [\"instance-gateway\"] }\n";
        let formatted =
            toml::to_string_pretty(&toml::from_str::<toml::Value>(generated).unwrap()).unwrap();
        let mut state = Ownership {
            original_cargo: before.into(),
            last_cargo: generated.into(),
            ..Default::default()
        };
        remember_dependency_edits(&mut state, &formatted).unwrap();
        assert!(state.dependency_overrides.is_empty());
        let mut cargo: DocumentMut = formatted.parse().unwrap();
        restore_dependencies(&mut cargo, &state).unwrap();
        assert_eq!(
            toml::from_str::<toml::Value>(&cargo.to_string()).unwrap(),
            toml::from_str::<toml::Value>(before).unwrap()
        );
    }

    #[test]
    fn deletion_reads_a_manifest_document_and_releases_only_the_last_gateway() {
        for another_service in [false, true] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
            let manifest = if another_service {
                "app_description = \"Document parsing regression\"\n[[projects]]\nname = \"other\"\n[projects.resources]\ncapabilities = [\"sms\"]\n"
            } else {
                "app_description = \"Document parsing regression\"\nprojects = []\n"
            };
            std::fs::write(root.path().join(".forklaunch/manifest.toml"), manifest).unwrap();
            std::fs::write(root.path().join("docker-compose.yaml"), "volumes: {}\nnetworks: {}\nservices:\n  removed:\n    environment:\n      INSTANCE_ID: local-instance\n  other:\n    environment:\n      SERVICE_PORT: '8123'\n  gateway-mock:\n    image: node:22-alpine\n    environment:\n      MOCK_EVENTS_URL_EMAIL: http://removed:8000\n      MOCK_EVENTS_URL_SMS: http://other:8123\n").unwrap();
            cleanup_deleted_native_service(root.path(), "removed", &["email".into()]).unwrap();
            let compose: DockerCompose = serde_yml::from_str(
                &std::fs::read_to_string(root.path().join("docker-compose.yaml")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                compose.services.contains_key(GATEWAY_MOCK_SERVICE),
                another_service
            );
            if another_service {
                let env = compose.services[GATEWAY_MOCK_SERVICE]
                    .environment
                    .as_ref()
                    .unwrap();
                assert!(!env.contains_key("MOCK_EVENTS_URL_EMAIL"));
                assert_eq!(env["MOCK_EVENTS_URL_SMS"], "http://other:8123");
            }
        }
    }

    #[test]
    fn renaming_callbacks_preserves_other_services_and_custom_origins() {
        let mut mock: DockerService = serde_yml::from_str("image: node:22-alpine\nenvironment:\n  MOCK_EVENTS_URL_EMAIL: http://old:8123\n  MOCK_EVENTS_URL_SMS: https://old:8443/base\n  MOCK_EVENTS_URL_VOICE: https://old.example:8443\n  OTHER_URL: http://old:8123\n").unwrap();
        rename_callback_targets(&mut mock, "old", "new");
        let env = mock.environment.unwrap();
        assert_eq!(env["MOCK_EVENTS_URL_EMAIL"], "http://new:8123");
        assert_eq!(env["MOCK_EVENTS_URL_SMS"], "https://new:8443/base");
        assert_eq!(env["MOCK_EVENTS_URL_VOICE"], "https://old.example:8443");
        assert_eq!(env["OTHER_URL"], "http://old:8123");
    }
}
