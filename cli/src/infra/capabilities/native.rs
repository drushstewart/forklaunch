//! Native capability edits. They keep application source, and removal is
//! stateless: it takes back what `infra add` writes for the service today
//! (generated files, the marked `FORKLAUNCH MANAGED CAPABILITIES` sections,
//! hooks, dependencies, settings), and keeps anything edited since.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use toml_edit::{Array, DocumentMut, InlineTable, value};

use super::{
    Capability, CapabilityEdit, GATEWAY_MOCK_SERVICE, ensure_gateway_mock, generated_gateway_mock,
    local_gateway_env, release_gateway_mock,
};
use crate::core::docker::{DependencyCondition, DependsOn, DockerCompose, DockerService};
const MODULE: &str = include_str!("../../templates/rust/capabilities/mod.rs");
const NATIVE_MODULE: &str = include_str!("../../templates/rust/capabilities/native_mod.rs");
const CONTRACT: &str = include_str!("../../templates/rust/capabilities/contract.rs");
const CONTROLLER: &str = include_str!("../../templates/rust/capabilities/controller.rs");
const CONTRACT_EXPORT: &str = "pub mod platform_events;\n";
const BUILTIN_CONTRACT_EXPORT: &str =
    "#[path = \"platform_events.rs\"]\npub mod platform_events;\n";
const NATIVE_REGISTRY: &str = "    platform_events::register_contracts(&mut registry);\n";
const HANDLER: &str = include_str!("../../templates/rust/capabilities/handler.rs");
const NONCE_STORE: &str = include_str!("../../templates/rust/capabilities/nonce_store.rs");
const BEGIN: &str = "// BEGIN FORKLAUNCH MANAGED CAPABILITIES\n";
const END: &str = "// END FORKLAUNCH MANAGED CAPABILITIES\n";
const LIB_EXPORT: &str = "pub mod capabilities;\n";
const REGISTRY: &str = "    capabilities::register_contracts(&mut registry);\n";
const STRIPE: &str = "forklaunch-billing-stripe";

fn events_var(feature: &str) -> String {
    format!("MOCK_EVENTS_URL_{}", feature.to_uppercase())
}

