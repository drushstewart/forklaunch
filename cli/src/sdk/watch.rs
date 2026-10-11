use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};
use termcolor::{ColorChoice, StandardStream, WriteColor};

use crate::{
    CliCommand, core::command::command, init::rust_project,
    sdk::generate::generate_or_check_sdk_outputs,
};

#[derive(Debug)]
pub(crate) struct WatchCommand;

impl WatchCommand {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl CliCommand for WatchCommand {
    fn command(&self) -> Command {
        command("watch", "Regenerate SDKs when Rust artifacts change")
            .arg(
                Arg::new("base_path")
                    .long("path")
                    .short('p')
                    .help("Path to application root"),
            )
            .arg(
                Arg::new("target")
                    .long("target")
                    .short('t')
                    .default_value("typescript")
                    .value_parser(["typescript", "rust", "all"])
                    .help("SDK target to generate"),
            )
            .arg(
                Arg::new("service")
                    .long("service")
                    .short('s')
                    .help("Generate SDKs for one service"),
            )
            .arg(
                Arg::new("from")
                    .long("from")
                    .default_value("contract")
                    .value_parser(["openapi", "contract", "socket-contract"])
                    .help("Artifact source to watch"),
            )
            .arg(
                Arg::new("interval_ms")
                    .long("interval-ms")
                    .default_value("1000")
                    .value_parser(clap::value_parser!(u64).range(100..))
                    .help("Polling interval in milliseconds"),
            )
            .arg(
                Arg::new("once")
                    .long("once")
                    .action(ArgAction::SetTrue)
                    .help("Run one regeneration pass and exit"),
            )
    }

    fn handler(&self, matches: &ArgMatches) -> Result<()> {
        let mut stdout = StandardStream::stdout(ColorChoice::Always);
        let (app_root, _) = crate::core::validate::require_manifest(matches)?;

        if !rust_project::is_rust_app_root(&app_root)? {
            anyhow::bail!("SDK watch is currently implemented for Rust and mixed Rust apps");
        }

        let target = matches.get_one::<String>("target").unwrap().as_str();
        let source = matches.get_one::<String>("from").unwrap().as_str();
        let selected_service = matches.get_one::<String>("service").map(String::as_str);
        let interval = Duration::from_millis(*matches.get_one::<u64>("interval_ms").unwrap());
        let once = matches.get_flag("once");

        run_sdk_watch(
            &app_root,
            target,
            source,
            selected_service,
            interval,
            once,
            &mut stdout,
        )
    }
}

fn run_sdk_watch(
    app_root: &Path,
    target: &str,
    source: &str,
    selected_service: Option<&str>,
    interval: Duration,
    once: bool,
    stdout: &mut StandardStream,
) -> Result<()> {
    generate_or_check_sdk_outputs(app_root, target, source, selected_service, false, stdout)?;

    if once {
        log_ok!(stdout, "SDK watch pass completed");
        return Ok(());
    }

    let mut last_fingerprint = artifact_fingerprint(app_root, source, selected_service)?;
    log_info!(
        stdout,
        "Watching Rust {source} artifacts every {}ms",
        interval.as_millis()
    );

    loop {
        sleep(interval);
        let fingerprint = artifact_fingerprint(app_root, source, selected_service)?;
        if fingerprint == last_fingerprint {
            continue;
        }

        last_fingerprint = fingerprint;
        log_info!(stdout, "Rust {source} artifacts changed; regenerating SDKs");
        generate_or_check_sdk_outputs(app_root, target, source, selected_service, false, stdout)?;
    }
}

fn artifact_fingerprint(
    app_root: &Path,
    source: &str,
    selected_service: Option<&str>,
) -> Result<BTreeMap<PathBuf, (u64, u128)>> {
    if source == "socket-contract" {
        return socket_artifact_fingerprint(app_root, selected_service);
    }
    let root = match source {
        "contract" => app_root.join(".forklaunch").join("contracts"),
        "openapi" => app_root.join(".forklaunch").join("openapi"),
        "socket-contract" => app_root.join(".forklaunch").join("socket-contracts"),
        _ => unreachable!("validated by clap"),
    };
    let mut fingerprint = BTreeMap::new();
    collect_artifact_fingerprint(&root, selected_service, &mut fingerprint)?;
    Ok(fingerprint)
}

fn collect_artifact_fingerprint(
    path: &Path,
    selected_service: Option<&str>,
    fingerprint: &mut BTreeMap<PathBuf, (u64, u128)>,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(path).with_context(|| format!("Failed to read {}", path.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if selected_service.is_some()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| Some(name) != selected_service)
                && path.parent().is_some_and(|parent| {
                    parent.file_name().and_then(|name| name.to_str()) == Some("contracts")
                        || parent.file_name().and_then(|name| name.to_str()) == Some("openapi")
                        || parent.file_name().and_then(|name| name.to_str())
                            == Some("socket-contracts")
                })
            {
                continue;
            }
            collect_artifact_fingerprint(&path, selected_service, fingerprint)?;
            continue;
        }

        let metadata = fs::metadata(&path)
            .with_context(|| format!("Failed to read metadata for {}", path.display()))?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        fingerprint.insert(path, (metadata.len(), modified));
    }

