//! Build explicit TypeScript contract modules without loading service bootstraps.
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command as ProcessCommand, thread, time::Duration};

use crate::core::forklaunch_state::{self, TYPESCRIPT_CONTRACTS};

use anyhow::{Context, Result, ensure};
use clap::{Arg, ArgAction, ArgMatches, Command};
use termcolor::{ColorChoice, StandardStream};

use crate::{CliCommand, core::command::command};

mod compiler_host;
#[cfg(test)]
mod compiler_host_tests;

pub(crate) const REGISTRY_COMPILER: &str = ".forklaunch/tools/contract-compiler";

pub(crate) const RUNNER: &str = include_str!("typescript_build/runner.mjs");
/// The runner before build plans moved into `build.json`; migration replaces it.
pub(crate) const PREVIOUS_RUNNER: &str = include_str!("typescript_build/runner_previous.mjs");
/// The runner for vendored framework compilers only.
pub(crate) const LEGACY_RUNNER: &str = include_str!("typescript_build/runner_legacy.mjs");

fn owned_runner(source: &str) -> bool {
    source == RUNNER || source == PREVIOUS_RUNNER || source == LEGACY_RUNNER
}

#[derive(Debug)]
pub(crate) struct BuildCommand;

impl CliCommand for BuildCommand {
    fn command(&self) -> Command {
        command(
            "build",
            "Compile TypeScript and emit both SDK languages from isolated contracts",
        )
        .arg(Arg::new("base_path").long("path").short('p'))
        .arg(Arg::new("service").long("service").required(true))
        .arg(Arg::new("watch").long("watch").action(ArgAction::SetTrue))
        .arg(
            Arg::new("install_hooks")
                .long("install-hooks")
                .action(ArgAction::SetTrue)
                .conflicts_with("watch"),
        )
        .arg(
            Arg::new("skip_compile")
                .long("skip-compile")
                .action(ArgAction::SetTrue),
        )
    }

    fn handler(&self, matches: &ArgMatches) -> Result<()> {
        let (root, manifest) = crate::core::validate::require_manifest(matches)?;
        let name = matches.get_one::<String>("service").unwrap();
        let project = manifest
            .projects
            .iter()
            .find(|project| project.name == *name)
            .context("Unknown TypeScript contract service")?;
        ensure!(
            project
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.language.as_deref())
                != Some("rust"),
            "Rust contracts are compiled through Cargo"
        );
        let service = crate::core::application_output::output_path(
            &root,
            &Path::new(&manifest.modules_path).join(name),
        )?;
        ensure!(
            service.join("api/contracts").is_dir(),
            "Service needs explicit api/contracts modules before enabling isolated SDK builds"
        );
        let watch = matches.get_flag("watch");
        let mut previous = None;
        loop {
            let current = fingerprint(&root.join(&manifest.modules_path))?;
            if previous.as_ref() != Some(&current) {
                previous = Some(current);
                let result = compile(
                    &root,
                    &service,
                    name,
                    &manifest.validator,
                    &manifest.runtime,
                    matches.get_flag("skip_compile"),
                );
                if !watch {
                    result?;
                    if matches.get_flag("install_hooks") {
                        install_hooks(
                            &root,
                            &service,
                            name,
                            &manifest.validator,
                            &manifest.runtime,
                            &manifest.modules_path,
                            false,
                        )?;
                    }
                    return Ok(());
                }
                if let Err(error) = result {
                    eprintln!("Contract build failed; last-good SDKs retained: {error:#}");
                }
            }
            thread::sleep(Duration::from_millis(300));
        }
    }
}

fn compile(
    root: &Path,
    service: &Path,
    name: &str,
    validator: &str,
    runtime: &str,
    skip_compile: bool,
) -> Result<()> {
    if !skip_compile {
        let status = ProcessCommand::new(if runtime == "bun" { "bun" } else { "pnpm" })
            .args(if runtime == "bun" {
                vec!["x", "--bun", "tsc", "-b"]
            } else {
                vec!["exec", "tsc", "-b"]
            })
            .current_dir(&service)
            .status()
            .context("Failed to start TypeScript compiler")?;
        ensure!(
            status.success(),
            "TypeScript compilation failed; SDKs were not regenerated"
        );
    }
    export(root, service, name, validator, runtime)?;
    super::generate::generate_or_check_sdk_outputs(
        root,
        "all",
        "contract",
        Some(name),
        false,
        &mut StandardStream::stdout(ColorChoice::Always),
    )
}