/// Whether `url` is an event destination `infra add` writes: a compose
/// service's own `http://<service>:<port>`. Anything else is the developer's.
fn service_callback(compose: &DockerCompose, url: &str) -> bool {
    url.strip_prefix("http://")
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(host, port)| {
            compose.services.contains_key(host)
                && !port.is_empty()
                && port.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// The `depends_on` entry add gives a service on the gateway mock.
fn generated_gateway_dependency() -> DependsOn {
    DependsOn {
        condition: DependencyCondition::ServiceStarted,
        ..Default::default()
    }
}

/// Remove the gateway mock once no project holds a capability, while it is
/// still the generated service (its event destinations aside). An edited
/// mock is the developer's.
fn release_unused_gateway_mock(edit: &CapabilityEdit, compose: &mut DockerCompose) -> Result<()> {
    let Some(current) = compose.services.get(GATEWAY_MOCK_SERVICE) else {
        return Ok(());
    };
    let mut candidate = current.clone();
    if let Some(env) = candidate.environment.as_mut() {
        env.retain(|key, _| !key.starts_with("MOCK_EVENTS_URL_"));
    }
    let generated = generated_gateway_mock(current.networks.clone());
    if serde_json::to_value(&candidate)? == serde_json::to_value(&generated)? {
        compose.services.shift_remove(GATEWAY_MOCK_SERVICE);
    } else {
        edit.note_kept_setting(
            &edit.app_root.join("docker-compose.yaml"),
            &format!("service {GATEWAY_MOCK_SERVICE}"),
        );
    }
    Ok(())
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
            .insert(events_var(feature), target);
    }
}

/// After a service rename: the mock's event destinations follow it.
pub(crate) fn rename_native_capabilities(root: &Path, old: &str, new: &str) -> Result<()> {
    if !root.join("docker-compose.yaml").exists() {
        return Ok(());
    }
    let mut edit = CapabilityEdit::new(root, "", new, root, vec![], vec![]);
    edit.native = true;
    edit.update_compose(|compose| {
        if let Some(mock) = compose.services.get_mut(GATEWAY_MOCK_SERVICE) {
            rename_callback_targets(mock, old, new);
        }
        Ok(())
    })?;
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

/// After `delete service`: drop the deleted service's event destinations,
/// promote another holder of each feature, and remove the mock with the last.
pub(crate) fn cleanup_deleted_native_service(
    root: &Path,
    name: &str,
    features: &[String],
) -> Result<()> {
    if features.is_empty() || !root.join("docker-compose.yaml").exists() {
        return Ok(());
    }
    let mut edit = CapabilityEdit::new(root, "", name, root, vec![], vec![]);
    edit.native = true;
    let manifest: toml::Value = toml::from_str(&std::fs::read_to_string(
        root.join(".forklaunch/manifest.toml"),
    )?)?;
    let remaining = capability_owners(&manifest, None);
    let mut compose = edit
        .read(&root.join("docker-compose.yaml"))?
        .map(|text| serde_yml::from_str::<DockerCompose>(&text))
        .transpose()?
        .context("docker-compose.yaml is missing")?;
    for feature in features {
        let previous = compose
            .services
            .get(GATEWAY_MOCK_SERVICE)
            .and_then(|mock| mock.environment.as_ref())
            .and_then(|env| env.get(&events_var(feature)))
            .cloned();
        let events = super::find(feature).is_none_or(|c| c.receives_events);
        release_gateway_mock(&mut compose, name, feature, events, true, false);
        retain_callback_target(&mut compose, name, feature, previous, &remaining);
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
                .entry(events_var(feature))
                .or_insert_with(|| format!("http://{service}:{port}"));
        }
    }
    if remaining.is_empty() {
        release_unused_gateway_mock(&edit, &mut compose)?;
    }
    edit.update_compose(|current| {
        *current = compose;
        Ok(())
    })?;
    edit.commit()
}

pub(super) fn apply(
    edit: &mut CapabilityEdit,
    capability: &Capability,
    adding: bool,
) -> Result<()> {
    let cargo_path = edit.service_path.join("Cargo.toml");
    let flattened = edit.read(&edit.service_path.join("lib.rs"))?.is_some();
    let source_root = if flattened {
        edit.service_path.clone()
    } else {
        edit.service_path.join("src")
    };
    let native_router = flattened
        && (edit
            .read(&edit.service_path.join("server.rs"))?
            .is_some_and(|source| source.contains("fn http_router("))
            || edit
                .read(&source_root.join("api/routes/mod.rs"))?
                .is_some_and(|source| source.contains("ContractRouter")));
    let builtin_contract = flattened
        && !native_router
        && edit
            .read(&source_root.join("api/contracts/registry.rs"))?
            .is_some();
    let shared_contract = native_router || builtin_contract;
    let module_path = source_root.join("capabilities/mod.rs");
    let nonce_store_path = source_root.join("capabilities/nonce_store.rs");
    let inline_contract = native_router
        && edit
            .read(&source_root.join(".forklaunch/contracts/lib.rs"))?
            .is_some();
    let contract_path = source_root.join(if inline_contract {
        "api/controllers/platform_events.rs"
    } else {
        "api/contracts/platform_events.rs"
    });
    let contract_template = if builtin_contract {
        builtin_capability_contract()
    } else if inline_contract {
        CONTROLLER.into()
    } else {
        CONTRACT.into()
    };
    let current_contract = if shared_contract {
        edit.read(&contract_path)?
    } else {
        None
    };
    if !edit.service_capabilities.is_empty() && edit.read(&nonce_store_path)?.is_none() {
        edit.write(nonce_store_path.clone(), NONCE_STORE.into());
    }
    let lib_path = source_root.join("lib.rs");
    let main_path = if native_router {
        source_root.join("api/routes/mod.rs")
    } else if flattened {
        source_root.join("server.rs")
    } else {
        source_root.join("main.rs")
    };
    let registry_path = if inline_contract {
        source_root.join(".forklaunch/contracts/lib.rs")
    } else if builtin_contract {
        source_root.join("api/contracts/registry.rs")
    } else if native_router {
        source_root.join("api/contracts/mod.rs")
    } else if flattened {
        source_root.join("sdk.rs")
    } else {
        lib_path.clone()
    };
    let registry_hook = if shared_contract {
        NATIVE_REGISTRY
    } else if flattened {
        "    crate::capabilities::register_contracts(&mut registry);\n"
    } else {
        REGISTRY
    };
    let cargo_text = edit
        .read(&cargo_path)?
        .context("Native service Cargo.toml is missing")?;
    let mut cargo: DocumentMut = cargo_text.parse()?;
    let crate_module = cargo["package"]["name"]
        .as_str()
        .context("Native service package.name is missing")?
        .replace('-', "_");
    let namespace = if flattened { "crate" } else { &crate_module };
    let import = format!("use {namespace}::capabilities::ManagedCapabilitiesRouterExt;\n");
    let mut lib = edit
        .read(&lib_path)?
        .context("Native service src/lib.rs is missing")?;
    let mut main = edit
        .read(&main_path)?
        .context("Native service src/main.rs is missing")?;
    let mut registry_source = edit
        .read(&registry_path)?
        .context("Service contract registry source is missing")?;
    let current_module = edit.read(&module_path)?;
    let depth = edit
        .service_path
        .strip_prefix(&edit.app_root)?
        .components()
        .count();
    let prefix = std::iter::repeat_n("..", depth)
        .collect::<Vec<_>>()
        .join("/");
    let prefix = if prefix.is_empty() { "." } else { prefix.as_str() };
    let add_stripe = |cargo: &mut DocumentMut| {
        ensure_dependency(
            cargo,
            STRIPE,
            &format!("{prefix}/.forklaunch/blueprint-rs/crates/implementations/billing/billing-stripe"),
            None,
        )
    };
    let handler = source_root.join(format!("capabilities/{}.rs", capability.id));
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
        if registry_path == lib_path {
            registry_source = lib.clone();
        }
        if shared_contract {
            if !inline_contract && !builtin_contract && !registry_source.contains(CONTRACT_EXPORT) {
                registry_source = format!("{CONTRACT_EXPORT}{registry_source}");
            }
            if current_contract.is_none() {
                edit.write(contract_path.clone(), contract_template.clone());
            }
        }
        if builtin_contract {
            registry_source = builtin_capability_registry(&registry_source, true, false)?;
        } else if inline_contract {
            registry_source = inline_capability_registry(&registry_source, true)?;
        } else if !registry_source.contains(registry_hook) {
            let function = if native_router {
                "fn registry("
            } else {
                "fn contract_registry("
            };
            let start = registry_source
                .find(function)
                .context("Service must expose a contract registry")?;
            let end = start
                + registry_source[start..]
                    .find("    registry\n}")
                    .context("Contract registry must return its registry")?;
            registry_source.insert_str(end, registry_hook);
        }
        if registry_path == lib_path {
            lib = registry_source.clone();
        }
        if !main.contains(".with_managed_capabilities()") {
            main = if native_router {
                with_native_capability_router(&main)?
            } else {
                with_capability_router(&main)?
            };
        }
        // Platform clients live in the framework, as in `@forklaunch/core/http`.
        ensure_dependency_feature(&mut cargo, "dependencies", "forklaunch-core", "instance-gateway")?;
        if capability.id == "payments" {
            add_stripe(&mut cargo)?;
        }
        if builtin_contract {
            for scope in ["dependencies", "build-dependencies"] {
                ensure_dependency_feature(&mut cargo, scope, "forklaunch-core", "typed-schema")?;
            }
        }
        let pairs = local_gateway_env();
        let pairs: Vec<_> = pairs
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        edit.ensure_env_local(&pairs)?;
        let service_name = edit.service_name.clone();
        let feature = capability.id;
        let events = super::find(feature).is_none_or(|c| c.receives_events);
        edit.update_compose(|compose| {
            // One destination per feature, set here rather than appended.
            ensure_gateway_mock(compose, &service_name, feature, false)?;
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
            let current = compose
                .services
                .get(GATEWAY_MOCK_SERVICE)
                .and_then(|mock| mock.environment.as_ref())
                .and_then(|env| env.get(&events_var(feature)))
                .cloned();
            // The newest holder receives the events, unless the developer
            // pointed them elsewhere.
            if events
                && current.is_none_or(|url| service_callback(compose, &url))
                && let Some(mock) = compose.services.get_mut(GATEWAY_MOCK_SERVICE)
            {
                mock.environment
                    .get_or_insert_with(Default::default)
                    .insert(events_var(feature), format!("http://{service_name}:{port}"));
            }
            Ok(())
        })?;
    } else {
        match edit.read(&handler)? {
            Some(text) if edit.is_generated(&handler, &text, HANDLER) => edit.delete(handler),
            Some(_) => edit.note_kept(&handler),
            None => {}
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
        let mut compose = edit
            .read(&edit.app_root.join("docker-compose.yaml"))?
            .map(|text| serde_yml::from_str::<DockerCompose>(&text))
            .transpose()?;
        if let Some(compose) = compose.as_mut() {
            let dependency = compose
                .services
                .get(&service)
                .and_then(|target| target.depends_on.as_ref())
                .and_then(|dependencies| dependencies.get(GATEWAY_MOCK_SERVICE))
                .cloned();
            let previous = compose
                .services
                .get(GATEWAY_MOCK_SERVICE)
                .and_then(|mock| mock.environment.as_ref())
                .and_then(|env| env.get(&events_var(feature)))
                .cloned();
            let events = super::find(feature).is_none_or(|c| c.receives_events);
            release_gateway_mock(compose, &service, feature, events, service_empty, false);
            retain_callback_target(compose, &service, feature, previous, &owners);
            if service_empty && let Some(target) = compose.services.get_mut(&service) {
                let env = target.environment.get_or_insert_with(Default::default);
                if env
                    .get("FORKLAUNCH_LOCAL_GATEWAY_MOCK")
                    .is_some_and(|value| value == "true")
                {
                    env.shift_remove("FORKLAUNCH_LOCAL_GATEWAY_MOCK");
                }
                // A dependency with the developer's own options stays.
                if let Some(dependency) = dependency
                    && serde_json::to_value(&dependency)?
                        != serde_json::to_value(generated_gateway_dependency())?
                {
                    target
                        .depends_on
                        .get_or_insert_with(Default::default)
                        .insert(GATEWAY_MOCK_SERVICE.into(), dependency);
                }
            }
            if app_empty {
                release_unused_gateway_mock(edit, compose)?;
            }
        }
        if let Some(compose) = compose {
            edit.update_compose(|current| {
                *current = compose;
                Ok(())
            })?;
        }
    }
    let excluded = [source_root.join("capabilities")];
    if !adding
        && !edit.service_capabilities.iter().any(|id| id == "payments")
        && !dependency_in_use(&edit.service_path, &cargo, STRIPE, &excluded)
    {
        release_dependency(edit, &cargo_path, &mut cargo, STRIPE, add_stripe)?;
    }
    if edit.service_capabilities.is_empty() {
        match edit.read(&nonce_store_path)? {
            Some(text) if edit.is_generated(&nonce_store_path, &text, NONCE_STORE) => {
                edit.delete(nonce_store_path)
            }
            Some(_) => edit.note_kept(&nonce_store_path),
            None => {}
        }
        release_dependency_feature(&mut cargo, "dependencies", "forklaunch-core", "instance-gateway")?;
        if builtin_contract {
            for scope in ["dependencies", "build-dependencies"] {
                release_dependency_feature(&mut cargo, scope, "forklaunch-core", "typed-schema")?;
            }
        }
        if cargo.to_string() != cargo_text {
            edit.write(cargo_path, cargo.to_string());
        }
        // The marked section is the CLI's; the rest of the module is the developer's.
        let kept = match current_module {
            Some(current) if current.contains(BEGIN) => replace_owned_block(&current, "")?,
            Some(current) => current,
            None => String::new(),
        };
        let module_kept = !kept.trim().is_empty();
        if module_kept {
            edit.note_kept(&module_path);
            edit.write(module_path, kept);
        } else {
            edit.delete(module_path);
            lib = lib.replace(LIB_EXPORT, "");
        }
        if registry_path == lib_path {
            registry_source = lib.clone();
        }
        if builtin_contract {
            registry_source = builtin_capability_registry(&registry_source, false, false)?;
        } else if inline_contract {
            registry_source = inline_capability_registry(&registry_source, false)?;
        } else {
            registry_source = registry_source.replace(registry_hook, "");
        }
        if shared_contract {
            let contract_kept = match &current_contract {
                Some(text) if edit.is_generated(&contract_path, text, &contract_template) => {
                    edit.delete(contract_path.clone());
                    false
                }
                Some(_) => {
                    edit.note_kept(&contract_path);
                    true
                }
                None => false,
            };
            if !inline_contract && !contract_kept {
                registry_source = if builtin_contract {
                    builtin_capability_registry(&registry_source, false, true)?
                } else {
                    registry_source.replace(CONTRACT_EXPORT, "")
                };
            }
        }
        if registry_path == lib_path {
            lib = registry_source.clone();
        }
        main = main
            .replace(&import, "")
            .replace(".with_managed_capabilities()?", "")
            .replace(".with_managed_capabilities()", "");
        let pairs = local_gateway_env();
        let pairs: Vec<_> = pairs
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        edit.release_env_local(&pairs)?;
    } else {
        let generated = render_module_with_layout(&edit.service_capabilities, native_router)?;
        let generated = if inline_contract {
            inline_capability_module(&generated)
        } else if builtin_contract {
            builtin_capability_module(&generated)?
        } else {
            generated
        };
        let module = match current_module {
            Some(current) if current.contains(BEGIN) => replace_owned_block(&current, &generated)?,
            Some(current) => format!(
                "{current}{}{generated}",
                if current.ends_with('\n') { "" } else { "\n" }
            ),
            None => generated,
        };
        edit.write(module_path, module);
        // Blueprint dependencies resolve through the application's
        // `[framework]` selection, like every generated manifest.
        let updated = crate::init::rust_project::dependencies::render_for_path(&cargo_path, &cargo.to_string())?;
        if updated != cargo_text {
            edit.write(cargo_path, updated);
        }
    }
    if registry_path != lib_path {
        edit.write(registry_path, registry_source);
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
        ensure_dependency_feature(cargo, "dependencies", name, feature)?;
    }
    Ok(())
}
fn ensure_dependency_feature(
    cargo: &mut DocumentMut,
    scope: &str,
    name: &str,
    feature: &str,
) -> Result<()> {
    anyhow::ensure!(
        cargo.get(scope).and_then(|deps| deps.get(name)).is_some(),
        "Native {scope} must declare {name}"
    );
    if let Some(version) = cargo[scope][name].as_str() {
        let mut table = InlineTable::new();
        table.insert("version", version.into());
        cargo[scope][name] = value(table);
    }
    if cargo[scope][name].get("features").is_none() {
        cargo[scope][name]["features"] = value(Array::new());
    }
    let features = cargo[scope][name]["features"]
        .as_array_mut()
        .context("Native dependency features must be an array")?;
    if !features.iter().any(|value| value.as_str() == Some(feature)) {
        // Follow the array's layout, so a one-per-line list stays one per line.
        let prefix = features
            .iter()
            .last()
            .and_then(|last| last.decor().prefix())
            .and_then(|prefix| prefix.as_str())
            .map(str::to_owned);
        let mut item = toml_edit::Value::from(feature);
        if let Some(prefix) = prefix {
            item.decor_mut().set_prefix(prefix);
        }
        features.push_formatted(item);
    }
    Ok(())
}

/// Undo [`ensure_dependency_feature`]: drop `feature` from `name`'s list, the
/// list once empty, and the table form a bare version requirement was given.
fn release_dependency_feature(
    cargo: &mut DocumentMut,
    scope: &str,
    name: &str,
    feature: &str,
) -> Result<()> {
    let Some(dependency) = cargo
        .get_mut(scope)
        .and_then(|deps| deps.get_mut(name))
        .and_then(|item| item.as_table_like_mut())
    else {
        return Ok(());
    };
    let Some(features) = dependency
        .get_mut("features")
        .and_then(|item| item.as_array_mut())
    else {
        return Ok(());
    };
    let Some(index) = features.iter().position(|value| value.as_str() == Some(feature)) else {
        return Ok(());
    };
    features.remove(index);
    if !features.is_empty() {
        return Ok(());
    }
    dependency.remove("features");
    let version = cargo[scope][name]
        .as_inline_table()
        .filter(|table| table.len() == 1)
        .and_then(|table| table.get("version"))
        .and_then(|version| version.as_str())
        .map(str::to_owned);
    if let Some(version) = version {
        cargo[scope][name] = value(version);
    }
    Ok(())
}

/// Remove dependency `name` while it still has the form `add` gives it in
/// this manifest (rendered through the application's `[framework]`
/// selection). One the developer changed stays, with a note.
pub(crate) fn release_dependency(
    edit: &mut CapabilityEdit,
    cargo_path: &Path,
    cargo: &mut DocumentMut,
    name: &str,
    add: impl Fn(&mut DocumentMut) -> Result<()>,
) -> Result<()> {
    let current: toml::Value = toml::from_str(&cargo.to_string())?;
    let Some(current) = current.get("dependencies").and_then(|deps| deps.get(name)) else {
        return Ok(());
    };
    let mut fresh = cargo.clone();
    if let Some(deps) = fresh
        .get_mut("dependencies")
        .and_then(|deps| deps.as_table_like_mut())
    {
        deps.remove(name);
    }
    add(&mut fresh)?;
    let rendered = crate::init::rust_project::dependencies::render_for_path(cargo_path, &fresh.to_string())?;
    let generated: toml::Value = toml::from_str(&rendered)?;
    if generated.get("dependencies").and_then(|deps| deps.get(name)) == Some(current) {
        if let Some(deps) = cargo
            .get_mut("dependencies")
            .and_then(|deps| deps.as_table_like_mut())
        {
            deps.remove(name);
        }
    } else {
        edit.note_kept_setting(cargo_path, &format!("dependency {name}"));
    }
    Ok(())
}

/// Whether the crate at `root` uses dependency `name` other than through
/// `excluded` paths: from a `[features]` entry, or in Rust source.
pub(crate) fn dependency_in_use(
    root: &Path,
    cargo: &DocumentMut,
    name: &str,
    excluded: &[PathBuf],
) -> bool {
    let feature_uses = cargo
        .get("features")
        .and_then(|features| features.as_table_like())
        .is_some_and(|features| {
            features.iter().any(|(_, entries)| {
                entries.as_array().is_some_and(|entries| {
                    entries.iter().filter_map(|entry| entry.as_str()).any(|entry| {
                        entry == format!("dep:{name}")
                            || entry.starts_with(&format!("{name}/"))
                            || entry.starts_with(&format!("{name}?/"))
                    })
                })
            })
        });
    let ident = name.replace('-', "_");
    fn sources_use(dir: &Path, ident: &str, excluded: &[PathBuf]) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if excluded.iter().any(|excluded| path.starts_with(excluded))
                || file_name.starts_with('.')
                || file_name == "target"
                || file_name == "node_modules"
            {
                return false;
            }
            if path.is_dir() {
                sources_use(&path, ident, excluded)
            } else {
                path.extension().is_some_and(|extension| extension == "rs")
                    && std::fs::read_to_string(&path).is_ok_and(|text| text.contains(ident))
            }
        })
    }
    feature_uses || sources_use(root, &ident, excluded)
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
fn render_module_with_layout(features: &[String], native_router: bool) -> Result<String> {
    let mut modules = Vec::new();
    let mut arms = Vec::new();
    let mut known = Vec::new();
    let mut factories = Vec::new();
    let mut clients = Vec::new();
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
        clients.push(format!("{module}::{ty}"));
        if feature == "sms" {
            factories.push("pub fn sms_client() -> BlueprintResult<SmsClient> {\n    if ![\"PLATFORM_GATEWAY_URL\", \"INSTANCE_ID\", \"INSTANCE_HMAC_KEY\"].iter().all(|name| env::var(name).is_ok_and(|value| !value.is_empty())) {\n        return Ok(SmsClient::deferred());\n    }\n    Ok(SmsClient::new(instance_gateway()?))\n}".into());
        } else {
            factories.push(format!("pub fn {feature}_client() -> BlueprintResult<{ty}> {{ Ok({ty}::new(instance_gateway()?)) }}"));
        }
        if feature == "payments" {
            factories.push("pub fn stripe_client() -> BlueprintResult<StripeBillingClient> { Ok(StripeBillingClient::with_gateway(instance_gateway()?)) }".into());
        }
    }
    let mut imports = String::new();
    match clients.as_slice() {
        [] => {}
        [client] => imports.push_str(&format!("use forklaunch_core::http::{client};\n")),
        clients => imports.push_str(&format!("use forklaunch_core::http::{{{}}};\n", clients.join(", "))),
    }
    if features.iter().any(|feature| feature == "payments") {
        imports.push_str("use forklaunch_billing_stripe::client::StripeBillingClient;\n");
    }
    Ok((if native_router { NATIVE_MODULE } else { MODULE })
        .replace("__EVENT_MODULES__", &modules.join("\n"))
        .replace("__HANDLER_ARMS__", &arms.join("\n"))
        .replace("__KNOWN_FEATURES__", &known.join(" | "))
        .replace("__CLIENT_IMPORTS__", &imports)
        .replace("__CLIENT_FACTORIES__", &factories.join("\n")))
}

