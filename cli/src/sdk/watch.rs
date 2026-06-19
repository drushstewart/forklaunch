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
                    .value_parser(["openapi", "contract"])
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
    let root = match source {
        "contract" => app_root.join(".forklaunch").join("contracts"),
        "openapi" => app_root.join(".forklaunch").join("openapi"),
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