pub(crate) fn hook_script(root: &Path, service: &Path, name: &str) -> Result<String> {
    let runner = pathdiff::diff_paths(root.join(".forklaunch/tools/contract-build.mjs"), service)
        .context("Contract hook path")?;
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(root.join(".forklaunch/manifest.toml"))?)?;
    let runtime = manifest["runtime"].as_str().context("Missing TypeScript runtime")?;
    ensure!(matches!(runtime, "node" | "bun"), "Unsupported TypeScript runtime");
    Ok(format!(
        "{runtime} {} {}",
        serde_json::to_string(&runner.to_string_lossy())?,
        name
    ))
}

pub(crate) fn installed_compiler(
    root: &Path,
    service: &Path,
    name: &str,
    script: &str,
) -> Result<Option<String>> {
    if script != hook_script(root, service, name)? {
        return Ok(None);
    }
    ensure!(
        owned_runner(&fs::read_to_string(root.join(".forklaunch/tools/contract-build.mjs"))?),
        "Modified contract build runner requires a custom image"
    );
    let config = forklaunch_state::contract_plan(root, name)?
        .with_context(|| format!("Contract build plan {name} is missing from {}", forklaunch_state::BUILD))?;
    ensure!(
        config["name"] == name
            && config["service"].as_str() == service.strip_prefix(root)?.to_str(),
        "Contract build hook must match its service"
    );
    let compiler = config["compiler"]
        .as_str()
        .context("Missing contract compiler source")?;
    if compiler == REGISTRY_COMPILER {
        let runner = fs::read_to_string(root.join(".forklaunch/tools/contract-build.mjs"))?;
        // Before migration the runner still reads the per-service plan files.
        let legacy = root.join(forklaunch_state::legacy::CONTRACT_PLANS).is_dir();
        ensure!(runner == RUNNER || (legacy && runner == PREVIOUS_RUNNER), "Registry compiler requires the current generated runner");
        compiler_host::validate(root)?;
    } else {
        ensure!(
            matches!(compiler, "vendor/framework" | ".forklaunch/framework-rs")
                && root.join(compiler).join("Cargo.lock").is_file(),
            "Contract compiler requires a pinned source snapshot"
        );
    }
    Ok(Some(compiler.to_owned()))
}

/// Install the proven default profile; retain unsupported profiles explicitly.
pub(crate) fn prepare_default_service(
    root: &Path,
    name: &str,
    runtime: &str,
    _protected: bool,
) -> Result<()> {
    ensure!(matches!(runtime, "node" | "bun"), "Unsupported TypeScript runtime");
    prepare_new_service(root, name)
}

/// Scaffold consumers without inventing a portable contract before compilation.
pub(crate) fn prepare_new_service(root: &Path, name: &str) -> Result<()> {
    compiler_host::preflight(root)?;
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(root.join(".forklaunch/manifest.toml"))?)?;
    let modules = manifest["modules_path"]
        .as_str()
        .context("Modules path missing")?;
    let validator = manifest["validator"]
        .as_str()
        .context("Validator missing")?;
    let runtime = manifest["runtime"].as_str().context("Runtime missing")?;
    let service = root.join(modules).join(name);
    ensure!(
        service.join("api/contracts").is_dir(),
        "Explicit contract modules required for compile hooks"
    );
    if forklaunch_state::contract_plan(root, name)?.is_none() {
        record_local_ownership(root, &service, name)?;
    }
    super::generate::prepare_contract_consumers(root, name)?;
    install_hooks(root, &service, name, validator, runtime, modules, true)?;
    record_contract_path(
        root,
        name,
        contract_path(root, name)?,
    )
}

