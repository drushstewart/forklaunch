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
        for source in [
            &mut ownership.original_registry,
            &mut ownership.last_registry,
        ]
        .into_iter()
        .flatten()
        {
            *source = source.replace(&format!("\"{old}\""), &format!("\"{new}\""));
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
    #[serde(default)]
    original_contract: Option<String>,
    #[serde(default)]
    last_contract: Option<String>,
    #[serde(default)]
    original_registry: Option<String>,
    #[serde(default)]
    last_registry: Option<String>,
    #[serde(default)]
    registry_modified: bool,
    #[serde(default)]
    contract_export_added: bool,
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
    // (CapabilityEdit stores them in the gitignored .forklaunch/local-state.json.)
    let state_path = edit
        .app_root
        .join(".forklaunch/native-capabilities")
        .join(format!("{}.json", edit.service_name));
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
    let current_contract = if shared_contract {
        edit.read(&contract_path)?
    } else {
        None
    };
    if !edit.service_capabilities.is_empty() && edit.read(&nonce_store_path)?.is_none() {
        edit.write(nonce_store_path.clone(), NONCE_STORE.into());
    }
    let env_path = edit.service_path.join(".env.local");
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
    let registry_has_export = if builtin_contract {
        builtin_capability_export(&registry_source)?
    } else {
        registry_source.contains(CONTRACT_EXPORT)
    };
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
            original_contract: if recovered {
                current_contract
                    .as_deref()
                    .map(|source| replace_owned_block(source, ""))
                    .transpose()?
                    .filter(|source| !source.trim().is_empty())
            } else {
                current_contract.clone()
            },
            last_contract: current_contract.clone(),
            original_registry: if (inline_contract || builtin_contract) && !recovered {
                Some(registry_source.clone())
            } else {
                None
            },
            last_registry: if inline_contract || builtin_contract {
                Some(registry_source.clone())
            } else {
                None
            },
            registry_modified: false,
            contract_export_added: recovered || !registry_has_export,
            original_module,
            last_module: current_module.clone(),
            module_modified: false,
            original_env: current_env.clone(),
            last_env: current_env.clone(),
            env_modified: false,
            lib_export_added: recovered || !lib.contains(LIB_EXPORT),
            main_import_added: recovered || !main.contains(&import),
            registry_added: recovered || !registry_source.contains(registry_hook),
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
    state.registry_modified |= (inline_contract || builtin_contract)
        && state
            .last_registry
            .as_deref()
            .is_some_and(|last| last != registry_source);
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
                edit.write(
                    contract_path.clone(),
                    if builtin_contract {
                        builtin_capability_contract()
                    } else if inline_contract {
                        CONTROLLER.into()
                    } else {
                        CONTRACT.into()
                    },
                );
            }
            if state.last_contract.is_none() {
                state.last_contract = edit.read(&contract_path)?;
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
        // Platform clients live in the framework, as in `@forklaunch/core/http`.
        ensure_dependency_feature(&mut cargo, "dependencies", "forklaunch-core", "instance-gateway")?;
        if capability.id == "payments" {
            ensure_dependency(
                &mut cargo,
                "forklaunch-billing-stripe",
                &format!(
                    "{prefix}/.forklaunch/blueprint-rs/crates/implementations/billing/billing-stripe"
                ),
                None,
            )?;
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
        if edit.read(&nonce_store_path)?.as_deref() == Some(NONCE_STORE) {
            edit.delete(nonce_store_path);
        }
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
                .read(&source_root.join("capabilities/mod.rs"))?
                .is_none()
        {
            lib = lib.replace(LIB_EXPORT, "");
        }
        if registry_path == lib_path {
            registry_source = lib.clone();
        }
        if inline_contract || builtin_contract {
            registry_source = if !state.registry_modified
                && state.last_registry.as_deref() == Some(registry_source.as_str())
                && state.original_registry.is_some()
            {
                state.original_registry.clone().expect("recorded registry")
            } else if builtin_contract {
                builtin_capability_registry(&registry_source, false, false)?
            } else {
                inline_capability_registry(&registry_source, false)?
            };
        } else if state.registry_added {
            registry_source = registry_source.replace(registry_hook, "");
        }
        if shared_contract {
            if current_contract == state.last_contract {
                if let Some(original) = &state.original_contract {
                    edit.write(contract_path.clone(), original.clone());
                } else {
                    edit.delete(contract_path.clone());
                }
            } else if let Some(current) = &current_contract {
                let kept = replace_owned_block(current, "")?;
                if kept.trim().is_empty() {
                    edit.delete(contract_path.clone());
                } else {
                    edit.write(contract_path.clone(), kept);
                }
            }
            if !inline_contract
                && state.contract_export_added
                && edit.read(&contract_path)?.is_none()
            {
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
        if state.main_import_added {
            main = main.replace(&import, "");
        }
        if state.router_added {
            main = main
                .replace(".with_managed_capabilities()?", "")
                .replace(".with_managed_capabilities()", "");
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
        edit.write(module_path, module.clone());
        let updated = cargo.to_string();
        edit.write(cargo_path, updated.clone());
        state.last_cargo = updated;
        state.last_module = Some(module);
        state.last_env = edit.read(&env_path)?;
        if inline_contract || builtin_contract {
            state.last_registry = Some(registry_source.clone());
        }
        edit.write(state_path, serde_json::to_string_pretty(&state)? + "\n");
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

const OWNED_DEPENDENCIES: &[(&str, &str, &str)] = &[
    (
        "dependencies",
        "forklaunch-blueprint-core",
        "forklaunch-blueprint-core",
    ),
    (
        "dependencies",
        "forklaunch-billing-stripe",
        "forklaunch-billing-stripe",
    ),
    ("dependencies", "forklaunch-core", "forklaunch-core"),
    (
        "build-dependencies",
        "forklaunch-core",
        "build-dependencies:forklaunch-core",
    ),
];

fn restore_dependencies(cargo: &mut DocumentMut, state: &Ownership) -> Result<()> {
    let before: DocumentMut = state.original_cargo.parse()?;
    let after: toml::Value = toml::from_str(&state.last_cargo)?;
    let current: toml::Value = toml::from_str(&cargo.to_string())?;
    for &(scope, name, key) in OWNED_DEPENDENCIES {
        let original = state
            .dependency_overrides
            .get(key)
            .map(|source| source.parse::<DocumentMut>())
            .transpose()?
            .unwrap_or_else(|| before.clone());
        if state.dependency_overrides.contains_key(key)
            || current.get(scope).and_then(|deps| deps.get(name))
                == after.get(scope).and_then(|deps| deps.get(name))
        {
            if let Some(original) = original.get(scope).and_then(|deps| deps.get(name)) {
                cargo[scope][name] = original.clone();
            } else if let Some(dependencies) = cargo
                .get_mut(scope)
                .and_then(|deps| deps.as_table_like_mut())
            {
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
    for &(scope, name, key) in OWNED_DEPENDENCIES {
        let current = current.get(scope).and_then(|deps| deps.get(name));
        let previous = last.get(scope).and_then(|deps| deps.get(name));
        if current != previous {
            state.dependency_overrides.insert(key.into(), source.into());
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
        if let syn::Item::Mod(module) = item {
            if remove_export && module.ident == "platform_events" {
                edits.push((module.span().byte_range(), String::new()));
            }
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
                // the declaration, even when the ignored ownership file is gone.
                let prefix = &source[..range.start];
                if let Some(start) = prefix.rfind(BUILTIN_REGISTRY_MARKER) {
                    if prefix[start + BUILTIN_REGISTRY_MARKER.len()..]
                        .trim()
                        .is_empty()
                    {
                        edits.push((start..range.end, String::new()));
                    }
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

fn inline_capability_registry(source: &str, adding: bool) -> Result<String> {
    use syn::visit::Visit;
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
    let mut file = syn::parse_file(source)?;
    file.items.retain(|item| !matches!(item, syn::Item::Macro(item) if item.mac.path.segments.last().is_some_and(|s| s.ident == "controller_contract") && item.mac.tokens.to_string().starts_with("platform_events ,")));
    if adding {
        file.items.push(syn::parse_str("forklaunch_core::contract::controller_contract!(platform_events, \"../../api/controllers/platform_events.rs\");")?);
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
            .iter_mut()
            .find_map(|item| match item {
                syn::Item::Fn(item) if item.sig.ident == name => Some(item),
                _ => None,
            })
            .context("Private contract host is missing its collection function")?;
        function.block.stmts.retain(|statement| {
            let mut found = CapabilityReference(false);
            found.visit_stmt(statement);
            !found.0
        });
        if adding {
            // Keep the collection initialization and its final return expression.
            let index = function
                .block
                .stmts
                .len()
                .checked_sub(1)
                .context("Contract collection is empty")?;
            function.block.stmts.insert(index, syn::parse_str(hook)?);
        }
    }
    Ok(prettyplease::unparse(&file))
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
        ramhorns::Template::new(include_str!("../../templates/rust/billing/lib.rs"))
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
            include_str!("../../templates/rust/iam/lib.rs").replace("{{{name}}}", "identity"),
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
            include_str!("../../templates/rust/iam/main.rs"),
            include_str!("../../templates/rust/cac/main.rs"),
            include_str!("../../templates/rust/billing/main.rs"),
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
