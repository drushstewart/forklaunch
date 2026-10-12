//! Run mixed applications with native rebuild/restart and existing TS dev commands.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{Arg, ArgAction, ArgMatches, Command};
use convert_case::{Case, Casing};
use serde_json::{Value, json};

use crate::{
    CliCommand,
    core::{application_output::output_path, command::command, env::find_workspace_root},
};

mod contracts;
mod sdk;

pub(crate) struct DevCommand;

impl CliCommand for DevCommand {
    fn command(&self) -> Command {
        command("dev", "Run TypeScript and Rust services with local development reloads")
            .arg(Arg::new("base_path").long("path").short('p'))
            .arg(Arg::new("services").num_args(0..).help("Compose services to start; defaults to the application"))
            .arg(Arg::new("compose_file").long("compose-file").short('f').action(ArgAction::Append).help("Additional Compose overrides, resolved from the application root"))
            .arg(Arg::new("project_name").long("project-name"))
            .arg(Arg::new("sdk_target").long("sdk-target").value_parser(["typescript", "rust", "all"]).help("Run HTTP SDK watch with development; implies --export-contracts"))
            .arg(Arg::new("sdk_service").long("sdk-service").requires("sdk_target").help("Limit HTTP SDK generation to one registered service"))
            .arg(Arg::new("export_contracts").long("export-contracts").action(ArgAction::SetTrue).help("Export native contracts and OpenAPI before startup and each reload"))
            .arg(Arg::new("no_watch").long("no-watch").action(ArgAction::SetTrue))
            .arg(Arg::new("no_deps").long("no-deps").action(ArgAction::SetTrue))
            .arg(Arg::new("no_build").long("no-build").action(ArgAction::SetTrue))
            .arg(Arg::new("dryrun").long("dryrun").visible_alias("dry-run").action(ArgAction::SetTrue).help("Print native development changes without starting containers or writing app files"))
    }

    fn handler(&self, matches: &ArgMatches) -> Result<()> {
        let start = matches
            .get_one::<String>("base_path")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        let root = find_workspace_root(&start)?;
        let base = [
            "docker-compose.yaml",
            "docker-compose.yml",
            "compose.yaml",
            "compose.yml",
        ]
        .into_iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .context("Application Compose file missing")?;
        let docker = crate::core::openapi_export::resolve_command("docker");
        let mut config = Process::new(&docker);
        config
            .args(["compose", "--project-directory"])
            .arg(&root)
            .arg("-f")
            .arg(base);
        if let Some(files) = matches.get_many::<String>("compose_file") {
            for file in files {
                config.arg("-f").arg(root.join(file));
            }
        }
        if let Some(name) = matches.get_one::<String>("project_name") {
            config.arg("--project-name").arg(name);
        }
        let output = config
            .args(["config", "--format", "json"])
            .current_dir(&root)
            .output()
            .context("Docker Compose is required for local development")?;
        if !output.status.success() {
            bail!(
                "Compose configuration failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let base: Value =
            serde_json::from_slice(&output.stdout).context("Invalid Compose configuration")?;
        let manifest: toml::Value =
            toml::from_str(&fs::read_to_string(root.join(".forklaunch/manifest.toml"))?)?;
        if let Some(service) = matches.get_one::<String>("sdk_service") {
            anyhow::ensure!(
                manifest
                    .get("projects")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|project| project.get("name").and_then(toml::Value::as_str)
                        == Some(service.as_str())
                        && matches!(
                            project.get("type").and_then(toml::Value::as_str),
                            Some("Service" | "Worker")
                        )),
                "Unknown SDK service: {service}"
            );
        }
        let selected: Vec<String> = matches
            .get_many::<String>("services")
            .map(|names| names.cloned().collect())
            .unwrap_or_default();
        let (mut planned, mut changes) = plan(
            &root,
            &manifest,
            base,
            &selected,
            !matches.get_flag("no_deps"),
        )?;
        if matches.get_flag("export_contracts") || matches.contains_id("sdk_target") {
            contracts::enable(&root, &manifest, &mut planned, &mut changes)?;
        }
        let rust_sdk_directory = sdk::rust_watch(
            &root,
            matches.get_one::<String>("sdk_target").map(String::as_str),
            &mut planned,
            &mut changes,
        )?;
        let watch = !matches.get_flag("no_watch") && !changes.as_object().unwrap().is_empty();
        if matches.get_flag("dryrun") {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"root":root,"watch":watch,"selectedServices":selected,"nativeServices":changes,"sdkTarget":matches.get_one::<String>("sdk_target"),"sdkService":matches.get_one::<String>("sdk_service")})
                )?
            );
            return Ok(());
        }
        if watch {
            let version = Process::new(&docker)
                .args(["compose", "version", "--short"])
                .output()?;
            if !version.status.success()
                || !supports_restart(&String::from_utf8_lossy(&version.stdout))
            {
                bail!(
                    "Native development reloads require Docker Compose 2.32 or later; --no-watch starts without reloads"
                );
            }
        }
        if let Some(directory) = rust_sdk_directory {
            fs::create_dir_all(directory)?;
        }
        // Resolved Compose may contain environment values; tempfile keeps it private.
        let mut temporary = tempfile::Builder::new()
            .prefix("forklaunch-dev-")
            .suffix(".json")
            .tempfile()?;
        serde_json::to_writer(&mut temporary, &planned)?;
        temporary.flush()?;
        let mut run = Process::new(&docker);
        run.args(["compose", "--project-directory"])
            .arg(&root)
            .arg("-f")
            .arg(temporary.path())
            .arg("up")
            .arg(if matches.get_flag("no_build") {
                "--no-build"
            } else {
                "--build"
            });
        if watch {
            run.arg("--watch");
        }
        if matches.get_flag("no_deps") {
            run.arg("--no-deps");
        }
        run.args(&selected)
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        run.env("COMPOSE_MENU", "false");
        let sdk = matches
            .get_one::<String>("sdk_target")
            .map(|target| {
                sdk::command(
                    &root,
                    target,
                    matches.get_one::<String>("sdk_service").map(String::as_str),
                )
            })
            .transpose()?;
        supervise(run, sdk)
    }
}