fn install_hooks(
    root: &Path,
    service: &Path,
    name: &str,
    validator: &str,
    runtime: &str,
    modules: &str,
    initial: bool,
) -> Result<()> {
    let package_path = service.join("package.json");
    let mut package: serde_json::Value = serde_json::from_slice(&fs::read(&package_path)?)?;
    let script = hook_script(root, service, name)?;
    let scripts = package["scripts"]
        .as_object_mut()
        .context("Service scripts missing")?;
    ensure!(
        scripts
            .get("build")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value == "tsc -b" || value == script),
        "Custom build script must be reconciled before installing the contract hook"
    );
    for (key, value) in [
        ("sdk:build", script.clone()),
        ("sdk:watch", format!("{script} --watch")),
    ] {
        ensure!(
            scripts
                .get(key)
                .is_none_or(|existing| existing.as_str() == Some(&value)),
            "Custom SDK script must be preserved"
        );
        scripts.insert(key.into(), value.into());
    }
    scripts.insert("build".into(), script.into());
    let compiler = compiler_host::plan(root)?;
    let contract_path = contract_path(root, name)?;
    let (request_multipart, response_multipart) = if initial {
        (false, false)
    } else {
        let compiled = forklaunch_core::build::http_artifact_sources(
            &fs::read_to_string(root.join(&contract_path))?,
            name,
        )?;
        (compiled.request_multipart, compiled.response_multipart)
    };
    let local = super::layout::typescript_authored(root, name)?;
    let ts = if local { service.strip_prefix(root)?.join("sdk/index.ts") } else { super::layout::source(root, &super::layout::typescript(root)?)?.join(name).join("index.ts") };
    let rust = if local { service.strip_prefix(root)?.join("sdk/rust/lib.rs") } else { super::layout::source(root, &super::layout::rust(root)?.join(format!("{name}-sdk")))?.join("lib.rs") };
    let config = serde_json::json!({
        "name": name, "service": service.strip_prefix(root)?, "validator":validator,
        "runtime":runtime, "modules": modules, "compiler":compiler.compiler,
        "contract":contract_path, "openapi":openapi_path(root, name)?,
        "typescript":ts, "rust":rust, "typescriptOwned":local,
        "requestMultipart":request_multipart, "responseMultipart":response_multipart
    });
    let runner = root.join(".forklaunch/tools/contract-build.mjs");
    ensure!(
        !runner.exists()
            || owned_runner(&fs::read_to_string(&runner)?),
        "Customized contract runner must be reconciled before installing hooks"
    );
    check_hook_config(root, name, &config)?;
    compiler.write()?;
    write_hook_config(root, name, &config)?;
    super::write_if_changed(root.join(forklaunch_state::CONTRACT_BUILD), RUNNER)?;
    super::write_if_changed(
        package_path,
        format!("{}\n", serde_json::to_string_pretty(&package)?),
    )?;
    crate::init::rust_project::enable_typescript_contract_image(root)?;
    if config["compiler"] == REGISTRY_COMPILER {
        crate::init::rust_project::refresh_mixed_images(root)?;
    }
    Ok(())
}

/// Plans live in `build.json#contracts.<name>`; their ownership receipts in
/// `generated.json#typescriptContracts.<name>`.
fn write_hook_config(root: &Path, name: &str, config: &serde_json::Value) -> Result<()> {
    check_hook_config(root, name, config)?;
    forklaunch_state::update_build(root, |plan| {
        let contracts = plan
            .entry("contracts")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("build.json contracts must be an object")?;
        contracts.insert(name.into(), config.clone());
        Ok(())
    })?;
    set_receipt(root, name, Some(forklaunch_state::json_sha256(config)))?;
    // Every build rewrites this SDK; formatting it would only fight freshness checks.
    if let Some(sdk) = config["rust"].as_str().and_then(|lib| Path::new(lib).parent()) {
        let rustfmt = root.join(sdk).join("rustfmt.toml");
        if !rustfmt.exists() {
            fs::create_dir_all(root.join(sdk))?;
            super::write_if_changed(rustfmt, GENERATED_RUSTFMT)?;
        }
    }
    Ok(())
}

