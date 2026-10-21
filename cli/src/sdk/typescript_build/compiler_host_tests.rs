use super::*;
use std::collections::BTreeMap;

fn fixture(selection: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".forklaunch")).unwrap();
    fs::create_dir_all(root.path().join("src/iam")).unwrap();
    fs::write(
        root.path().join(".forklaunch/manifest.toml"),
        "runtime='node'\nmodules_path='src'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".forklaunch/framework-dependencies.toml"),
        selection,
    )
    .unwrap();
    fs::write(root.path().join("Cargo.toml"), "# Application-owned workspace settings\n[workspace]\nmembers = ['src/notes']\nresolver = '3'\n[patch.crates-io]\nserde_json = { path = 'vendor/serde_json' }\n[profile.dev]\ndebug = 0\n").unwrap();
    root
}

fn tree(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(root: &Path, result: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, result);
            } else {
                result.insert(path.clone(), fs::read(&path).unwrap());
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, &mut result);
    result
}

fn hook(root: &Path, compiler: &str, runner: &str) -> String {
    super::super::write_if_changed(root.join(".forklaunch/tools/contract-build.mjs"), runner)
        .unwrap();
    write_hook_config(
        root,
        "iam",
        &serde_json::json!({"name":"iam", "service":"src/iam", "compiler":compiler}),
    )
    .unwrap();
    hook_script(root, &root.join("src/iam"), "iam").unwrap()
}

