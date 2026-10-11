//! Offline raw socket artifacts use the bundled native generators.
use std::{
    collections::BTreeMap,
    fs,
    io::{Read as _, Seek as _, Write as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use termcolor::{StandardStream, WriteColor};

mod ownership;

pub(crate) fn generate(
    root: &Path,
    target: &str,
    selected: Option<&str>,
    check: bool,
    stdout: &mut StandardStream,
) -> Result<()> {
    let root = root.canonicalize()?;
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(root.join(".forklaunch/manifest.toml"))?)?;
    let mut files = BTreeMap::new();
    let mut members = Vec::new();
    let mut retired_members = Vec::new();
    let mut removals = BTreeMap::new();
    let mut count = 0;
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
        let name = project
            .get("name")
            .and_then(toml::Value::as_str)
            .context("Socket service name missing")?;
        if selected.is_some_and(|selected| selected != name) {
            continue;
        }
        let configured = project
            .get("metadata")
            .and_then(|metadata| metadata.get("socketContractArtifactPath"))
            .and_then(toml::Value::as_str);
        let artifact = root.join(configured.map_or_else(
            || format!(".forklaunch/socket-contracts/{name}/socket-contract.json"),
            str::to_owned,
        ));
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            bail!("Invalid socket service name {name}");
        }
        if !artifact.exists() {
            let mut retired = false;
            for language in ["typescript", "rust"] {
                if (target == "all" || target == language)
                    && ownership::stage_empty_registry(
                        &root,
                        &mut files,
                        &mut removals,
                        name,
                        language,
                        project,
                    )?
                {
                    retired = true;
                    if language == "rust" {
                        retired_members.push(format!("sdks/rust/{name}-websocket-sdk"));
                    }
                }
            }
            if retired {
                ownership::stage_empty_asyncapi(&root, &mut files, &mut removals, name)?;
                count += 1;
                continue;
            }
            if configured.is_none() && selected.is_none() {
                continue;
            }
        }
        let artifact = artifact
            .canonicalize()
            .with_context(|| format!("Missing socket contract artifact {}", artifact.display()))?;
        if !artifact.starts_with(&root) {
            bail!("Socket artifact path must stay inside the application");
        }
        let compiled = compile(&root, &artifact, target)
            .with_context(|| format!("Socket artifact compilation failed for {name}"))?;
        if compiled["asyncapi"]["asyncapi"] != "3.0.0" || !compiled["asyncapi"]["info"].is_object()
        {
            bail!("Invalid compiled AsyncAPI document");
        }
        let registry_exported = ownership::registry_exported(&root, name, &artifact)?;
        let asyncapi_output = project
            .get("metadata")
            .and_then(|metadata| metadata.get("asyncapiPath"))
            .and_then(toml::Value::as_str)
            .map_or_else(
                || format!(".forklaunch/asyncapi/{name}/asyncapi.json"),
                str::to_owned,
            );
        stage(&mut files, &asyncapi_output, pretty(&compiled["asyncapi"])?)?;
        ownership::stage_asyncapi(
            &root,
            &mut files,
            &mut removals,
            name,
            Path::new(&asyncapi_output),
            registry_exported,
        )?;
        if matches!(target, "typescript" | "all") {
            stage_typescript(&root, &mut files, name, &compiled)?;
            ownership::stage_package(
                &root,
                &mut files,
                &mut removals,
                name,
                "typescript",
                registry_exported,
            )?;
        }
        if matches!(target, "rust" | "all") {
            let member = format!("sdks/rust/{name}-websocket-sdk");
            stage_rust(&mut files, name, &member, &compiled)?;
            ownership::stage_package(
                &root,
                &mut files,
                &mut removals,
                name,
                "rust",
                registry_exported,
            )?;
            members.push(member);
        }
        count += 1;
    }
    if count == 0 {
        bail!("No socket contract artifacts found for the selected services");
    }
    stage_workspace(&root, &mut files, &members, target)?;
    if !retired_members.is_empty() {
        let cargo = files.get_mut(Path::new("Cargo.toml"));
        let content = match cargo.as_ref() {
            Some(content) => content.to_string(),
            None => fs::read_to_string(root.join("Cargo.toml"))?,
        };
        let mut document = content.parse::<toml_edit::DocumentMut>()?;
        let array = document["workspace"]["members"]
            .as_array_mut()
            .context("Root Cargo workspace members missing")?;
        array.retain(|value| {
            !value
                .as_str()
                .is_some_and(|member| retired_members.iter().any(|retired| retired == member))
        });
        files.insert(PathBuf::from("Cargo.toml"), document.to_string());
    }
    for relative in files.keys() {
        validate_destination(&root, relative)?;
    }
    for relative in removals.keys() {
        validate_destination(&root, relative)?;
        if files.contains_key(relative) {
            bail!("Conflicting socket output removal: {}", relative.display());
        }
    }
    if check {
        if let Some(relative) = removals.keys().next() {
            bail!("Stale generated socket SDK output: {}", relative.display());
        }
        for (relative, content) in &files {
            if fs::read_to_string(root.join(relative)).ok().as_deref() != Some(content) {
                bail!("Socket SDK output is out of date: {}", relative.display());
            }
        }
        log_ok!(stdout, "Socket SDK and AsyncAPI outputs are up to date");
    } else {
        ownership::remove_unchanged(&root, &removals)?;
        for (relative, content) in files {
            let destination = root.join(relative);
            fs::create_dir_all(destination.parent().context("Socket output parent")?)?;
            fs::write(destination, content)?;
        }
        log_ok!(
            stdout,
            "Generated socket SDKs and AsyncAPI for {count} service(s)"
        );
    }
    Ok(())
}
fn compile(root: &Path, artifact: &Path, target: &str) -> Result<Value> {
    let manifest = root.join(".forklaunch/framework-rs/Cargo.toml").canonicalize().context("Bundled Rust framework missing; generate or upgrade the application's bundled framework first")?;
    if !manifest.starts_with(root) {
        bail!("Bundled socket compiler must stay inside the application");
    }
    let mut output = tempfile::tempfile()?;
    let mut child = Command::new(crate::core::openapi_export::resolve_command("cargo"))
        .args(["run", "--quiet", "--manifest-path"])
        .arg(manifest)
        .args([
            "-p",
            "forklaunch-sdk-typescript",
            "--example",
            "socket_artifacts",
            "--",
        ])
        .arg(artifact)
        .arg(target)
        .current_dir(root)
        .env("CARGO_INCREMENTAL", "0")
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::inherit())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("Bundled socket compiler exited with {status}");
            }
            break;
        }
        if Instant::now() >= deadline || output.metadata()?.len() > 64 * 1024 * 1024 {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Bundled socket compiler exceeded its time/output limit");
        }
        thread::sleep(Duration::from_millis(100));
    }
    if output.metadata()?.len() > 64 * 1024 * 1024 {
        bail!("Socket compiler output exceeds 64 MiB");
    }
    output.rewind()?;
    let mut bytes = Vec::new();
    output.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).context("Invalid bundled socket compiler output")
}
fn pretty(value: &Value) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}
fn stage(
    files: &mut BTreeMap<PathBuf, String>,
    path: impl Into<PathBuf>,
    content: String,
) -> Result<()> {
    let path = path.into();
    if path.is_absolute()
        || path.components().any(|component| {
            !matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        bail!("Invalid socket output path");
    }
    let normalized: PathBuf = path
        .components()
        .filter(|component| !matches!(component, std::path::Component::CurDir))
        .collect();
    if normalized.as_os_str().is_empty() {
        bail!("Socket output path must name a file");
    }
    if files.insert(normalized, content).is_some() {
        bail!("Conflicting socket output paths");
    }
    Ok(())
}
fn sources<'a>(compiled: &'a Value, target: &str) -> Result<&'a serde_json::Map<String, Value>> {
    let sources = compiled[target]
        .as_object()
        .context("Socket compiler sources missing")?;
    if sources.is_empty() {
        bail!("Socket compiler returned no {target} sources");
    }
    for (name, source) in sources {
        if name.len() > 135
            || !name.starts_with("socket_")
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            || !source.is_string()
        {
            bail!("Invalid compiled socket module {name}");
        }
    }
    Ok(sources)
}
fn stage_typescript(
    root: &Path,
    files: &mut BTreeMap<PathBuf, String>,
    name: &str,
    compiled: &Value,
) -> Result<()> {
    let directory = format!("sdks/typescript-websocket/{name}");
    let mut index = String::new();
    for (module, source) in sources(compiled, "typescript")? {
        index.push_str(&format!("export * as {module} from './{module}.js';\n"));
        stage(
            files,
            format!("{directory}/src/{module}.ts"),
            source.as_str().unwrap().into(),
        )?;
    }
    stage(files, format!("{directory}/src/index.ts"), index)?;
    stage(
        files,
        format!("{directory}/package.json"),
        pretty(&json!({
            "name":format!("{name}-websocket-sdk"),"version":"0.1.0","private":true,"type":"module",
            "main":"./dist/index.js","types":"./dist/index.d.ts","exports":{".":{"types":"./dist/index.d.ts","import":"./dist/index.js"}},
            "scripts":{"build":"tsc --project tsconfig.json","typecheck":"tsc --project tsconfig.json --noEmit"},
            "dependencies":{"ws":"^8.22.0","ajv":"^8.20.0","ajv-draft-04":"^1.0.0","ajv-formats":"^3.0.1"},
            "devDependencies":{"typescript":"^5.9.3","@types/ws":"^8.18.1","@types/node":"^22.0.0"}
        }))?,
    )?;
    let config = format!("{directory}/tsconfig.json");
    if !root.join(&config).exists() {
        stage(
            files,
            config,
            pretty(
                &json!({"compilerOptions":{"strict":true,"target":"ES2022","module":"NodeNext","moduleResolution":"NodeNext","esModuleInterop":true,"declaration":true,"outDir":"dist","types":["node"]},"include":["src/**/*.ts"]}),
            )?,
        )?;
    }
    Ok(())
}
fn stage_rust(
    files: &mut BTreeMap<PathBuf, String>,
    name: &str,
    directory: &str,
    compiled: &Value,
) -> Result<()> {
    let mut index = String::from("#![forbid(unsafe_code)]\n");
    for (module, source) in sources(compiled, "rust")? {
        index.push_str(&format!("pub mod {module};\n"));
        stage(
            files,
            format!("{directory}/src/{module}.rs"),
            source.as_str().unwrap().into(),
        )?;
    }
    stage(files, format!("{directory}/src/lib.rs"), index)?;
    stage(
        files,
        format!("{directory}/Cargo.toml"),
        format!(
            r#"[package]
name = "{name}-websocket-sdk"
version = "0.1.0"
edition = "2024"

[dependencies]
forklaunch-ws = {{ path = "../../../.forklaunch/framework-rs/crates/forklaunch-ws", features = ["transport", "json-schema"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tokio = {{ version = "1", features = ["net"] }}
tokio-tungstenite = "0.30"
"#
        ),
    )?;
    Ok(())
}
fn stage_workspace(
    root: &Path,
    files: &mut BTreeMap<PathBuf, String>,
    members: &[String],
    target: &str,
) -> Result<()> {
    if !members.is_empty() {
        let mut cargo =
            fs::read_to_string(root.join("Cargo.toml"))?.parse::<toml_edit::DocumentMut>()?;
        let array = cargo["workspace"]["members"]
            .as_array_mut()
            .context("Root Cargo workspace members missing")?;
        for member in members {
            if !array.iter().any(|value| value.as_str() == Some(member)) {
                array.push(member);
            }
        }
        stage(files, "Cargo.toml", cargo.to_string())?;
    }
    if matches!(target, "typescript" | "all") {
        let content = match fs::read_to_string(root.join("pnpm-workspace.yaml")) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "packages: []\n".into(),
            Err(error) => return Err(error.into()),
        };
        let mut workspace: serde_yml::Value = serde_yml::from_str(&content)?;
        let packages = workspace
            .get_mut("packages")
            .and_then(serde_yml::Value::as_sequence_mut)
            .context("pnpm workspace packages must be an array")?;
        let sdk = serde_yml::Value::String("sdks/typescript-websocket/*".into());
        let updated = if packages.contains(&sdk) {
            content
        } else {
            packages.push(sdk);
            if let Some((offset, indent)) = content
                .split_inclusive('\n')
                .scan(0, |offset, line| {
                    let start = *offset;
                    *offset += line.len();
                    Some((start, line))
                })
                .find_map(|(offset, line)| {
                    (line.trim() == "packages:").then_some((
                        offset + line.len(),
                        line.len() - line.trim_start().len() + 2,
                    ))
                })
            {
                let mut updated = content;
                updated.insert_str(
                    offset,
                    &format!("{}- 'sdks/typescript-websocket/*'\n", " ".repeat(indent)),
                );
                updated
            } else {
                serde_yml::to_string(&workspace)?
            }
        };
        serde_yml::from_str::<serde_yml::Value>(&updated)
            .context("Invalid generated pnpm workspace")?;
        stage(files, "pnpm-workspace.yaml", updated)?;
    }
    Ok(())
}
fn validate_destination(root: &Path, relative: &Path) -> Result<()> {
    crate::core::application_output::output_path(root, relative).map(|_| ())
}

pub(crate) fn cleanup_service(root: &Path, name: &str) -> Result<bool> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("Invalid socket service name {name}");
    }
    ownership::cleanup_service(&root.canonicalize()?, name)
}