fn receipt(root: &Path, name: &str) -> Result<Option<String>> {
    Ok(forklaunch_state::generated_section(root, TYPESCRIPT_CONTRACTS)?
        .and_then(|receipts| receipts.get(name)?.as_str().map(str::to_owned)))
}

fn set_receipt(root: &Path, name: &str, digest: Option<String>) -> Result<()> {
    forklaunch_state::update_generated(root, |sections| {
        let receipts = sections
            .entry(TYPESCRIPT_CONTRACTS)
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("generated.json typescriptContracts must be an object")?;
        match digest {
            Some(digest) => receipts.insert(name.into(), digest.into()),
            None => receipts.remove(name),
        };
        if receipts.is_empty() {
            sections.remove(TYPESCRIPT_CONTRACTS);
        }
        Ok(())
    })
}

fn remove_contract_plan(root: &Path, name: &str) -> Result<()> {
    forklaunch_state::update_build(root, |plan| {
        if let Some(contracts) = plan.get_mut("contracts").and_then(serde_json::Value::as_object_mut) {
            contracts.remove(name);
        }
        Ok(())
    })?;
    set_receipt(root, name, None)
}

/// Opts a build-owned Rust directory out of the application's `cargo fmt`.
pub(crate) const GENERATED_RUSTFMT: &str = "# Generated by ForkLaunch; regenerated on build.\ndisable_all_formatting = true\n";

fn check_hook_config(root: &Path, name: &str, next: &serde_json::Value) -> Result<()> {
    if let Some(existing) = forklaunch_state::contract_plan(root, name)? {
        ensure!(
            existing == *next
                || receipt(root, name)?.as_deref() == Some(&forklaunch_state::json_sha256(&existing)),
            "Customized contract hook configuration must be reconciled before updating it"
        );
    }
    Ok(())
}

/// Validate ownership before any project mutation, preserving custom hooks.
pub(crate) fn preflight_lifecycle(root: &Path, name: &str) -> Result<bool> {
    let Some(config) = owned_config(root, name)? else {
        return Ok(false);
    };
    let service = crate::core::application_output::output_path(
        root,
        Path::new(config["service"].as_str().context("Missing hook service")?),
    )?;
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(service.join("package.json"))?)?;
    let expected = hook_script(root, &service, name)?;
    ensure!(
        package["scripts"]["build"] == expected
            && package["scripts"]["sdk:build"] == expected
            && package["scripts"]["sdk:watch"] == format!("{expected} --watch"),
        "Customized contract build scripts must be reconciled before changing or deleting this project"
    );
    ensure!(
        installed_compiler(root, &service, name, &expected)?.is_some(),
        "Contract compiler source is unavailable"
    );
    Ok(true)
}

/// Stage descriptor changes only after every installed hook passes ownership checks.
/// No files are written here, so change-command dry runs and later admission failures
/// preserve the original application.
pub(crate) fn application_descriptor_updates(
    root: &Path,
    validator: Option<&str>,
    runtime: Option<&str>,
) -> Result<(Vec<crate::core::rendered_template::RenderedTemplate>, Vec<(String, String)>)> {
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(root.join(".forklaunch/manifest.toml"))?)?;
    let mut configs = Vec::new();
    let mut scripts = Vec::new();
    for project in manifest["projects"].as_array().context("Missing projects")? {
        let name = project["name"].as_str().context("Missing project name")?;
        if !preflight_lifecycle(root, name)? { continue; }
        let mut config = owned_config(root, name)?.context("Missing owned hook")?;
        if let Some(validator) = validator { config["validator"] = validator.into(); }
        if let Some(runtime) = runtime {
            ensure!(matches!(runtime, "node" | "bun"), "Unsupported TypeScript runtime");
            config["runtime"] = runtime.into();
            let service = root.join(config["service"].as_str().context("Missing service path")?);
            let old = hook_script(root, &service, name)?;
            let (_, arguments) = old.split_once(' ').context("Invalid owned hook script")?;
            scripts.push((name.to_owned(), format!("{runtime} {arguments}")));
        }
        configs.push((name.to_owned(), config));
    }
    if configs.is_empty() {
        return Ok((Vec::new(), scripts));
    }
    // Both consolidated files are rendered whole, including any plans and
    // receipts still in legacy files, so writing them completes migration.
    let updates = forklaunch_state::staged_contract_updates(root, |plan, generated| {
        let contracts = plan
            .entry("contracts")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("build.json contracts must be an object")?;
        let receipts = generated
            .entry(TYPESCRIPT_CONTRACTS)
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("generated.json typescriptContracts must be an object")?;
        for (name, config) in configs {
            receipts.insert(name.clone(), forklaunch_state::json_sha256(&config).into());
            contracts.insert(name, config);
        }
        Ok(())
    })?
    .into_iter()
    .map(|(path, content)| crate::core::rendered_template::RenderedTemplate { path, content, context: None })
    .collect();
    Ok((updates, scripts))
}

