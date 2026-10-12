//! Own SDK watch for exactly the lifetime of the development stack.
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

/// Plan a watch before the first Rust SDK exists; create its directory only after dry-run.
pub(super) fn rust_watch(
    root: &Path,
    target: Option<&str>,
    compose: &mut serde_json::Value,
    changes: &mut serde_json::Value,
) -> Result<Option<std::path::PathBuf>> {
    if !matches!(target, Some("rust" | "all")) {
        return Ok(None);
    }
    let path = crate::core::application_output::output_path(root, Path::new("sdks/rust"))?;
    for (name, change) in changes
        .as_object_mut()
        .context("Native development changes missing")?
    {
        let watches = change["develop"]["watch"]
            .as_array_mut()
            .context("Development watches missing")?;
        if !watches
            .iter()
            .any(|watch| watch["path"] == serde_json::json!(path))
        {
            watches.push(serde_json::json!({"action":"restart","path":path,"ignore":["target/","**/target/","**/node_modules/","**/.git/"]}));
        }
        compose["services"][name]["develop"]["watch"] = serde_json::json!(watches);
    }
    Ok(Some(path))
}

pub(super) fn command(root: &Path, target: &str, service: Option<&str>) -> Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["sdk", "watch", "--path"])
        .arg(root)
        .args(["--target", target]);
    if let Some(service) = service {
        command.args(["--service", service]);
    }
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    Ok(command)
}

pub(super) struct Watcher {
    child: Child,
    reported: bool,
}
impl Watcher {
    pub(super) fn spawn(mut command: Command) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        Ok(Self {
            child: command
                .spawn()
                .context("Unable to start development SDK watch")?,
            reported: false,
        })
    }
    pub(super) fn report_exit(&mut self) {
        if !self.reported
            && let Ok(Some(status)) = self.child.try_wait()
        {
            eprintln!(
                "Development SDK watch exited with {status}; service development is still running"
            );
            self.reported = true;
        }
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        #[cfg(unix)]
        {
            use nix::{
                sys::signal::{Signal, killpg},
                unistd::Pid,
            };
            if let Ok(id) = i32::try_from(self.child.id())
                && id > 1
            {
                let _ = killpg(Pid::from_raw(id), Signal::SIGTERM);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn rust_sdk_watch_is_planned_before_directory_creation() {
        let root = tempfile::tempdir().unwrap();
        let mut changes = serde_json::json!({"api":{"develop":{"watch":[]}}});
        let mut compose = serde_json::json!({"services":{"api":{"develop":{"watch":[]}}}});
        let path = rust_watch(root.path(), Some("all"), &mut compose, &mut changes)
            .unwrap()
            .unwrap();
        assert!(!path.exists());
        assert_eq!(
            compose["services"]["api"]["develop"]["watch"][0]["path"],
            serde_json::json!(path)
        );
        rust_watch(root.path(), Some("rust"), &mut compose, &mut changes).unwrap();
        assert_eq!(
            changes["api"]["develop"]["watch"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn development_sdk_watcher_is_reaped_on_scope_exit() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 60"]);
        let watcher = Watcher::spawn(command).unwrap();
        let pid = nix::unistd::Pid::from_raw(watcher.child.id() as i32);
        drop(watcher);
        assert_eq!(
            nix::sys::signal::kill(pid, None),
            Err(nix::errno::Errno::ESRCH)
        );
    }
}