// Built-in modules compile api/contracts/registry.rs both in their build host
// and at runtime. Keep this declaration independent of the runtime adapter.
fn builtin_capability_contract() -> String {
    CONTRACT
        .replace("pub occurred_at: Option<String>", "pub occurred_at: String")
        .replace("pub data: Option<JsonValue>", "pub data: JsonValue")
        .replace("BodyContract, JsonValue};", "BodyContract, BodyKind, JsonValue, SchemaRef};")
        .replace("    route.options.mcp = false;", "    for status in [401, 404, 500, 503] {\n        route = route.with_response(status, Some(BodyContract { kind: BodyKind::Text, schema: SchemaRef::named(\"String\"), content_type: Some(\"text/plain\".into()) }));\n    }\n    route.options.mcp = false;")
}

fn builtin_capability_module(source: &str) -> Result<String> {
    let start = source
        .find("fn text_body()")
        .context("Managed callback declaration is missing")?;
    let end = source
        .find("pub trait ManagedCapabilitiesRouterExt")
        .context("Managed callback adapter is missing")?;
    let mut source = format!(
        "{}pub use crate::api::contracts::registry::platform_events::{{contracts, register_contracts}};\n\n{}",
        &source[..start],
        &source[end..]
    );
    source = source
        .replace("use forklaunch_core::contract::{BodyContract, ContractRegistry, FieldContract, RouteContract, BodyKind, SchemaRef};\n", "")
        .replace("use forklaunch_core::contract::AccessLevel;\n", "");
    Ok(source)
}