fn owned_config(root: &Path, name: &str) -> Result<Option<serde_json::Value>> {
    let Some(config) = forklaunch_state::contract_plan(root, name)? else {
        return Ok(None);
    };
    ensure!(
        receipt(root, name)?.as_deref() == Some(&forklaunch_state::json_sha256(&config)),
        "Contract hook configuration was customized or predates ownership tracking; reconcile it with `forklaunch sdk build --service {name} --install-hooks` before changing the project"
    );
    Ok(Some(config))
}

pub(crate) fn remove_hook(root: &Path, name: &str) -> Result<()> {
    let Some(config) = owned_config(root, name)? else { return Ok(()); };
    if config["typescriptOwned"].as_bool() == Some(true) {
        if let Some(path) = config["rust"].as_str().and_then(|path|Path::new(path).parent()) {
            crate::init::rust_project::remove_cargo_member(root, &path.to_string_lossy())?;
        }
    }
    remove_contract_plan(root, name)?;
    for path in [
        root.join(format!(".forklaunch/contracts/{name}/contract.json")),
        root.join(format!(".forklaunch/openapi/{name}/openapi.json")),
    ] {
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    super::generate::cleanup_service_generated_files(root, name)?;
    if config["typescriptOwned"].as_bool() == Some(true) { return Ok(()); }
    super::generate::refresh_contract_consumers(root)
}

pub(crate) fn rename_hook(root: &Path, old: &str, new: &str) -> Result<()> {
    if old == new {
        return Ok(());
    }
    let Some(mut config) = owned_config(root, old)? else {
        return Ok(());
    };
    ensure!(
        forklaunch_state::contract_plan(root, new)?.is_none(),
        "Destination contract hook already exists"
    );
    let modules = config["modules"]
        .as_str()
        .context("Missing hook modules path")?
        .to_owned();
    let service = root.join(&modules).join(new);
    let local = config["typescriptOwned"].as_bool() == Some(true);
    if local {
        record_local_ownership(root, &service, new)?;
        crate::init::rust_project::rename_service_sdk_dependency(root, &format!("{modules}/{old}"), &format!("{modules}/{new}"), old, new)?;
    }
    super::generate::prepare_contract_consumers_with_transports(
        root,
        new,
        config["requestMultipart"].as_bool().unwrap_or(false),
        config["responseMultipart"].as_bool().unwrap_or(false),
    )?;
    config["name"] = new.into();
    config["service"] = serde_json::to_value(service.strip_prefix(root)?)?;
    config["contract"] = contract_path(root, new)?.into();
    config["openapi"] = openapi_path(root, new)?.into();
    config["typescript"] = if local { serde_json::to_value(service.strip_prefix(root)?.join("sdk/index.ts"))? } else { serde_json::to_value(
        super::layout::source(root, &super::layout::typescript(root)?)?
            .join(new)
            .join("index.ts"),
    )? };
    config["rust"] = if local { serde_json::to_value(service.strip_prefix(root)?.join("sdk/rust/lib.rs"))? } else { serde_json::to_value(
        super::layout::source(root, &super::layout::rust(root)?.join(format!("{new}-sdk")))?
            .join("lib.rs"),
    )? };
    write_hook_config(root, new, &config)?;
    let package_path = service.join("package.json");
    let mut package: serde_json::Value = serde_json::from_slice(&fs::read(&package_path)?)?;
    let script = hook_script(root, &service, new)?;
    package["scripts"]["build"] = script.clone().into();
    package["scripts"]["sdk:build"] = script.clone().into();
    package["scripts"]["sdk:watch"] = format!("{script} --watch").into();
    super::write_if_changed(package_path, serde_json::to_string_pretty(&package)?)?;
    record_contract_path(
        root,
        new,
        contract_path(root, new)?,
    )?;
    remove_hook(root, old)
}

fn fingerprint(directory: &Path) -> Result<Vec<u8>> {
    fn visit(directory: &Path, digest: &mut Sha256) -> Result<()> {
        let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    name.as_ref(),
                    "node_modules"
                        | "dist"
                        | "lib"
                        | "target"
                        | "production"
                        | "client-sdk"
                        | ".git"
                        | ".forklaunch"
                ) {
                    visit(&entry.path(), digest)?;
                }
            } else if kind.is_file()
                && (name.ends_with(".ts") || name == "package.json" || name.starts_with("tsconfig"))
            {
                digest.update(entry.path().to_string_lossy().as_bytes());
                digest.update(fs::read(entry.path())?);
            }
        }
        Ok(())
    }
    let mut digest = Sha256::new();
    visit(directory, &mut digest)?;
    Ok(digest.finalize().to_vec())
}