fn supports_restart(version: &str) -> bool {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    matches!((parts.next().and_then(|v| v.parse::<u32>().ok()), parts.next().and_then(|v| v.parse::<u32>().ok())), (Some(major), Some(minor)) if major > 2 || major == 2 && minor >= 32)
}

fn plan(
    root: &Path,
    manifest: &toml::Value,
    mut compose: Value,
    selected: &[String],
    dependencies: bool,
) -> Result<(Value, Value)> {
    let services = compose
        .get("services")
        .and_then(Value::as_object)
        .context("Compose services must be an object")?;
    for name in selected {
        if !services.contains_key(name) {
            bail!("Unknown Compose service: {name}");
        }
    }
    let mut active: BTreeSet<String> = if selected.is_empty() {
        services.keys().cloned().collect()
    } else {
        selected.iter().cloned().collect()
    };
    if dependencies {
        loop {
            let before = active.len();
            for name in active.clone() {
                if let Some(dependencies) = services
                    .get(&name)
                    .and_then(|service| service.get("depends_on"))
                    .and_then(Value::as_object)
                {
                    active.extend(dependencies.keys().cloned());
                }
            }
            if active.len() == before {
                break;
            }
        }
    }
    let mut required = active.clone();
    for name in &active {
        if let Some(binary) = services
            .get(name)
            .and_then(|service| service.get("command"))
            .and_then(Value::as_array)
            .and_then(|args| args.first())
            .and_then(Value::as_str)
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str())
        {
            required.insert(binary.to_owned());
        }
    }
    let native_project = |project: &toml::Value| {
        if project.get("variant").and_then(toml::Value::as_str) == Some("storefront") { return false; }
        let language = project
            .get("metadata")
            .and_then(|metadata| metadata.get("language"))
            .and_then(toml::Value::as_str);
        language
            .map(|language| language == "rust")
            .unwrap_or_else(|| {
                project
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|name| root.join("crates").join(name).join("Cargo.toml").is_file())
            })
    };
    let projects = manifest
        .get("projects")
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if !projects.iter().any(|project| {
        native_project(project)
            && project
                .get("name")
                .and_then(toml::Value::as_str)
                .is_some_and(|name| required.contains(name))
    }) {
        return Ok((compose, json!({})));
    }
    let mut native = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for project in manifest
        .get("projects")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = project
            .get("name")
            .and_then(toml::Value::as_str)
            .context("Project name missing")?;
        let metadata = project.get("metadata");
        if !native_project(project) {
            continue;
        }
        let relative = metadata
            .and_then(|metadata| {
                metadata
                    .get("cratePath")
                    .or_else(|| metadata.get("crate_path"))
            })
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("crates/{name}"));
        let directory = output_path(root, Path::new(&relative))?;
        if directory.is_dir() {
            paths.insert(directory.clone());
        }
        if matches!(
            project.get("type").and_then(toml::Value::as_str),
            Some("Service" | "Worker")
        ) && required.contains(name)
        {
            let cargo: toml::Value =
                toml::from_str(&fs::read_to_string(directory.join("Cargo.toml"))?)?;
            let package = cargo
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(toml::Value::as_str)
                .context("Native Cargo package name missing")?;
            if package.is_empty()
                || package.starts_with('-')
                || !package
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                bail!("Unsupported native Cargo package name");
            }
            if name.is_empty()
                || name.starts_with('-')
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                bail!("Unsupported native binary name");
            }
            native.insert(name.to_owned(), package.to_owned());
        }
    }
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        ".forklaunch/framework-rs",
        ".forklaunch/blueprint-rs",
        "sdks/rust",
    ] {
        let path = output_path(root, Path::new(relative))?;
        if path.exists() {
            paths.insert(path);
        }
    }
    let project = compose
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("forklaunch")
        .to_case(Case::Kebab);
    let dockerfile = include_str!("templates/rust/application/Dockerfile")
        .split_once("COPY . .")
        .context("Native builder template missing source-copy boundary")?
        .0
        .replace("pkg-config libssl-dev", "pkg-config curl libssl-dev");
    let mut cache_names = Vec::new();
    for suffix in ["target", "registry", "git"] {
        let base = format!("forklaunch-native-{suffix}");
        let mut name = base.clone();
        let mut index = 0;
        while compose
            .get("volumes")
            .and_then(|volumes| volumes.get(&name))
            .is_some()
        {
            index += 1;
            name = format!("{base}-{index}");
        }
        cache_names.push(name);
    }
    let mut changes = serde_json::Map::new();
    let services = compose["services"].as_object_mut().unwrap();
    for (name, service) in services.iter_mut() {
        if !active.contains(name) {
            continue;
        }
        // This also finds generated auxiliary workers using their owner's binary.
        let executable = service
            .get("command")
            .and_then(Value::as_array)
            .and_then(|args| args.first())
            .and_then(Value::as_str);
        let binary = executable
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str());
        let Some((binary, package)) = binary.and_then(|binary| {
            native
                .get(binary)
                .map(|package| (binary.to_owned(), package))
        }) else {
            if native.contains_key(name) {
                bail!(
                    "Native service {name} has a custom launch command; configure a generated binary command before using forklaunch dev"
                );
            }
            continue;
        };
        if service.get("entrypoint").is_some_and(|entry| {
            !entry.is_null() && entry.as_array().is_none_or(|args| !args.is_empty())
        }) {
            bail!(
                "Native service {name} has a custom entrypoint; configure its development startup explicitly"
            );
        }
        let command = service["command"].as_array_mut().unwrap();
        command[0] = json!(format!("/app/target/debug/{binary}"));
        let watch: Vec<_> = paths.iter().map(|path| json!({"action":"restart","path":path,"ignore":["target/","**/target/","**/node_modules/","**/.git/"]})).collect();
        let settings = json!({
            "image":format!("forklaunch-dev-{project}-{}:local", name.to_case(Case::Kebab)),
            "build":{"context":root,"dockerfile_inline":dockerfile},
            "working_dir":"/app",
            "entrypoint":["sh","-c",format!("cargo build --package {package} --bin {binary} && exec \"$@\""),"--"],
            "develop":{"watch":watch}
        });
        let object = service
            .as_object_mut()
            .context("Compose service must be an object")?;
        for (key, value) in settings.as_object().unwrap() {
            object.insert(key.clone(), value.clone());
        }
        let volumes = object
            .entry("volumes")
            .or_insert(json!([]))
            .as_array_mut()
            .context("Resolved Compose volumes must be an array")?;
        for volume in [
            json!({"type":"bind","source":root,"target":"/app"}),
            json!({"type":"volume","source":cache_names[0],"target":"/app/target"}),
            json!({"type":"volume","source":cache_names[1],"target":"/usr/local/cargo/registry"}),
            json!({"type":"volume","source":cache_names[2],"target":"/usr/local/cargo/git"}),
        ] {
            volumes.retain(|existing| existing.get("target") != volume.get("target"));
            volumes.push(volume);
        }
        let environment = object
            .entry("environment")
            .or_insert(json!({}))
            .as_object_mut()
            .context("Resolved Compose environment must be an object")?;
        environment.insert("CARGO_TARGET_DIR".into(), json!("/app/target"));
        environment.entry("CARGO_INCREMENTAL").or_insert(json!("0"));
        let mut settings = settings;
        settings["command"] = object["command"].clone();
        settings["volumes"] = object["volumes"].clone();
        settings["environment"] = json!({"CARGO_TARGET_DIR":"/app/target", "CARGO_INCREMENTAL":object["environment"]["CARGO_INCREMENTAL"]});
        changes.insert(name.clone(), settings);
    }
    if !changes.is_empty() {
        let volumes = compose
            .as_object_mut()
            .unwrap()
            .entry("volumes")
            .or_insert(json!({}))
            .as_object_mut()
            .context("Compose volumes must be an object")?;
        for name in cache_names {
            volumes.entry(name).or_insert(json!({}));
        }
    }
    Ok((compose, Value::Object(changes)))
}