pub(crate) fn rename_service(
    root: &Path,
    old: &str,
    new: &str,
    stdout: &mut StandardStream,
) -> Result<()> {
    ownership::rename_service(&root.canonicalize()?, old, new, stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiled_module_names_cannot_choose_output_paths() {
        assert!(sources(&json!({"rust":{"../escape":"source"}}), "rust").is_err());
        assert!(sources(&json!({"rust":{"socket_ok":null}}), "rust").is_err());
        assert!(sources(&json!({"rust":{}}), "rust").is_err());
        assert!(sources(&json!({"rust":{"socket_type":"source"}}), "rust").is_ok());
    }
    #[test]
    fn workspace_registration_preserves_cargo_comments_and_other_packages() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(
            root.join("Cargo.toml"),
            "# user comment\n[workspace]\nmembers = [\"crates/chat\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "# user pnpm comment\npackages:\n  - crates/*\n",
        )
        .unwrap();
        let mut files = BTreeMap::new();
        stage_workspace(
            root,
            &mut files,
            &["sdks/rust/chat-websocket-sdk".into()],
            "all",
        )
        .unwrap();
        let cargo = &files[Path::new("Cargo.toml")];
        assert!(cargo.starts_with("# user comment"));
        assert!(cargo.contains("crates/chat"));
        assert!(cargo.contains("sdks/rust/chat-websocket-sdk"));
        let pnpm = &files[Path::new("pnpm-workspace.yaml")];
        assert!(pnpm.starts_with("# user pnpm comment"));
        assert!(pnpm.contains("crates/*"));
        assert!(pnpm.contains("sdks/typescript-websocket/*"));
    }
    #[test]
    fn normalized_output_paths_detect_collisions_before_writes() {
        let mut files = BTreeMap::new();
        stage(&mut files, "./custom/asyncapi.json", "first".into()).unwrap();
        assert!(stage(&mut files, "custom/asyncapi.json", "second".into()).is_err());
        assert!(stage(&mut files, "../outside.json", "invalid".into()).is_err());
        assert!(stage(&mut files, ".", "invalid".into()).is_err());
    }
    #[test]
    fn output_paths_stay_inside_the_application() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        assert!(validate_destination(&root, Path::new("sdks/rust/chat/src/lib.rs")).is_ok());
        assert!(validate_destination(&root, Path::new("../escape")).is_err());
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.join("sdks")).unwrap();
            assert!(validate_destination(&root, Path::new("sdks/chat/src/lib.rs")).is_err());
        }
    }
}