fn builtin_capability_export(source: &str) -> Result<bool> {
    let file = syn::parse_file(source).context("Cannot parse built-in contract registry")?;
    for item in &file.items {
        if let syn::Item::Mod(module) = item {
            if module.ident == "platform_events" {
                let expected_path = module.attrs.iter().any(|attr| match &attr.meta {
                    syn::Meta::NameValue(value) if value.path.is_ident("path") => matches!(&value.value, syn::Expr::Lit(value) if matches!(&value.lit, syn::Lit::Str(path) if path.value() == "platform_events.rs")),
                    _ => false,
                });
                anyhow::ensure!(
                    expected_path
                        && module.content.is_none()
                        && matches!(module.vis, syn::Visibility::Public(_)),
                    "Built-in platform_events module must publicly declare the sibling platform_events.rs contract"
                );
                return Ok(true);
            }
        }
    }
    Ok(false)
}

const BUILTIN_REGISTRY_MARKER: &str = "// ForkLaunch managed platform event contract.\n";

fn builtin_capability_registry(source: &str, adding: bool, remove_export: bool) -> Result<String> {
    use syn::spanned::Spanned;
    fn registry_value(expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Path(path) => path.path.is_ident("registry"),
            syn::Expr::Call(call) => {
                matches!(&*call.func, syn::Expr::Path(path) if path.path.is_ident("Ok"))
                    && call.args.len() == 1
                    && registry_value(&call.args[0])
            }
            syn::Expr::Return(value) => value.expr.as_deref().is_some_and(registry_value),
            _ => false,
        }
    }
    fn mutable_registry(pat: &syn::Pat) -> bool {
        match pat {
            syn::Pat::Ident(ident) => ident.ident == "registry" && ident.mutability.is_some(),
            syn::Pat::Type(ty) => mutable_registry(&ty.pat),
            _ => false,
        }
    }
    fn hook(stmt: &syn::Stmt) -> bool {
        let syn::Stmt::Expr(syn::Expr::Call(call), _) = stmt else {
            return false;
        };
        matches!(&*call.func, syn::Expr::Path(path) if path.path.segments.len() == 2 && path.path.segments[0].ident == "platform_events" && path.path.segments[1].ident == "register_contracts")
            && call.args.len() == 1
            && matches!(&call.args[0], syn::Expr::Reference(reference) if reference.mutability.is_some() && matches!(&*reference.expr, syn::Expr::Path(path) if path.path.is_ident("registry")))
    }
    let exported = builtin_capability_export(source)?;
    let file = syn::parse_file(source)?;
    let mut edits = Vec::new();
    let mut builders = 0;
    for item in &file.items {
        if let syn::Item::Mod(module) = item
            && remove_export
            && module.ident == "platform_events"
        {
            // With the line break add wrote after it.
            let mut range = module.span().byte_range();
            if source[range.end..].starts_with('\n') {
                range.end += 1;
            }
            edits.push((range, String::new()));
        }
        let syn::Item::Fn(function) = item else {
            continue;
        };
        if !function
            .sig
            .ident
            .to_string()
            .starts_with("contract_registry")
        {
            continue;
        }
        let stmts = &function.block.stmts;
        if adding {
            if !stmts
                .iter()
                .any(|stmt| matches!(stmt, syn::Stmt::Local(local) if mutable_registry(&local.pat)))
            {
                continue;
            }
            let tail = stmts
                .last()
                .context("Built-in contract registry builder is empty")?;
            anyhow::ensure!(
                matches!(tail, syn::Stmt::Expr(expr, _) if registry_value(expr)),
                "Built-in contract registry builder must return registry or Ok(registry)"
            );
            builders += 1;
            if !stmts.iter().any(hook) {
                let at = tail.span().byte_range().start;
                let line = source[..at].rfind('\n').map_or(0, |n| n + 1);
                let indent = &source[line..at];
                let indent = if indent.trim().is_empty() {
                    indent
                } else {
                    "    "
                };
                edits.push((at..at, format!("{BUILTIN_REGISTRY_MARKER}{indent}platform_events::register_contracts(&mut registry);\n{indent}")));
            }
        } else {
            for statement in stmts.iter().filter(|stmt| hook(stmt)) {
                let range = statement.span().byte_range();
                // Remove only our marked hook, preserving any app-owned use of
                // the declaration.
                let prefix = &source[..range.start];
                if let Some(start) = prefix.rfind(BUILTIN_REGISTRY_MARKER)
                    && prefix[start + BUILTIN_REGISTRY_MARKER.len()..]
                        .trim()
                        .is_empty()
                {
                    // With the separator add wrote before the return.
                    let end = range.end
                        + source[range.end..]
                            .find(|c: char| !c.is_whitespace())
                            .unwrap_or(source.len() - range.end);
                    edits.push((start..end, String::new()));
                }
            }
        }
    }
    if adding {
        anyhow::ensure!(
            builders > 0,
            "Built-in module must expose a mutable contract registry builder returning registry or Ok(registry)"
        );
        if !exported {
            let at = file
                .items
                .first()
                .map(|item| item.span().byte_range().start)
                .unwrap_or(source.len());
            edits.push((at..at, BUILTIN_CONTRACT_EXPORT.into()));
        }
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut output = source.to_owned();
    for (range, replacement) in edits {
        output.replace_range(range, &replacement);
    }
    syn::parse_file(&output)
        .context("Managed built-in contract registry edit produced invalid Rust")?;
    Ok(output)
}

