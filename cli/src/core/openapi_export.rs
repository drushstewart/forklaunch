use std::{
    env, fs,
    io::Read as _,
    path::Path,
    process::{Command as ProcessCommand, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};

/// Resolve a command name to its full path by searching PATH.
/// Falls back to the bare command name if not found (lets the OS try).
pub(crate) fn resolve_command(name: &str) -> String {
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                if let Some(s) = candidate.to_str() {
                    return s.to_string();
                }
            }
        }
    }

    // Common install locations as fallback
    let home = env::var("HOME").unwrap_or_default();
    let mut fallback_paths = vec![
        format!("{}/Library/pnpm/{}", home, name),
        format!("{}/.local/share/pnpm/{}", home, name),
        format!("{}/.corepack/bin/{}", home, name),
        format!("/opt/homebrew/bin/{}", name),
        format!("/usr/local/bin/{}", name),
        format!("{}/.bun/bin/{}", home, name),
    ];

    // Check nvm directories for node/npm/npx
    if let Ok(nvm_dir) = env::var("NVM_DIR").or_else(|_| Ok::<String, ()>(format!("{}/.nvm", home)))
    {
        let nvm_versions = Path::new(&nvm_dir).join("versions").join("node");
        if nvm_versions.is_dir() {
            // Find the latest installed node version
            if let Ok(entries) = fs::read_dir(&nvm_versions) {
                let mut versions: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .collect();
                versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
                for version_entry in versions {
                    fallback_paths.push(format!("{}/bin/{}", version_entry.path().display(), name));
                }
            }
        }
    }

    for path in &fallback_paths {
        if Path::new(path).is_file() {
            return path.clone();
        }
    }

    name.to_string()
}

use crate::{
    constants::Runtime,
    core::{
        ast::infrastructure::env::{EnvVarUsage, find_all_env_vars},
        manifest::{ProjectType, application::ApplicationManifestData},
        rendered_template::RenderedTemplatesCache,
    },
};

fn generate_dummy_value(var_name: &str, var_type: &str, iam_port: Option<u16>) -> String {
    match var_type {
        "string" => {
            if var_name.ends_with("_PATH") || var_name.ends_with("_FILE") {
                "/dev/null".to_string()
            } else if var_name.contains("DB_URL") {
                "postgresql://dummy:dummy@localhost:5432/dummy".to_string()
            } else if var_name.contains("REDIS_URL") {
                "redis://localhost:6379".to_string()
            } else if var_name.contains("KAFKA") {
                "localhost:9092".to_string()
            } else if var_name == "HOST" {
                "localhost".to_string()
            } else if var_name == "PROTOCOL" {
                "http".to_string()
            } else if var_name == "VERSION" {
                "v1".to_string()
            } else if var_name.contains("DOCS_PATH") {
                "/docs".to_string()
            } else if var_name.contains("OTEL_EXPORTER") {
                "http://localhost:4318".to_string()
            } else if var_name.contains("OTEL_SERVICE_NAME") {
                "service".to_string()
            } else if var_name.contains("OTEL_LEVEL") {
                "info".to_string()
            } else if var_name == "NODE_ENV" {
                "development".to_string()
            } else if var_name.contains("S3_BUCKET") || var_name.contains("BUCKET_NAME") {
                "dummy-bucket".to_string()
            } else if var_name.contains("S3_REGION") || var_name.contains("AWS_REGION") {
                "us-east-1".to_string()
            } else if var_name == "JWKS_PUBLIC_KEY_URL" {
                if let Some(port) = iam_port {
                    format!("http://localhost:{}/api/auth/jwks", port)
                } else {
                    "dummy-jwks-url".to_string()
                }
            } else if var_name.contains("URL") {
                "http://localhost:3000".to_string()
            } else if var_name.contains("_SECRET")
                || var_name.contains("_KEY")
                || var_name.starts_with("SECRET")
                || var_name.starts_with("KEY")
            {
                if var_name == "HMAC_SECRET_KEY" {
                    format!("{:x}", {
                        use std::{
                            collections::hash_map::DefaultHasher,
                            hash::{Hash, Hasher},
                        };
                        let mut hasher = DefaultHasher::new();
                        var_name.hash(&mut hasher);
                        hasher.finish()
                    })
                } else {
                    "dummy-secret-key".to_string()
                }
            } else {
                "1".to_string()
            }
        }
        "number" => {
            if var_name.contains("PORT") {
                "3000".to_string()
            } else {
                "1".to_string()
            }
        }
        "boolean" => "true".to_string(),
        _ => "1".to_string(),
    }
}