fn contract_path(root: &Path, name: &str) -> Result<String> {
    Ok(if let Some(local) = super::layout::local_sdk(root, name, "typescript")? {
        local.join("contract.json").strip_prefix(root.canonicalize()?)?.to_string_lossy().into_owned()
    } else { format!(".forklaunch/contracts/{name}/contract.json") })
}
fn openapi_path(root: &Path, name: &str) -> Result<String> {
    Ok(if let Some(local) = super::layout::local_sdk(root, name, "typescript")? {
        local.join("openapi.json").strip_prefix(root.canonicalize()?)?.to_string_lossy().into_owned()
    } else { format!(".forklaunch/openapi/{name}/openapi.json") })
}
fn record_local_ownership(root: &Path, service: &Path, name: &str) -> Result<()> {
    let path = root.join(".forklaunch/manifest.toml");
    let mut manifest: toml::Value = toml::from_str(&fs::read_to_string(&path)?)?;
    let project = manifest["projects"].as_array_mut().context("Missing projects")?.iter_mut().find(|project|project["name"].as_str()==Some(name)).context("Missing service")?;
    let metadata = project.as_table_mut().context("Invalid project")?.entry("metadata").or_insert_with(||toml::Value::Table(Default::default())).as_table_mut().context("Invalid metadata")?;
    metadata.insert("sdkLayout".into(), "project-local-v1".into());
    metadata.insert("language".into(), "typescript".into());
    metadata.insert("staticPath".into(), service.strip_prefix(root)?.to_string_lossy().into_owned().into());
    metadata.insert("contractArtifactPath".into(), service.strip_prefix(root)?.join("sdk/contract.json").to_string_lossy().into_owned().into());
    metadata.insert("openapiPath".into(), service.strip_prefix(root)?.join("sdk/openapi.json").to_string_lossy().into_owned().into());
    super::write_if_changed(path, toml::to_string_pretty(&manifest)?)?;
    Ok(())
}

fn export(root: &Path, service: &Path, name: &str, validator: &str, runtime: &str) -> Result<()> {
    let document = forklaunch_core::build::export_typescript_openapi_with_runtime(service, validator, runtime)?;
    let openapi: serde_json::Value = serde_json::from_str(&document)?;
    // The source-distributed Linux build tool and installed CLI use this same
    // importer/compiler; both validate every target before publishing outputs.
    let compiled =
        forklaunch_core::build::openapi_artifact_sources(&serde_json::to_string(&openapi)?, name)?;
    let relative = contract_path(root, name)?;
    let mut contract: serde_json::Value = serde_json::from_str(&compiled.contract)?;
    if super::layout::typescript_authored(root, name)? {
        contract["sdkLayout"] = "project-local-v1".into();
            contract["rustClientName"] = "Client".into();
        contract["typescriptRuntimeLayout"] = "local".into();
    }
    super::write_if_changed(root.join(&relative), serde_json::to_string(&contract)?)?;
    super::write_if_changed(
        root.join(openapi_path(root, name)?),
        serde_json::to_string_pretty(&openapi)?,
    )?;
    record_contract_path(root, name, relative)
}