#[test]
fn registry_contract_compiler_uses_selected_version_and_root_workspace_idempotently() {
    for version in ["0.1.0", "0.1.1"] {
        let root = fixture(&format!("version='{version}'\n"));
        let before = tree(root.path());
        let plan = compiler_host::plan(root.path()).unwrap();
        assert_eq!(plan.compiler, REGISTRY_COMPILER);
        assert_eq!(
            tree(root.path()),
            before,
            "Planning must not mutate application files"
        );
        plan.write().unwrap();
        let manifest: toml::Value = toml::from_str(
            &fs::read_to_string(root.path().join(REGISTRY_COMPILER).join("Cargo.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["package"]["publish"].as_bool(), Some(false));
        assert!(manifest.get("workspace").is_none());
        assert!(manifest.get("patch").is_none());
        assert_eq!(
            manifest["dependencies"]["forklaunch-core"]["version"].as_str(),
            Some(format!("={version}").as_str())
        );
        assert!(
            manifest["dependencies"]["forklaunch-core"]
                .get("path")
                .is_none()
        );
        let workspace_source = fs::read_to_string(root.path().join("Cargo.toml")).unwrap();
        assert!(workspace_source.starts_with("# Application-owned workspace settings"));
        let workspace: toml::Value = toml::from_str(&workspace_source).unwrap();
        assert_eq!(
            workspace["workspace"]["members"].as_array().unwrap(),
            &vec![
                toml::Value::String("src/notes".into()),
                toml::Value::String(REGISTRY_COMPILER.into())
            ]
        );
        assert_eq!(
            workspace["patch"]["crates-io"]["serde_json"]["path"].as_str(),
            Some("vendor/serde_json")
        );
        assert!(!root.path().join("Cargo.lock").exists());
        assert!(
            !root
                .path()
                .join(REGISTRY_COMPILER)
                .join("Cargo.lock")
                .exists()
        );
        assert!(!root.path().join("vendor/framework").exists());
        let script = hook(root.path(), REGISTRY_COMPILER, include_str!("runner.mjs"));
        assert_eq!(
            installed_compiler(root.path(), &root.path().join("src/iam"), "iam", &script)
                .unwrap()
                .as_deref(),
            Some(REGISTRY_COMPILER)
        );
        let before = tree(root.path());
        compiler_host::plan(root.path()).unwrap().write().unwrap();
        assert_eq!(tree(root.path()), before);
    }
}

#[test]
fn registry_contract_compiler_preserves_custom_sources_and_requires_membership() {
    for relative in ["main.rs", "Cargo.toml"] {
        let root = fixture("version='0.1.1'");
        compiler_host::plan(root.path()).unwrap().write().unwrap();
        let script = hook(root.path(), REGISTRY_COMPILER, include_str!("runner.mjs"));
        let path = root.path().join(REGISTRY_COMPILER).join(relative);
        fs::write(
            &path,
            format!(
                "{}\npub const APPLICATION_CUSTOMIZATION: bool = true;\n",
                fs::read_to_string(&path).unwrap()
            ),
        )
        .unwrap();
        let before = tree(root.path());
        assert!(compiler_host::plan(root.path()).is_err());
        assert!(
            installed_compiler(root.path(), &root.path().join("src/iam"), "iam", &script).is_err()
        );
        assert_eq!(tree(root.path()), before);
    }
    let root = fixture("version='0.1.1'");
    compiler_host::plan(root.path()).unwrap().write().unwrap();
    let workspace = root.path().join("Cargo.toml");
    let mut value: toml::Value = toml::from_str(&fs::read_to_string(&workspace).unwrap()).unwrap();
    value["workspace"]["members"] = toml::Value::Array(vec!["src/notes".into()]);
    fs::write(&workspace, toml::to_string(&value).unwrap()).unwrap();
    assert!(compiler_host::validate(root.path()).is_err());
    compiler_host::plan(root.path()).unwrap().write().unwrap();
    compiler_host::validate(root.path()).unwrap();
    fs::write(
        root.path().join(REGISTRY_COMPILER).join("Cargo.lock"),
        "nested lock must remain application-owned",
    )
    .unwrap();
    assert!(compiler_host::plan(root.path()).is_err());
}

#[test]
fn registry_contract_compiler_survives_application_cargo_fmt_without_losing_ownership() {
    use std::io::Write;
    let root = fixture("version='0.1.1'");
    compiler_host::plan(root.path()).unwrap().write().unwrap();
    let path = root.path().join(REGISTRY_COMPILER).join("main.rs");
    let original = fs::read(&path).unwrap();
    let mut formatter = ProcessCommand::new("rustfmt")
        .args([
            "--edition",
            "2024",
            "--emit",
            "stdout",
            "--config",
            "max_width=100,use_small_heuristics=Max",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    formatter
        .stdin
        .take()
        .unwrap()
        .write_all(&original)
        .unwrap();
    let output = formatter.wait_with_output().unwrap();
    assert!(output.status.success());
    fs::write(&path, &output.stdout).unwrap();
    compiler_host::validate(root.path()).unwrap();
    compiler_host::plan(root.path()).unwrap().write().unwrap();
    assert_eq!(
        fs::read(path).unwrap(),
        output.stdout,
        "Reconciliation must preserve formatted application bytes"
    );
}

#[test]
fn contract_compiler_keeps_pinned_snapshots_local_paths_and_legacy_runner() {
    for snapshot in ["vendor/framework", ".forklaunch/framework-rs"] {
        let root = fixture("version='0.1.1'");
        fs::create_dir_all(root.path().join(snapshot)).unwrap();
        fs::write(
            root.path().join(snapshot).join("Cargo.toml"),
            "[workspace]\n",
        )
        .unwrap();
        fs::write(root.path().join(snapshot).join("Cargo.lock"), "# pinned\n").unwrap();
        let script = hook(root.path(), snapshot, include_str!("runner_legacy.mjs"));
        let before = tree(root.path());
        let plan = compiler_host::plan(root.path()).unwrap();
        assert_eq!(plan.compiler, snapshot);
        plan.write().unwrap();
        assert_eq!(
            installed_compiler(root.path(), &root.path().join("src/iam"), "iam", &script)
                .unwrap()
                .as_deref(),
            Some(snapshot)
        );
        assert_eq!(tree(root.path()), before);
    }
    let framework = tempfile::tempdir().unwrap();
    let selection = toml::to_string(&BTreeMap::from([(
        "path",
        framework.path().to_string_lossy().into_owned(),
    )]))
    .unwrap();
    let root = fixture(&selection);
    compiler_host::plan(root.path()).unwrap().write().unwrap();
    let manifest: toml::Value = toml::from_str(
        &fs::read_to_string(root.path().join(REGISTRY_COMPILER).join("Cargo.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["dependencies"]["forklaunch-core"]["path"].as_str(),
        framework.path().join("crates/core").to_str()
    );
}

#[cfg(unix)]
#[test]
fn registry_contract_compiler_rejects_symlinked_owned_files_without_writes() {
    use std::os::unix::fs::symlink;
    let root = fixture("version='0.1.1'");
    compiler_host::plan(root.path()).unwrap().write().unwrap();
    let source = root.path().join(REGISTRY_COMPILER).join("main.rs");
    let authored = root.path().join("authored.rs");
    fs::rename(&source, &authored).unwrap();
    symlink(&authored, &source).unwrap();
    let before = fs::read(&authored).unwrap();
    assert!(compiler_host::plan(root.path()).is_err());
    assert_eq!(fs::read(authored).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn contract_runner_builds_root_host_and_allows_new_workspace_lock_entries() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture("version='0.1.1'");
    let canonical_root = root.path().canonicalize().unwrap();
    let bin = root.path().join("fake-tools");
    fs::create_dir(&bin).unwrap();
    let tool = r#"#!/usr/bin/env node
const fs = require('node:fs'), path = require('node:path');
const args = process.argv.slice(2), program = path.basename(process.argv[1]);
if (program === 'cargo' && args[0] === 'metadata') {
  // The workspace's normal target directory, beside its manifest.
  const manifest = args[args.indexOf('--manifest-path')+1];
  process.stdout.write(JSON.stringify({target_directory: path.join(path.dirname(manifest), 'target')}));
  process.exit(0);
}
fs.appendFileSync(process.env.COMPILER_TEST_LOG, JSON.stringify({program,args,cwd:process.cwd()})+'\n');
if (program === 'cargo') {
  if (args.includes('--bin')) {
    const lock = path.join(path.dirname(args[args.indexOf('--manifest-path')+1]), 'Cargo.lock');
    if (args.includes('--locked')) process.exit(9);
    fs.writeFileSync(lock, '# refreshed compiler workspace graph\n');
  }
  if (args.includes('--target-dir')) process.exit(8);
  const target = path.resolve(process.env.CARGO_TARGET_DIR || path.join(path.dirname(args[args.indexOf('--manifest-path')+1]), 'target'));
  const output = args.includes('--bin') ? 'debug/forklaunch-contract-compiler' : 'debug/examples/typescript_contract_build';
  fs.mkdirSync(path.dirname(path.join(target,output)), {recursive:true});
  fs.copyFileSync(process.argv[1],path.join(target,output)); fs.chmodSync(path.join(target,output),0o755);
}
"#;
    for name in ["cargo", "pnpm", "bun"] {
        fs::write(bin.join(name), tool).unwrap();
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let runner = root.path().join(".forklaunch/tools/contract-build.mjs");
    super::super::write_if_changed(&runner, super::RUNNER).unwrap();
    let log = root.path().join("commands.jsonl");
    let paths = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    for (compiler, runtime, existing_lock, target_env) in [
        (REGISTRY_COMPILER, "node", false, true),
        (REGISTRY_COMPILER, "bun", true, true),
        (REGISTRY_COMPILER, "node", true, false),
        ("vendor/framework", "node", true, true),
    ] {
        if existing_lock {
            fs::write(
                root.path().join("Cargo.lock"),
                "# existing root lock without newly added compiler package\n",
            )
            .unwrap();
        }
        if log.exists() {
            fs::remove_file(&log).unwrap();
        }
        write_hook_config(root.path(), "iam", &serde_json::json!({"name":"iam", "service":"src/iam", "modules":"src", "runtime":runtime,"compiler":compiler})).unwrap();
        let mut command = ProcessCommand::new("node");
        command
            .arg(&runner)
            .arg("iam")
            .env("PATH", &paths)
            .env("COMPILER_TEST_LOG", &log)
            .env_remove("FORKLAUNCH_CONTRACT_COMPILER")
            .current_dir(root.path());
        if target_env {
            command.env("CARGO_TARGET_DIR", "relative-compiler-target");
        } else {
            command.env_remove("CARGO_TARGET_DIR");
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let commands: Vec<serde_json::Value> = fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0]["program"], "cargo");
        let args = commands[0]["args"].as_array().unwrap();
        assert_eq!(
            args.contains(&serde_json::json!("--locked")),
            compiler != REGISTRY_COMPILER
        );
        let manifest = args
            .iter()
            .position(|arg| arg == "--manifest-path")
            .unwrap()
            + 1;
        if compiler == REGISTRY_COMPILER {
            assert_eq!(
                args[manifest],
                canonical_root.join("Cargo.toml").to_str().unwrap()
            );
            assert!(args.contains(&serde_json::json!("--bin")));
            assert_eq!(commands[2]["program"], "forklaunch-contract-compiler");
            if !target_env {
                // Built into the workspace target directory, never under .forklaunch.
                assert!(root.path().join("target/debug/forklaunch-contract-compiler").is_file());
                assert!(!root.path().join(".forklaunch/tools/target").exists());
            }
            assert_eq!(
                fs::read_to_string(root.path().join("Cargo.lock")).unwrap(),
                "# refreshed compiler workspace graph\n"
            );
        } else {
            assert_eq!(
                args[manifest],
                canonical_root
                    .join("vendor/framework/Cargo.toml")
                    .to_str()
                    .unwrap()
            );
            assert!(args.contains(&serde_json::json!("--example")));
            assert_eq!(commands[2]["program"], "typescript_contract_build");
        }
        assert_eq!(
            commands[1]["program"],
            if runtime == "bun" { "bun" } else { "pnpm" }
        );
        assert_eq!(commands[2]["args"][0], "--project");
    }
}