pub(crate) fn export_service_openapi(
    service_path: &Path,
    service_name: &str,
    output_file: &Path,
    runtime: &str,
    env_vars: &[EnvVarUsage],
    iam_port: Option<u16>,
) -> Result<()> {
    if !service_path.join("package.json").exists() {
        bail!("No package.json found in {}", service_path.display());
    }

    let tsconfig_path = service_path.join("tsconfig.json");

    // Resolve the runtime binary and build the command.
    // For Node, run node directly with tsx's CLI module to avoid shebang PATH issues
    // (e.g. `#!/usr/bin/env node` fails when node isn't in PATH).
    let (resolved_bin, args) = match runtime.parse::<Runtime>()? {
        Runtime::Bun => {
            let bun = resolve_command("bun");
            (bun, vec!["server.ts".to_string()])
        }
        Runtime::Node => {
            let node = resolve_command("node");
            let tsx_cli = service_path
                .join("node_modules/tsx/dist/cli.mjs")
                .to_string_lossy()
                .to_string();

            if !Path::new(&tsx_cli).exists() {
                bail!(
                    "tsx not found at {} for service {}. Run pnpm install first.",
                    tsx_cli,
                    service_name
                );
            }

            (
                node,
                vec![
                    tsx_cli,
                    "--tsconfig".to_string(),
                    tsconfig_path.to_str().unwrap().to_string(),
                    "server.ts".to_string(),
                ],
            )
        }
    };

    let mut cmd = ProcessCommand::new(&resolved_bin);
    cmd.args(&args).current_dir(service_path);

    for env_var in env_vars {
        let dummy_value = generate_dummy_value(&env_var.var_name, "string", iam_port);
        cmd.env(&env_var.var_name, &dummy_value);
    }

    // Application scans may include these variables. The explicit export mode
    // and destination must take precedence over generated dummy values.
    cmd.env("FORKLAUNCH_MODE", "openapi")
        .env("FORKLAUNCH_OPENAPI_OUTPUT", output_file);

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn {} command", resolved_bin))?;

    let timeout = Duration::from_secs(60);
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let stderr = child
                        .stderr
                        .take()
                        .map(|mut s| {
                            let mut buf = String::new();
                            s.read_to_string(&mut buf).ok();
                            buf
                        })
                        .unwrap_or_default();
                    bail!("Service {} failed to export: {}", service_name, stderr);
                }
                break;
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    bail!(
                        "Service {} timed out after {}s during OpenAPI export. \
                        This usually means server.ts has top-level await calls (e.g. universalSdk, DB connections) \
                        that block before the FORKLAUNCH_MODE check in listen(). \
                        Wrap them in a guard: if (process.env.FORKLAUNCH_MODE !== 'openapi') {{ ... }}",
                        service_name,
                        timeout.as_secs()
                    );
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => bail!("Error waiting for {} subprocess: {}", service_name, e),
        }
    }

    if !output_file.exists() {
        bail!(
            "OpenAPI file not created at {:?} for service {}",
            output_file,
            service_name
        );
    }

    Ok(())
}