    Ok(())
}

fn socket_artifact_fingerprint(
    app_root: &Path,
    selected: Option<&str>,
) -> Result<BTreeMap<PathBuf, (u64, u128)>> {
    let root = app_root.canonicalize()?;
    let manifest_path = root.join(".forklaunch/manifest.toml");
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(&manifest_path)?)?;
    let mut paths = vec![manifest_path];
    for project in manifest
        .get("projects")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        if !matches!(
            project.get("type").and_then(toml::Value::as_str),
            Some("Service" | "Worker")
        ) {
            continue;
        }
        let Some(name) = project.get("name").and_then(toml::Value::as_str) else {
            continue;
        };
        if selected.is_some_and(|selected| selected != name) {
            continue;
        }
        let configured = project
            .get("metadata")
            .and_then(|metadata| metadata.get("socketContractArtifactPath"))
            .and_then(toml::Value::as_str);
        paths.push(root.join(configured.map_or_else(
            || format!(".forklaunch/socket-contracts/{name}/socket-contract.json"),
            str::to_owned,
        )));
    }
    let mut fingerprint = BTreeMap::new();
    for path in paths {
        if !path.exists() {
            continue;
        }
        let canonical = path.canonicalize()?;
        anyhow::ensure!(
            canonical.starts_with(&root),
            "Socket artifact path must stay inside the application"
        );
        let metadata = fs::metadata(&canonical)?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        fingerprint.insert(canonical, (metadata.len(), modified));
    }
    Ok(fingerprint)
}

#[cfg(test)]
mod socket_tests {
    use super::*;
    #[test]
    fn tracks_configured_socket_artifacts_and_service_selection() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join(".forklaunch")).unwrap();
        fs::create_dir_all(root.join("custom")).unwrap();
        fs::write(
            root.join(".forklaunch/manifest.toml"),
            r#"[[projects]]
name = "chat"
type = "Service"
[projects.metadata]
socketContractArtifactPath = "custom/chat.json"
[[projects]]
name = "other"
type = "Service"
[projects.metadata]
socketContractArtifactPath = "custom/other.json"
"#,
        )
        .unwrap();
        fs::write(root.join("custom/chat.json"), "{}").unwrap();
        fs::write(root.join("custom/other.json"), "{}").unwrap();
        let initial = artifact_fingerprint(root, "socket-contract", Some("chat")).unwrap();
        assert_eq!(initial.len(), 2);
        fs::write(root.join("custom/other.json"), "changed other").unwrap();
        assert_eq!(
            initial,
            artifact_fingerprint(root, "socket-contract", Some("chat")).unwrap()
        );
        fs::write(root.join("custom/chat.json"), "changed chat").unwrap();
        assert_ne!(
            initial,
            artifact_fingerprint(root, "socket-contract", Some("chat")).unwrap()
        );
        assert_eq!(
            artifact_fingerprint(root, "socket-contract", None)
                .unwrap()
                .len(),
            3
        );
    }
}