fn record_contract_path(root: &Path, name: &str, relative: String) -> Result<()> {
    let path = root.join(".forklaunch/manifest.toml");
    let mut manifest: toml::Value = toml::from_str(&fs::read_to_string(&path)?)?;
    let project = manifest["projects"]
        .as_array_mut()
        .context("Manifest projects missing")?
        .iter_mut()
        .find(|project| project["name"].as_str() == Some(name))
        .context("Contract project missing")?;
    let metadata = project
        .as_table_mut()
        .context("Contract project must be a table")?
        .entry("metadata")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .context("Contract metadata must be a table")?;
    metadata.insert("contractArtifactPath".into(), toml::Value::String(relative));
    super::write_if_changed(path, toml::to_string_pretty(&manifest)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_typescript_sdk_keeps_canonical_types_and_local_artifacts() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
        let service = root.path().join("src/iam");
        fs::create_dir_all(&service).unwrap();
        fs::write(service.join("sdk.ts"), "export type IamSdkClient = { canonical: true };\n").unwrap();
        fs::write(root.path().join(".forklaunch/manifest.toml"), "modules_path='src'\n[[projects]]\nname='iam'\ntype='Service'\n").unwrap();
        record_local_ownership(root.path(), &service, "iam").unwrap();
        assert_eq!(contract_path(root.path(), "iam").unwrap(), "src/iam/sdk/contract.json");
        assert_eq!(openapi_path(root.path(), "iam").unwrap(), "src/iam/sdk/openapi.json");
        assert!(super::super::layout::typescript_authored(root.path(), "iam").unwrap());
        assert_eq!(super::super::layout::local_sdk(root.path(), "iam", "rust").unwrap().unwrap(), root.path().canonicalize().unwrap().join("src/iam/sdk/rust"));
        assert_eq!(fs::read_to_string(service.join("sdk.ts")).unwrap(), "export type IamSdkClient = { canonical: true };\n");
    }

    #[test]
    fn hook_ownership_rejects_custom_configuration_before_updates() {
        let root = tempfile::tempdir().unwrap();
        let original = serde_json::json!({"name":"notes","requestMultipart":false});
        write_hook_config(root.path(), "notes", &original).unwrap();
        assert!(owned_config(root.path(), "notes").unwrap().is_some());
        // Reformatting build.json is not a customization.
        let path = root.path().join(forklaunch_state::BUILD);
        let plan: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        fs::write(&path, serde_json::to_string(&plan).unwrap()).unwrap();
        assert!(owned_config(root.path(), "notes").unwrap().is_some());
        let custom = "{\"contracts\":{\"notes\":{\"custom\":true}}}";
        fs::write(&path, custom).unwrap();
        assert!(owned_config(root.path(), "notes").is_err());
        assert!(write_hook_config(root.path(), "notes", &original).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), custom);
        assert!(!root.path().join(".forklaunch/typescript-build").exists());
    }

    #[test]
    fn watch_tracks_contract_dependencies_without_rebuilding_on_generated_outputs() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("api/domain/schemas")).unwrap();
        let schema = root.path().join("api/domain/schemas/note.ts");
        fs::write(&schema, "export const size = 1;").unwrap();
        let original = fingerprint(root.path()).unwrap();
        for folder in ["api/dist", "api/production", "client-sdk"] {
            fs::create_dir_all(root.path().join(folder)).unwrap();
            fs::write(root.path().join(folder).join("index.ts"), "generated").unwrap();
        }
        assert_eq!(original, fingerprint(root.path()).unwrap());
        fs::write(schema, "export const size = 2;").unwrap();
        assert_ne!(original, fingerprint(root.path()).unwrap());
    }
}
