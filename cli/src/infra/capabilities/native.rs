//! Native capability edits retain application source and record dependency ownership.
use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml_edit::{Array, DocumentMut, InlineTable, Item, value};

use super::{
    Capability, CapabilityEdit, GATEWAY_MOCK_SERVICE, ensure_gateway_mock, local_gateway_env,
    release_gateway_mock,
};
use crate::core::docker::{DependsOn, DockerCompose, DockerService};
const MODULE: &str = include_str!("../../templates/rust/capabilities/mod.rs");
const HANDLER: &str = include_str!("../../templates/rust/capabilities/handler.rs");
const BEGIN: &str = "// BEGIN FORKLAUNCH MANAGED CAPABILITIES\n";
const END: &str = "// END FORKLAUNCH MANAGED CAPABILITIES\n";
const LIB_EXPORT: &str = "pub mod capabilities;\n";
const REGISTRY: &str = "    capabilities::register_contracts(&mut registry);\n";
#[derive(Serialize, Deserialize)]
struct Ownership {
    original_cargo: String,
    last_cargo: String,
    cargo_modified: bool,
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
    let mut state = if let Some(text) = edit.read(&state_path)? {
        serde_json::from_str::<Ownership>(&text)?
    } else {
        if !adding {
            bail!("Native capability ownership record is missing");
        }
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
            original_module: current_module.clone(),
            last_module: current_module.clone(),
            module_modified: false,
            original_env: current_env.clone(),
            last_env: current_env.clone(),
            env_modified: false,
            lib_export_added: !lib.contains(LIB_EXPORT),
            main_import_added: !main.contains(&import),
            registry_added: !lib.contains(REGISTRY),
            router_added: !main.contains(".with_managed_capabilities()"),
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
            original_mock: if edit.app_capabilities.len() == 1 {
                compose
                    .as_ref()
                    .and_then(|compose| compose.services.get(GATEWAY_MOCK_SERVICE))
                    .cloned()
            } else {
                None
            },
        }
    };
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
        edit.update_compose(|compose| {
            release_gateway_mock(compose, &service, feature, service_empty, app_empty);
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
                    for (key, value) in &state.original_service_env {
                        env.entry(key.clone()).or_insert_with(|| value.clone());
                    }
                    if let Some(dependency) = &state.original_gateway_dependency {
                        target
                            .depends_on
                            .get_or_insert_with(Default::default)
                            .entry(GATEWAY_MOCK_SERVICE.into())
                            .or_insert_with(|| dependency.clone());
                    }
                }
                if app_empty && !compose.services.contains_key(GATEWAY_MOCK_SERVICE) {
                    if let Some(mock) = &state.original_mock {
                        compose
                            .services
                            .insert(GATEWAY_MOCK_SERVICE.into(), mock.clone());
                    }
                }
            }
            Ok(())
        })?;
    }
    if edit.service_capabilities.is_empty() {
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
fn ensure_dependency(
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
    let after: DocumentMut = state.last_cargo.parse()?;
    for name in ["forklaunch-blueprint-core", "forklaunch-billing-stripe"] {
        if cargo["dependencies"].get(name).map(Item::to_string)
            == after["dependencies"].get(name).map(Item::to_string)
        {
            if let Some(original) = before["dependencies"].get(name) {
                cargo["dependencies"][name] = original.clone();
            } else if let Some(dependencies) = cargo["dependencies"].as_table_like_mut() {
                dependencies.remove(name);
            }
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