pub(crate) fn export_all_services(
    app_root: &Path,
    manifest: &ApplicationManifestData,
    output_dir: &Path,
) -> Result<Vec<String>> {
    let mut exported_services = export_all_rust_services(app_root, output_dir)?;

    // Tokio is the Rust runtime; TypeScript services in that mixed workspace
    // still export through Node. Bun/Node manifests keep their selected runtime.
    let runtime = if manifest.runtime == "tokio" {
        "node"
    } else {
        manifest.runtime.as_str()
    };
    let modules_path = app_root.join(&manifest.modules_path);

    let rendered_templates_cache = RenderedTemplatesCache::new();
    let all_env_vars = find_all_env_vars(&modules_path, &rendered_templates_cache)?;

    // Include both services and workers (workers have a server component)
    // Exclude observability services as they are not user application components
    const OBSERVABILITY_SERVICES: &[&str] = &[
        "mimir",
        "loki",
        "tempo",
        "grafana",
        "otel",
        "otel-collector",
        "memcached",
        "minio",
        "minio-init",
    ];

    let services: Vec<_> = manifest
        .projects
        .iter()
        .filter(|p| {
            (p.r#type == ProjectType::Service || p.r#type == ProjectType::Worker)
                && !OBSERVABILITY_SERVICES.contains(&p.name.as_str())
                && p.metadata.as_ref().and_then(|m| m.language.as_deref()) != Some("rust")
        })
        .collect();

    // Find IAM service to get its port
    let iam_port = manifest
        .projects
        .iter()
        .find(|p| {
            p.r#type == ProjectType::Service && (p.name == "iam" || p.name == "iam-better-auth")
        })
        .and_then(|iam_project| {
            // Try to get port from docker-compose by looking at services
            let docker_compose_path = app_root.join("docker-compose.yaml");
            if docker_compose_path.exists() {
                if let Ok(content) = fs::read_to_string(&docker_compose_path) {
                    if let Ok(compose) = serde_yml::from_str::<serde_yml::Value>(&content) {
                        if let Some(services) = compose.get("services") {
                            if let Some(i_services) = services.as_mapping() {
                                if let Some(iam) = i_services.get(&iam_project.name) {
                                    if let Some(ports) = iam.get("ports") {
                                        if let Some(port_str) = ports[0].as_str() {
                                            if let Some(host_port) = port_str.split(':').next() {
                                                if let Ok(port) = host_port.parse::<u16>() {
                                                    return Some(port);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            None
        });

    for service in services {
        let service_path = app_root.join(&manifest.modules_path).join(&service.name);
        let service_output_dir = output_dir.join(&service.name);

        fs::create_dir_all(&service_output_dir)
            .with_context(|| format!("Failed to create directory: {:?}", service_output_dir))?;

        let openapi_file = service_output_dir.join("openapi.json");

        let service_env_vars = all_env_vars
            .get(&service.name)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        export_service_openapi(
            &service_path,
            &service.name,
            &openapi_file,
            runtime,
            service_env_vars,
            iam_port,
        )?;

        exported_services.push(service.name.clone());
    }

    Ok(exported_services)
}

pub(crate) fn export_all_rust_services(app_root: &Path, output_dir: &Path) -> Result<Vec<String>> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let manifest_content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest = toml::from_str::<toml::Value>(&manifest_content)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    let projects = manifest
        .get("projects")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut exported_services = Vec::new();
    for project in projects {
        let project_type = project.get("type").and_then(toml::Value::as_str);
        if !matches!(project_type, Some("Service") | Some("Worker")) {
            continue;
        }

        let Some(service_name) = project.get("name").and_then(toml::Value::as_str) else {
            continue;
        };

        let metadata = project.get("metadata").and_then(toml::Value::as_table);
        if metadata
            .and_then(|metadata| metadata.get("language"))
            .and_then(toml::Value::as_str)
            != Some("rust")
        {
            continue;
        }

        let service_output_dir = output_dir.join(service_name);
        fs::create_dir_all(&service_output_dir)
            .with_context(|| format!("Failed to create directory: {:?}", service_output_dir))?;
        let output_file = service_output_dir.join("openapi.json");
        let contract_path = metadata
            .and_then(|metadata| metadata.get("contractArtifactPath"))
            .and_then(toml::Value::as_str)
            .map_or_else(
                || {
                    app_root
                        .join(".forklaunch")
                        .join("contracts")
                        .join(service_name)
                        .join("contract.json")
                },
                |path| app_root.join(path),
            );
        if let Some(contract_dir) = contract_path.parent() {
            fs::create_dir_all(contract_dir)
                .with_context(|| format!("Failed to create {}", contract_dir.display()))?;
        }
        let crate_path = metadata
            .and_then(|metadata| metadata.get("cratePath"))
            .and_then(toml::Value::as_str)
            .map(|path| app_root.join(path))
            .unwrap_or_else(|| app_root.join("crates").join(service_name));

        let artifact = export_rust_service_artifacts(
            app_root,
            service_name,
            &crate_path,
            &output_file,
            &contract_path,
        )?;

        export_registered_sockets(app_root, service_name, metadata, &artifact)?;

        exported_services.push(service_name.to_string());
    }

    Ok(exported_services)
}

fn export_rust_service_artifacts(
    app_root: &Path,
    service_name: &str,
    crate_path: &Path,
    openapi_output: &Path,
    contract_output: &Path,
) -> Result<serde_json::Value> {
    if !crate_path.join("Cargo.toml").exists() {
        bail!(
            "Rust project {} does not have a Cargo.toml at {}",
            service_name,
            crate_path.display()
        );
    }

    let cargo = resolve_command("cargo");
    let mut child = ProcessCommand::new(&cargo)
        .args(["run", "-p", service_name, "--quiet"])
        .current_dir(app_root)
        .env("FORKLAUNCH_MODE", "contract")
        .env("FORKLAUNCH_OPENAPI_OUTPUT", openapi_output)
        .env("FORKLAUNCH_CONTRACT_OUTPUT", contract_output)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn cargo for Rust service {service_name}"))?;

    let stderr = child.stderr.take();
    let stderr_reader = thread::spawn(move || read_child_pipe(stderr));
    // A first contract export may compile the generated service and its dependencies.
    let timeout = Duration::from_secs(300);
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = stderr_reader.join().unwrap_or_default();
                if !status.success() {
                    bail!(
                        "Rust service {} failed to export framework contract artifacts: {}",
                        service_name,
                        stderr
                    );
                }
                break;
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!(
                        "Rust service {} timed out after {}s during framework contract export",
                        service_name,
                        timeout.as_secs()
                    );
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => bail!("Error waiting for Rust service {}: {}", service_name, error),
        }
    }

    if !openapi_output.exists() {
        bail!(
            "Rust OpenAPI file was not created at {} for service {}",
            openapi_output.display(),
            service_name
        );
    }
    if !contract_output.exists() {
        bail!(
            "Rust contract artifact was not created at {} for service {}",
            contract_output.display(),
            service_name
        );
    }

    let contract = fs::read_to_string(contract_output)
        .with_context(|| format!("Failed to read {}", contract_output.display()))?;
    let artifact: serde_json::Value =
        serde_json::from_str(&contract).context("Invalid exported Rust contract artifact")?;
    if artifact["generator"] != "framework-rs" {
        bail!(
            "Rust contract artifact for {} was not generated by framework-rs",
            service_name
        );
    }
    Ok(artifact)
}

pub(crate) fn export_registered_sockets(
    app_root: &Path,
    service_name: &str,
    metadata: Option<&toml::map::Map<String, toml::Value>>,
    artifact: &serde_json::Value,
) -> Result<()> {
    let root = app_root.canonicalize()?;
    let marker = crate::core::application_output::output_path(
        &root,
        Path::new(&format!(
            ".forklaunch/socket-contracts/{service_name}/registry-export.json"
        )),
    )?;
    let previous = read_socket_export_marker(&marker)?;
    let sockets = artifact
        .get("sockets")
        .map(|sockets| {
            sockets
                .as_object()
                .context("Exported socket contracts must be an object")
        })
        .transpose()?;
    if sockets.is_none_or(serde_json::Map::is_empty) {
        cleanup_socket_export(&root, previous.as_ref(), None)?;
        if previous.is_some() {
            fs::remove_file(marker)?;
        }
        return Ok(());
    }
    let sockets = sockets.unwrap();
    let relative = metadata
        .and_then(|metadata| metadata.get("socketContractArtifactPath"))
        .and_then(toml::Value::as_str)
        .map_or_else(
            || format!(".forklaunch/socket-contracts/{service_name}/socket-contract.json"),
            str::to_owned,
        );
    let output = crate::core::application_output::output_path(app_root, Path::new(&relative))?;
    let service = artifact
        .get("service")
        .context("Socket contract export requires service metadata")?;
    let title = service
        .get("name")
        .and_then(serde_json::Value::as_str)
        .context("Socket service name missing")?;
    let version = service
        .get("version")
        .and_then(serde_json::Value::as_str)
        .context("Socket service version missing")?;
    let portable =
        serde_json::json!({"formatVersion":1,"title":title,"version":version,"sockets":sockets});
    let content = format!("{}\n", serde_json::to_string_pretty(&portable)?);
    if content.len() > 16 * 1024 * 1024 {
        bail!("Socket contract artifact exceeds 16 MiB");
    }
    if output == marker
        || output
            .canonicalize()
            .ok()
            .zip(marker.canonicalize().ok())
            .is_some_and(|(output, marker)| output == marker)
    {
        bail!("Socket artifact path conflicts with registry export metadata");
    }
    fs::create_dir_all(output.parent().context("Socket output parent missing")?)?;
    fs::write(&output, &content)?;
    cleanup_socket_export(&root, previous.as_ref(), Some(&output))?;
    let ownership = serde_json::json!({"formatVersion":1,"outputPath":output.strip_prefix(&root)?,"sha256":socket_export_digest(content.as_bytes())});
    fs::create_dir_all(marker.parent().context("Socket marker parent missing")?)?;
    fs::write(
        marker,
        format!("{}\n", serde_json::to_string_pretty(&ownership)?),
    )?;
    Ok(())
}

pub(crate) fn rename_registered_sockets(
    app_root: &Path,
    old: &str,
    new: &str,
    metadata: Option<&toml::map::Map<String, toml::Value>>,
    artifact: Option<&serde_json::Value>,
) -> Result<bool> {
    let root = app_root.canonicalize()?;
    let marker = crate::core::application_output::output_path(
        &root,
        Path::new(&format!(
            ".forklaunch/socket-contracts/{old}/registry-export.json"
        )),
    )?;
    let Some(previous) = read_socket_export_marker(&marker)? else {
        return Ok(false);
    };
    let artifact =
        artifact.context("Registered socket rename requires its exported framework contract")?;
    export_registered_sockets(&root, new, metadata, artifact)?;
    let output = metadata
        .and_then(|metadata| metadata.get("socketContractArtifactPath"))
        .and_then(toml::Value::as_str)
        .map_or_else(
            || format!(".forklaunch/socket-contracts/{new}/socket-contract.json"),
            str::to_owned,
        );
    let output = crate::core::application_output::output_path(&root, Path::new(&output))?;
    cleanup_socket_export(&root, Some(&previous), Some(&output))?;
    fs::remove_file(&marker)?;
    if let Some(parent) = marker.parent() {
        if parent.canonicalize().ok().as_deref() == Some(parent) {
            match fs::remove_dir(parent) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(true)
}

fn socket_export_digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn read_socket_export_marker(marker: &Path) -> Result<Option<serde_json::Value>> {
    let metadata = match fs::metadata(marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.len() > 4096 {
        bail!("Invalid registry socket export metadata");
    }
    let value: serde_json::Value = serde_json::from_slice(&fs::read(marker)?)?;
    if value["formatVersion"] != 1
        || !value["outputPath"].is_string()
        || value["sha256"].as_str().is_none_or(|hash| {
            hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        bail!("Invalid registry socket export metadata");
    }
    Ok(Some(value))
}
fn cleanup_socket_export(
    root: &Path,
    previous: Option<&serde_json::Value>,
    keep: Option<&Path>,
) -> Result<()> {
    let Some(previous) = previous else {
        return Ok(());
    };
    let output = crate::core::application_output::output_path(
        root,
        Path::new(
            previous["outputPath"]
                .as_str()
                .context("Socket export path missing")?,
        ),
    )?;
    if keep.is_some_and(|keep| {
        keep == output
            || keep
                .canonicalize()
                .ok()
                .zip(output.canonicalize().ok())
                .is_some_and(|(keep, output)| keep == output)
    }) {
        return Ok(());
    }
    let metadata = match fs::metadata(&output) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    // Only remove the exact bounded bytes previously written by this exporter.
    if metadata.is_file()
        && metadata.len() <= 16 * 1024 * 1024
        && socket_export_digest(&fs::read(&output)?) == previous["sha256"].as_str().unwrap()
    {
        fs::remove_file(output)?;
    }
    Ok(())
}

fn read_child_pipe(pipe: Option<impl std::io::Read>) -> String {
    pipe.map(|mut output| {
        let mut buffer = String::new();
        let _ = output.read_to_string(&mut buffer);
        buffer
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod socket_export_tests {
    use serde_json::{Value, json};

    use super::*;

    fn registered() -> Value {
        json!({"service":{"name":"Realtime 水","version":"2.0.0"},"sockets":{"chat":{"path":"/chat","clientMessages":{"request":{"shape":{"type":"string","const":"water 水"}}}}}})
    }

    #[test]
    fn registry_exports_portable_artifacts_with_default_and_custom_destinations() {
        let root = tempfile::tempdir().unwrap();
        let artifact = registered();
        export_registered_sockets(root.path(), "realtime", None, &artifact).unwrap();
        let output = root
            .path()
            .join(".forklaunch/socket-contracts/realtime/socket-contract.json");
        let exported: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(exported["formatVersion"], 1);
        assert_eq!(exported["title"], artifact["service"]["name"]);
        assert_eq!(exported["version"], "2.0.0");
        assert_eq!(exported["sockets"], artifact["sockets"]);
        let metadata: toml::Table =
            toml::from_str("socketContractArtifactPath = './custom/socket.json'").unwrap();
        export_registered_sockets(root.path(), "realtime", Some(&metadata), &artifact).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("custom/socket.json")).unwrap(),
            format!("{}\n", serde_json::to_string_pretty(&exported).unwrap())
        );
    }

    #[test]
    fn services_without_registered_sockets_preserve_manual_socket_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(".forklaunch/socket-contracts/realtime");
        fs::create_dir_all(&directory).unwrap();
        let output = directory.join("socket-contract.json");
        fs::write(&output, "manual artifact").unwrap();
        export_registered_sockets(root.path(), "realtime", None, &json!({"routes":[]})).unwrap();
        assert_eq!(fs::read_to_string(output).unwrap(), "manual artifact");
    }

    #[test]
    fn registry_rename_updates_custom_output_ownership_without_removing_active_file() {
        let root = tempfile::tempdir().unwrap();
        let metadata: toml::Table =
            toml::from_str("socketContractArtifactPath = 'custom/socket.json'").unwrap();
        let mut artifact = registered();
        export_registered_sockets(root.path(), "old", Some(&metadata), &artifact).unwrap();
        artifact["service"]["name"] = json!("new");
        assert!(
            rename_registered_sockets(root.path(), "old", "new", Some(&metadata), Some(&artifact))
                .unwrap()
        );
        let output: Value =
            serde_json::from_slice(&fs::read(root.path().join("custom/socket.json")).unwrap())
                .unwrap();
        assert_eq!(output["title"], "new");
        assert!(
            !root
                .path()
                .join(".forklaunch/socket-contracts/old")
                .exists()
        );
        assert!(
            root.path()
                .join(".forklaunch/socket-contracts/new/registry-export.json")
                .exists()
        );
        export_registered_sockets(root.path(), "new", None, &json!({"sockets":{}})).unwrap();
        assert!(!root.path().join("custom/socket.json").exists());
    }

    #[test]
    fn registry_export_removes_only_unchanged_owned_files_on_move_or_removal() {
        let root = tempfile::tempdir().unwrap();
        let artifact = registered();
        let default = root
            .path()
            .join(".forklaunch/socket-contracts/realtime/socket-contract.json");
        let marker = root
            .path()
            .join(".forklaunch/socket-contracts/realtime/registry-export.json");
        export_registered_sockets(root.path(), "realtime", None, &artifact).unwrap();
        assert!(default.exists());
        assert!(marker.exists());
        let metadata: toml::Table =
            toml::from_str("socketContractArtifactPath = 'custom/socket.json'").unwrap();
        export_registered_sockets(root.path(), "realtime", Some(&metadata), &artifact).unwrap();
        assert!(!default.exists());
        let custom = root.path().join("custom/socket.json");
        assert!(custom.exists());
        export_registered_sockets(
            root.path(),
            "realtime",
            Some(&metadata),
            &json!({"routes":[]}),
        )
        .unwrap();
        assert!(!custom.exists());
        assert!(!marker.exists());
        export_registered_sockets(root.path(), "realtime", None, &artifact).unwrap();
        fs::write(&default, "manually edited metadata").unwrap();
        export_registered_sockets(root.path(), "realtime", None, &json!({"sockets":{}})).unwrap();
        assert_eq!(
            fs::read_to_string(default).unwrap(),
            "manually edited metadata"
        );
        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn registry_output_aliases_preserve_the_active_artifact() {
        let root = tempfile::tempdir().unwrap();
        let artifact = registered();
        export_registered_sockets(root.path(), "realtime", None, &artifact).unwrap();
        let output = root
            .path()
            .join(".forklaunch/socket-contracts/realtime/socket-contract.json");
        std::os::unix::fs::symlink(&output, root.path().join("alias.json")).unwrap();
        let metadata: toml::Table =
            toml::from_str("socketContractArtifactPath = 'alias.json'").unwrap();
        export_registered_sockets(root.path(), "realtime", Some(&metadata), &artifact).unwrap();
        assert_eq!(
            fs::read(root.path().join("alias.json")).unwrap(),
            fs::read(output).unwrap()
        );
    }

    #[test]
    fn registry_exports_reject_external_paths_and_symlinks_before_writing() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let artifact = registered();
        for path in [
            outside.path().join("socket.json"),
            Path::new("../escape.json").to_path_buf(),
        ] {
            let metadata: toml::Table = toml::from_str(&format!(
                "socketContractArtifactPath = {}",
                serde_json::to_string(&path).unwrap()
            ))
            .unwrap();
            assert!(
                export_registered_sockets(root.path(), "realtime", Some(&metadata), &artifact)
                    .is_err()
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
            let metadata: toml::Table =
                toml::from_str("socketContractArtifactPath = 'linked/socket.json'").unwrap();
            assert!(
                export_registered_sockets(root.path(), "realtime", Some(&metadata), &artifact)
                    .is_err()
            );
        }
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