fn inline_capability_module(source: &str) -> String {
    source.replace("pub use contracts::platform_events::{contracts, register_contracts};", "pub use contracts::platform_events::contract;")
        .replace("use forklaunch_axum::native::{ContractRouter, EndpointError, EndpointPolicy, PolicyCapabilities};", "use forklaunch_axum::native::{ContractRouter, EndpointError, EndpointPolicy, PolicyCapabilities, State};")
        .replace("use forklaunch_core::contract::RouteContract;", "use forklaunch_core::contract::{RouteContract, bindings::OriginalBodySignedPolicy};")
        .replace("impl EndpointPolicy<AppState> for CallbackPolicy {", "impl OriginalBodySignedPolicy for CallbackPolicy {}\nimpl EndpointPolicy<AppState> for CallbackPolicy {")
        .replace("self.bind_authorized(contracts::platform_events::endpoint(), CallbackPolicy(verifier),", "contract::Bindings { receive_platform_event: contract::ReceivePlatformEvent::bind_authorized(CallbackPolicy(verifier),")
        .replace("|_, context, _body|", "|_: State<AppState>, context: CallbackContext, _body: contract::PlatformEvent|")
        .replace("        })\n    }\n}", "        }) }.mount(self)\n    }\n}")
}

const INLINE_CONTRACT: &str =
    "forklaunch_core::contract::controller_contract!(platform_events, \"../../api/controllers/platform_events.rs\");";

/// Add or remove the platform-events contract in a private contract host,
/// as text: the rest of the file keeps its bytes, so removal returns the
/// host as it was before add.
fn inline_capability_registry(source: &str, adding: bool) -> Result<String> {
    use syn::{spanned::Spanned, visit::Visit};
    struct CapabilityReference(bool);
    impl<'ast> Visit<'ast> for CapabilityReference {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            self.0 |= path
                .segments
                .first()
                .is_some_and(|segment| segment.ident == "platform_events");
            syn::visit::visit_path(self, path);
        }
    }
    let references = |statement: &syn::Stmt| {
        let mut found = CapabilityReference(false);
        found.visit_stmt(statement);
        found.0
    };
    let file = syn::parse_file(source)?;
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let contracts: Vec<_> = file
        .items
        .iter()
        .filter(|item| matches!(item, syn::Item::Macro(item) if item.mac.path.segments.last().is_some_and(|s| s.ident == "controller_contract") && item.mac.tokens.to_string().starts_with("platform_events ,")))
        .collect();
    if adding {
        if contracts.is_empty() {
            let addition = if source.is_empty() || source.ends_with('\n') {
                format!("{INLINE_CONTRACT}\n")
            } else {
                format!("\n{INLINE_CONTRACT}")
            };
            edits.push((source.len()..source.len(), addition));
        }
    } else {
        for item in contracts {
            let mut range = item.span().byte_range();
            if source[range.end..].starts_with('\n') {
                range.end += 1;
            } else if source[..range.start].ends_with('\n') {
                range.start -= 1;
            }
            edits.push((range, String::new()));
        }
    }
    for (name, hook) in [
        (
            "routes",
            "routes.extend(platform_events::contract::registry().routes().iter().cloned());",
        ),
        (
            "binding_manifest",
            "declarations.extend(platform_events::contract::binding_manifest()?);",
        ),
    ] {
        let function = file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Fn(item) if item.sig.ident == name => Some(item),
                _ => None,
            })
            .context("Private contract host is missing its collection function")?;
        let statements = &function.block.stmts;
        if adding {
            if statements.iter().any(references) {
                continue;
            }
            // Before the final return expression, separated from it like the
            // statement before it.
            let tail = statements.last().context("Contract collection is empty")?;
            let at = tail.span().byte_range().start;
            let gap_start = statements
                .len()
                .checked_sub(2)
                .map(|index| statements[index].span().byte_range().end)
                .unwrap_or_else(|| function.block.brace_token.span.open().byte_range().end);
            let gap = &source[gap_start..at];
            let separator = if gap.trim().is_empty() && !gap.is_empty() { gap } else { "\n" };
            edits.push((at..at, format!("{hook}{separator}")));
        } else {
            for statement in statements.iter().filter(|statement| references(statement)) {
                let range = statement.span().byte_range();
                let end = range.end
                    + source[range.end..]
                        .find(|c: char| !c.is_whitespace())
                        .unwrap_or(source.len() - range.end);
                edits.push((range.start..end, String::new()));
            }
        }
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut output = source.to_owned();
    for (range, replacement) in edits {
        output.replace_range(range, &replacement);
    }
    syn::parse_file(&output).context("Managed contract host edit produced invalid Rust")?;
    Ok(output)
}