fn supervise(mut command: Process, sdk: Option<Process>) -> Result<()> {
    let interrupts = Arc::new(AtomicUsize::new(0));
    let handler = interrupts.clone();
    ctrlc::set_handler(move || {
        handler.fetch_add(1, Ordering::SeqCst);
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut sdk = sdk.map(sdk::Watcher::spawn).transpose()?;
    let mut child = command
        .spawn()
        .context("Failed to start development stack")?;
    let mut forwarded = 0;
    loop {
        if let Some(watcher) = &mut sdk {
            watcher.report_exit();
        }
        if let Some(status) = child.try_wait()? {
            if status.success() || forwarded > 0 {
                return Ok(());
            }
            bail!("Development stack exited with {status}");
        }
        let received = interrupts.load(Ordering::SeqCst);
        if received > forwarded {
            #[cfg(unix)]
            {
                use nix::{
                    sys::signal::{Signal, killpg},
                    unistd::Pid,
                };
                let id = i32::try_from(child.id()).context("Invalid Compose process ID")?;
                if id > 1 {
                    let _ = killpg(Pid::from_raw(id), Signal::SIGINT);
                }
            }
            forwarded = received;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture() -> (tempfile::TempDir, toml::Value, Value) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("services/native-api")).unwrap();
        fs::create_dir_all(root.path().join("shared/models")).unwrap();
        fs::write(root.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(
            root.path().join("services/native-api/Cargo.toml"),
            "[package]\nname = 'native-core'\n",
        )
        .unwrap();
        let manifest = toml::from_str(
            r#"
            runtime = "node"
            [[projects]]
            name = "native-api"
            type = "Service"
            [projects.metadata]
            language = "rust"
            cratePath = "services/native-api"
            [[projects]]
            name = "models"
            type = "Library"
            [projects.metadata]
            language = "rust"
            cratePath = "shared/models"
            [[projects]]
            name = "legacy-api"
            type = "Service"
        "#,
        )
        .unwrap();
        let compose = json!({"name":"test-app","services":{
            "native-api":{"command":["./target/release/native-api"],"environment":{"PORT":"8000"},"volumes":[{"type":"volume","source":"data","target":"/app/data"}]},
            "native-outbox":{"command":["./target/release/native-api","--worker"]},
            "legacy-api":{"entrypoint":["pnpm","run","dev"],"environment":{"PORT":"8001"}}
        },"volumes":{"data":{}}});
        (root, manifest, compose)
    }

    #[test]
    fn plans_custom_native_paths_and_auxiliary_workers_without_changing_typescript() {
        let (root, manifest, base) = fixture();
        let (planned, changes) = plan(root.path(), &manifest, base.clone(), &[], true).unwrap();
        assert_eq!(
            planned["services"]["legacy-api"],
            base["services"]["legacy-api"]
        );
        assert_eq!(
            planned["services"]["native-outbox"]["command"][1],
            "--worker"
        );
        assert_eq!(
            planned["services"]["native-api"]["command"][0],
            "/app/target/debug/native-api"
        );
        assert_eq!(
            planned["services"]["native-api"]["entrypoint"][2],
            "cargo build --package native-core --bin native-api && exec \"$@\""
        );
        assert_eq!(
            planned["services"]["native-api"]["environment"]["PORT"],
            "8000"
        );
        assert_eq!(
            planned["services"]["native-api"]["volumes"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
        let paths: Vec<_> = changes["native-api"]["develop"]["watch"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["path"].as_str().unwrap())
            .collect();
        assert!(
            paths.contains(
                &root
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("shared/models")
                    .to_str()
                    .unwrap()
            )
        );
        assert!(
            paths.contains(
                &root
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("services/native-api")
                    .to_str()
                    .unwrap()
            )
        );
        assert!(
            !changes["native-api"]["build"]["dockerfile_inline"]
                .as_str()
                .unwrap()
                .contains("COPY . .")
        );
        assert!(!root.path().join("docker-compose.dev.yaml").exists());
    }

    #[test]
    fn rejects_unknown_services_external_paths_and_custom_launches() {
        let (root, mut manifest, mut base) = fixture();
        assert!(
            plan(
                root.path(),
                &manifest,
                base.clone(),
                &["missing".into()],
                true
            )
            .is_err()
        );
        base["services"]["native-api"]["entrypoint"] = json!(["custom"]);
        assert!(plan(root.path(), &manifest, base.clone(), &[], true).is_err());
        manifest["projects"][0]["metadata"]["cratePath"] = toml::Value::String("../outside".into());
        assert!(plan(root.path(), &manifest, base, &[], true).is_err());
    }

    #[test]
    fn selected_typescript_does_not_require_unselected_native_source() {
        let (root, manifest, base) = fixture();
        fs::remove_file(root.path().join("services/native-api/Cargo.toml")).unwrap();
        let (planned, changes) = plan(
            root.path(),
            &manifest,
            base.clone(),
            &["legacy-api".into()],
            false,
        )
        .unwrap();
        assert_eq!(planned, base);
        assert_eq!(changes, json!({}));
    }

    #[test]
    fn infrastructure_containers_are_not_native_crates() {
        let (root, mut manifest, mut base) = fixture();
        manifest["runtime"] = toml::Value::String("tokio".into());
        manifest["projects"]
            .as_array_mut()
            .unwrap()
            .push(toml::from_str("name = 'postgresql'\ntype = 'Service'\n").unwrap());
        base["services"]["postgresql"] = json!({"image":"postgres:17","command":["postgres"]});
        let (planned, changes) = plan(
            root.path(),
            &manifest,
            base.clone(),
            &["postgresql".into()],
            false,
        )
        .unwrap();
        assert_eq!(planned, base);
        assert_eq!(changes, json!({}));
    }

    #[test]
    fn compose_restart_version_and_cli_selection_are_checked() {
        for version in ["2.32.0", "v2.32.1-desktop.1", "5.1.2\n"] {
            assert!(supports_restart(version));
        }
        for version in ["2.31.9", "1.29.0", "invalid"] {
            assert!(!supports_restart(version));
        }
        let command = DevCommand.command().version("test");
        let parsed = command
            .try_get_matches_from(["dev", "native-api", "legacy-api", "--no-deps", "--dry-run"])
            .unwrap();
        assert_eq!(parsed.get_many::<String>("services").unwrap().count(), 2);
        assert!(parsed.get_flag("dryrun"));
    }
}