fn with_native_capability_router(source: &str) -> Result<String> {
    anyhow::ensure!(
        source.matches(".finish()").count() == 1,
        "Native capability wiring requires one contract router finish"
    );
    Ok(source.replacen(".finish()", ".with_managed_capabilities()?.finish()", 1))
}

fn with_capability_router(main: &str) -> Result<String> {
    let markers = [
        ".into_router_with_default_mcp()",
        ".serve_with_default_mcp(",
    ];
    if markers
        .iter()
        .map(|marker| main.matches(marker).count())
        .sum::<usize>()
        != 1
    {
        bail!("Native capability wiring requires one generated HTTP/MCP router mount");
    }
    let marker = markers
        .into_iter()
        .find(|marker| main.contains(marker))
        .context("Expected generated HTTP/MCP router mount")?;
    Ok(main.replacen(marker, &format!(".with_managed_capabilities(){marker}"), 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn billing_registry_fixture() -> String {
        #[derive(ramhorns::Content)]
        struct Context {
            name: &'static str,
            stripe: bool,
        }
        ramhorns::Template::new(include_str!("../../templates/rust/billing/api/contracts/registry.rs"))
            .unwrap()
            .render(&Context {
                name: "billing",
                stripe: true,
            })
    }

    #[test]
    fn builtin_capability_registry_handles_profile_builders_without_runtime_imports() {
        for source in [
            billing_registry_fixture(),
            include_str!("../../templates/rust/iam/api/contracts/registry.rs").replace("{{{name}}}", "identity"),
            include_str!("../../templates/rust/cac/lib.rs").replace("{{{name}}}", "codes"),
        ] {
            let source = format!(
                "// App-owned registry note: keep this formatting.\n{source}\nconst USER_NOTE: &str = \"registry {{ text }}\";\n"
            );
            let added = builtin_capability_registry(&source, true, false).unwrap();
            assert!(added.starts_with("// App-owned registry note: keep this formatting.\n"));
            assert!(added.ends_with("const USER_NOTE: &str = \"registry { text }\";\n"));
            assert!(added.contains(BUILTIN_CONTRACT_EXPORT));
            assert!(!added.contains("crate::capabilities"));
            assert_eq!(
                builtin_capability_registry(&added, true, false).unwrap(),
                added
            );
            let expected = if source.contains("contract_registry_with_profiles") {
                2
            } else {
                1
            };
            assert_eq!(added.matches(BUILTIN_REGISTRY_MARKER).count(), expected);
            let removed = builtin_capability_registry(&added, false, true).unwrap();
            assert!(!removed.contains("platform_events"));
            assert!(removed.contains("// App-owned registry note: keep this formatting."));
            assert!(removed.contains("const USER_NOTE: &str = \"registry { text }\";"));
        }
        assert!(
            builtin_capability_registry(
                "pub fn contract_registry() -> Registry { make_registry() }",
                true,
                false
            )
            .is_err()
        );
        assert!(builtin_capability_registry("pub mod platform_events;", true, false).is_err());
        let app_hook = format!(
            "{BUILTIN_CONTRACT_EXPORT}pub fn contract_registry() -> Registry {{ let mut registry = make_registry(); platform_events::register_contracts(&mut registry); registry }}"
        );
        assert_eq!(
            builtin_capability_registry(&app_hook, true, false).unwrap(),
            app_hook
        );
        assert_eq!(
            builtin_capability_registry(&app_hook, false, false).unwrap(),
            app_hook
        );
        let contract = builtin_capability_contract();
        syn::parse_file(&contract).unwrap();
        assert!(contract.contains("pub occurred_at: String"));
        assert!(contract.contains("[401, 404, 500, 503]"));
        assert!(!contract.contains("forklaunch_blueprint_core"));
        let runtime = builtin_capability_module(
            &render_module_with_layout(&["payments".into(), "email".into()], false).unwrap(),
        )
        .unwrap();
        syn::parse_file(&runtime).unwrap();
        assert!(runtime.contains("pub use crate::api::contracts::registry::platform_events::{contracts, register_contracts};"));
        assert!(!runtime.contains("pub fn contracts()"));
        assert!(runtime.contains(
            "return next.run(Request::from_parts(parts, Body::from(body))).await;"
        ));
        assert!(runtime.contains("nonce_store::store()"));
    }

    #[test]
    fn builtin_capabilities_share_build_contracts_and_preserve_owned_removal() {
        for authored_edits in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let service = root.path().join("src/billing");
            std::fs::create_dir_all(service.join("api/contracts")).unwrap();
            std::fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
            let cargo = "[package]\nname = 'billing'\nversion = '0.1.0'\n[dependencies]\nforklaunch-core = { version = '=0.1.0', features = ['openapi'] }\n[build-dependencies]\nforklaunch-core = { version = '=0.1.0', features = ['build'] }\n";
            let lib = "// App exports\npub mod api;\npub mod sdk;\n";
            let sdk = "pub use crate::api::contracts::registry::*;\n";
            let server = "pub fn server() { router.serve_with_default_mcp(listener, registry); }\n";
            let registry = billing_registry_fixture();
            for (path, text) in [
                ("Cargo.toml", cargo),
                ("lib.rs", lib),
                ("sdk.rs", sdk),
                ("server.rs", server),
                ("api/contracts/registry.rs", registry.as_str()),
            ] {
                std::fs::write(service.join(path), text).unwrap();
            }
            std::fs::write(
                root.path().join(".forklaunch/manifest.toml"),
                "app_name = 'test'\nprojects = []\n",
            )
            .unwrap();
            std::fs::write(root.path().join("docker-compose.yaml"), "services:\n  billing:\n    image: test-rust\n    environment:\n      PORT: '8000'\nvolumes: {}\nnetworks: {}\n").unwrap();
            let change = |name: &str, caps: &[&str], adding: bool| {
                let caps = caps
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect::<Vec<_>>();
                let mut edit = CapabilityEdit::new(
                    root.path(),
                    "test",
                    "billing",
                    &service,
                    caps.clone(),
                    caps,
                );
                edit.native = true;
                apply(&mut edit, super::super::find(name).unwrap(), adding).unwrap();
                edit.commit().unwrap();
            };
            let mut preview = CapabilityEdit::new(
                root.path(),
                "test",
                "billing",
                &service,
                vec!["email".into()],
                vec!["email".into()],
            );
            preview.native = true;
            apply(&mut preview, super::super::find("email").unwrap(), true).unwrap();
            assert!(!preview.changed_paths().is_empty());
            assert_eq!(
                std::fs::read_to_string(service.join("api/contracts/registry.rs")).unwrap(),
                registry
            );
            assert_eq!(
                std::fs::read_to_string(service.join("Cargo.toml")).unwrap(),
                cargo
            );
            assert!(!service.join("api/contracts/platform_events.rs").exists());
            change("email", &["email"], true);
            let installed =
                std::fs::read_to_string(service.join("api/contracts/registry.rs")).unwrap();
            assert_eq!(installed.matches(BUILTIN_REGISTRY_MARKER).count(), 2);
            assert_eq!(
                std::fs::read_to_string(service.join("sdk.rs")).unwrap(),
                sdk
            );
            let manifest: toml::Value =
                toml::from_str(&std::fs::read_to_string(service.join("Cargo.toml")).unwrap())
                    .unwrap();
            for scope in ["dependencies", "build-dependencies"] {
                assert_eq!(
                    manifest[scope]["forklaunch-core"]["version"].as_str(),
                    Some("=0.1.0")
                );
                assert!(
                    manifest[scope]["forklaunch-core"]["features"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|value| value.as_str() == Some("typed-schema"))
                );
            }
            if authored_edits {
                std::fs::write(service.join("api/contracts/registry.rs"), format!("{installed}\n// App-owned after infra add\npub const APP_MARKER: bool = true;\n")).unwrap();
                let modified = std::fs::read_to_string(service.join("Cargo.toml")).unwrap();
                // Use TOML editing to retain a developer feature even if formatting changes.
                let mut modified: DocumentMut = modified.parse().unwrap();
                modified["build-dependencies"]["forklaunch-core"]["features"]
                    .as_array_mut()
                    .unwrap()
                    .push("auth");
                std::fs::write(service.join("Cargo.toml"), modified.to_string()).unwrap();
            }
            change("sms", &["email", "sms"], true);
            change("email", &["sms"], false);
            assert!(service.join("api/contracts/platform_events.rs").exists());
            change("sms", &[], false);
            let removed =
                std::fs::read_to_string(service.join("api/contracts/registry.rs")).unwrap();
            if authored_edits {
                assert!(
                    removed.contains(
                        "// App-owned after infra add\npub const APP_MARKER: bool = true;"
                    )
                );
                assert!(!removed.contains("platform_events"));
                let manifest: toml::Value =
                    toml::from_str(&std::fs::read_to_string(service.join("Cargo.toml")).unwrap())
                        .unwrap();
                assert!(
                    manifest["build-dependencies"]["forklaunch-core"]["features"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|value| value.as_str() == Some("auth"))
                );
                assert_eq!(
                    manifest["dependencies"]["forklaunch-core"]["features"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
            } else {
                assert_eq!(removed, registry);
                assert_eq!(
                    std::fs::read_to_string(service.join("Cargo.toml")).unwrap(),
                    cargo
                );
            }
            assert_eq!(
                std::fs::read_to_string(service.join("server.rs")).unwrap(),
                server
            );
            assert_eq!(
                std::fs::read_to_string(service.join("lib.rs")).unwrap(),
                lib
            );
            assert_eq!(
                std::fs::read_to_string(service.join("sdk.rs")).unwrap(),
                sdk
            );
            assert!(!service.join("api/contracts/platform_events.rs").exists());
            assert!(!service.join("capabilities/mod.rs").exists());
        }
    }

    #[test]
    fn payments_dependency_follows_the_framework_selection() {
        let root = tempfile::tempdir().unwrap();
        let service = root.path().join("src/billing");
        std::fs::create_dir_all(service.join("api/contracts")).unwrap();
        std::fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
        let cargo = "[package]\nname = 'billing'\nversion = '0.1.0'\n[dependencies]\nforklaunch-core = { version = '=0.1.0', features = ['openapi'] }\n[build-dependencies]\nforklaunch-core = { version = '=0.1.0', features = ['build'] }\n";
        for (path, text) in [
            ("Cargo.toml", cargo.to_owned()),
            ("lib.rs", "pub mod api;\n".to_owned()),
            ("server.rs", "pub fn server() { router.serve_with_default_mcp(listener, registry); }\n".to_owned()),
            // A module that does not already depend on Stripe billing.
            ("api/contracts/registry.rs", include_str!("../../templates/rust/iam/api/contracts/registry.rs").to_owned()),
        ] {
            std::fs::write(service.join(path), text).unwrap();
        }
        std::fs::write(
            root.path().join(".forklaunch/manifest.toml"),
            "app_name = 'test'\nprojects = []\n\n[framework]\nversion = '0.1.3'\nblueprint_version = '0.1.1'\n",
        )
        .unwrap();
        std::fs::write(root.path().join("docker-compose.yaml"), "services:\n  billing:\n    image: test-rust\n    environment:\n      PORT: '8000'\nvolumes: {}\nnetworks: {}\n").unwrap();
        let change = |caps: &[&str], adding: bool| {
            let caps = caps.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>();
            let mut edit = CapabilityEdit::new(root.path(), "test", "billing", &service, caps.clone(), caps);
            edit.native = true;
            apply(&mut edit, super::super::find("payments").unwrap(), adding).unwrap();
            edit.commit().unwrap();
        };
        change(&["payments"], true);
        // Published blueprint crates, never a `.forklaunch/blueprint-rs` copy
        // that a relocated or containerized application cannot see.
        let manifest: toml::Value =
            toml::from_str(&std::fs::read_to_string(service.join("Cargo.toml")).unwrap()).unwrap();
        let stripe = &manifest["dependencies"]["forklaunch-billing-stripe"];
        assert_eq!(stripe["version"].as_str(), Some("=0.1.1"));
        assert!(stripe.get("path").is_none());
        change(&[], false);
        assert_eq!(std::fs::read_to_string(service.join("Cargo.toml")).unwrap(), cargo);
    }

    #[test]
    fn inline_capability_host_collects_routes_and_binding_evidence_once() {
        let source = "pub fn routes()->Vec<u8>{let mut routes=Vec::new();routes}\npub fn binding_manifest()->Result<Vec<u8>,String>{let mut declarations=Vec::new();Ok(declarations)}";
        let added = inline_capability_registry(source, true).unwrap();
        assert_eq!(added.matches("controller_contract!").count(), 1);
        assert!(added.contains("platform_events::contract::registry()"));
        assert!(added.contains("platform_events::contract::binding_manifest()?"));
        assert_eq!(inline_capability_registry(&added, true).unwrap(), added);
        let removed = inline_capability_registry(&added, false).unwrap();
        assert!(!removed.contains("platform_events"));
        syn::parse_file(&removed).unwrap();
        let module =
            inline_capability_module(&render_module_with_layout(&["email".into()], true).unwrap());
        assert!(module.contains("ReceivePlatformEvent::bind_authorized"));
        assert!(module.contains("OriginalBodySignedPolicy for CallbackPolicy"));
        syn::parse_file(&module).unwrap();
    }

    #[test]
    fn native_capabilities_stay_inside_the_shared_http_mcp_security_wrapper() {
        let source = include_str!("../../templates/rust/project/native-service/api/routes/mod.rs");
        let wired = with_native_capability_router(source).unwrap();
        assert!(wired.contains(".with_managed_capabilities()?.finish()"));
        assert_eq!(wired.replace(".with_managed_capabilities()?", ""), source);
        let module = render_module_with_layout(&["email".into()], true).unwrap();
        assert!(module.contains("bind_authorized"));
        assert!(module.contains("async fn authorize_body"));
        assert!(module.contains("nonce_store::store()"));
        assert!(CONTRACT.contains("AccessLevel::Internal"));
        assert!(!CONTRACT.contains("forklaunch_blueprint_core"));
    }

    #[test]
    fn capabilities_bind_every_generated_listener_entrypoint_and_legacy_mount() {
        for main in [
            include_str!("../../templates/rust/project/service/src/main.rs"),
            include_str!("../../templates/rust/project/module/server.rs"),
            include_str!("../../templates/rust/cac/main.rs"),
            include_str!("../../templates/rust/ecommerce/main.rs"),
            "router.into_router_with_default_mcp()?;",
        ] {
            let wired = with_capability_router(main).unwrap();
            assert_eq!(wired.matches(".with_managed_capabilities()").count(), 1);
            assert_eq!(wired.replace(".with_managed_capabilities()", ""), main);
        }
        assert!(with_capability_router("router.into_router()").is_err());
        assert!(
            with_capability_router(
                "a.into_router_with_default_mcp(); b.serve_with_default_mcp(x,y)"
            )
            .is_err()
        );
    }

    #[test]
    fn dependency_features_come_off_as_they_went_on() {
        for before in [
            "[dependencies]\nforklaunch-core = { version = '=0.1.0', features = ['openapi'] }\n",
            "[dependencies]\nforklaunch-core = \"=0.1.0\"\n",
            "[dependencies.forklaunch-core]\nfeatures = [\n    \"typed-schema\",\n    \"otlp\",\n]\nversion = \"=0.1.3\"\n",
        ] {
            let mut cargo: DocumentMut = before.parse().unwrap();
            ensure_dependency_feature(&mut cargo, "dependencies", "forklaunch-core", "instance-gateway").unwrap();
            assert_ne!(cargo.to_string(), before);
            release_dependency_feature(&mut cargo, "dependencies", "forklaunch-core", "instance-gateway").unwrap();
            assert_eq!(cargo.to_string(), before);
        }
        // A feature the developer added alongside stays.
        let mut cargo: DocumentMut = "[dependencies]\nforklaunch-core = \"=0.1.0\"\n".parse().unwrap();
        ensure_dependency_feature(&mut cargo, "dependencies", "forklaunch-core", "instance-gateway").unwrap();
        ensure_dependency_feature(&mut cargo, "dependencies", "forklaunch-core", "stripe-webhook").unwrap();
        release_dependency_feature(&mut cargo, "dependencies", "forklaunch-core", "instance-gateway").unwrap();
        let features = cargo["dependencies"]["forklaunch-core"]["features"].as_array().unwrap();
        assert_eq!(features.iter().filter_map(|value| value.as_str()).collect::<Vec<_>>(), ["stripe-webhook"]);
    }

    #[test]
    fn dependencies_are_released_only_in_their_generated_form() {
        let root = tempfile::tempdir().unwrap();
        let cargo_path = root.path().join("Cargo.toml");
        let before = "[package]\nname = 'billing'\n[dependencies]\nuser = \"1\"\n";
        let add = |cargo: &mut DocumentMut| ensure_dependency(cargo, STRIPE, "../stripe", None);
        let mut edit = CapabilityEdit::new(root.path(), "test", "billing", root.path(), vec![], vec![]);
        let mut cargo: DocumentMut = before.parse().unwrap();
        add(&mut cargo).unwrap();
        release_dependency(&mut edit, &cargo_path, &mut cargo, STRIPE, add).unwrap();
        assert_eq!(cargo.to_string(), before);
        // A changed specification is the developer's.
        let mut cargo: DocumentMut = format!("{before}{STRIPE} = {{ path = \"../stripe\", features = [\"postgres-identity\"] }}\n").parse().unwrap();
        let edited = cargo.to_string();
        release_dependency(&mut edit, &cargo_path, &mut cargo, STRIPE, add).unwrap();
        assert_eq!(cargo.to_string(), edited);
        // As is one other code still uses.
        let cargo: DocumentMut = format!("{before}[features]\nbound = [\"{STRIPE}/postgres-identity\"]\n").parse().unwrap();
        assert!(dependency_in_use(root.path(), &cargo, STRIPE, &[]));
        let cargo: DocumentMut = before.parse().unwrap();
        assert!(!dependency_in_use(root.path(), &cargo, STRIPE, &[]));
        std::fs::create_dir_all(root.path().join("capabilities")).unwrap();
        std::fs::write(root.path().join("capabilities/mod.rs"), "use forklaunch_billing_stripe::client;\n").unwrap();
        assert!(!dependency_in_use(root.path(), &cargo, STRIPE, &[root.path().join("capabilities")]));
        std::fs::write(root.path().join("billing.rs"), "use forklaunch_billing_stripe::client;\n").unwrap();
        assert!(dependency_in_use(root.path(), &cargo, STRIPE, &[root.path().join("capabilities")]));
    }

    #[test]
    fn inline_capability_host_round_trips_its_bytes() {
        for source in [
            "//! Generated private compiler host.\n#[path=\"../../domain/schemas/mod.rs\"]\npub mod schemas;\nforklaunch_core::contract::controller_contract!(health, \"../../api/controllers/health.rs\");\n\npub fn routes()->Vec<u8>{let mut routes=Vec::new();routes.extend(health::contract::registry().routes().iter().cloned());\nroutes}\npub fn binding_manifest()->Result<Vec<u8>,String>{let mut declarations=Vec::new();declarations.extend(health::contract::binding_manifest()?);\nOk(declarations)}",
            "pub fn routes() -> Vec<u8> {\n    let mut routes = Vec::new();\n    routes\n}\npub fn binding_manifest() -> Result<Vec<u8>, String> {\n    let mut declarations = Vec::new();\n    Ok(declarations)\n}\n",
        ] {
            let added = inline_capability_registry(source, true).unwrap();
            assert!(added.contains("platform_events::contract::registry()"));
            assert_eq!(inline_capability_registry(&added, true).unwrap(), added);
            assert_eq!(inline_capability_registry(&added, false).unwrap(), source);
        }
    }

    #[test]
    fn deletion_reads_a_manifest_document_and_releases_only_the_last_gateway() {
        for (another_service, edited_mock) in [(false, false), (true, false), (false, true)] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
            let manifest = if another_service {
                "app_description = \"Document parsing regression\"\n[[projects]]\nname = \"other\"\n[projects.resources]\ncapabilities = [\"sms\"]\n"
            } else {
                "app_description = \"Document parsing regression\"\nprojects = []\n"
            };
            std::fs::write(root.path().join(".forklaunch/manifest.toml"), manifest).unwrap();
            let mut compose: DockerCompose = serde_yml::from_str("volumes: {}\nnetworks: {}\nservices:\n  removed:\n    environment:\n      INSTANCE_ID: local-instance\n  other:\n    environment:\n      SERVICE_PORT: '8123'\n").unwrap();
            let mut mock = generated_gateway_mock(None);
            if edited_mock {
                mock.image = Some("human/mock:fixture".into());
            }
            let env = mock.environment.as_mut().unwrap();
            env.insert("MOCK_EVENTS_URL_EMAIL".into(), "http://removed:8000".into());
            env.insert("MOCK_EVENTS_URL_SMS".into(), "http://other:8123".into());
            compose.services.insert(GATEWAY_MOCK_SERVICE.into(), mock);
            std::fs::write(root.path().join("docker-compose.yaml"), serde_yml::to_string(&compose).unwrap()).unwrap();
            cleanup_deleted_native_service(root.path(), "removed", &["email".into()]).unwrap();
            let compose: DockerCompose = serde_yml::from_str(
                &std::fs::read_to_string(root.path().join("docker-compose.yaml")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                compose.services.contains_key(GATEWAY_MOCK_SERVICE),
                another_service || edited_mock
            );
            if another_service || edited_mock {
                let env = compose.services[GATEWAY_MOCK_SERVICE]
                    .environment
                    .as_ref()
                    .unwrap();
                assert!(!env.contains_key("MOCK_EVENTS_URL_EMAIL"));
            }
            if another_service {
                let env = compose.services[GATEWAY_MOCK_SERVICE].environment.as_ref().unwrap();
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
