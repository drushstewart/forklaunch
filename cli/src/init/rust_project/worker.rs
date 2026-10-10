use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Backend {
    Postgres,
    Redis,
}
impl Backend {
    fn variant(self) -> &'static str {
        match self {
            Self::Postgres => "worker-postgres",
            Self::Redis => "worker-redis",
        }
    }
    fn template(self) -> &'static str {
        match self {
            Self::Postgres => "postgres-worker",
            Self::Redis => "redis-worker",
        }
    }
    pub(super) fn from_options(worker_type: &str, database: &str) -> Option<Self> {
        match (worker_type, database) {
            ("database", "postgresql") => Some(Self::Postgres),
            ("redis", _) => Some(Self::Redis),
            _ => None,
        }
    }
}

pub(super) fn backend(root: &Path, name: &str) -> Result<Option<Backend>> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    let variant = project_table(&manifest, name)
        .and_then(|project| project.get("variant"))
        .and_then(Value::as_str);
    match variant {
        Some("worker-postgres") => return Ok(Some(Backend::Postgres)),
        Some("worker-redis") => return Ok(Some(Backend::Redis)),
        _ => {}
    }
    let directory = root.join("crates").join(name);
    if directory.join("src/handler.rs").exists() {
        let source = read_to_string(directory.join("src/main.rs"))?;
        if source.contains("PostgresWorkerQueue") {
            return Ok(Some(Backend::Postgres));
        }
        if source.contains("RedisLeasedQueue") {
            return Ok(Some(Backend::Redis));
        }
    }
    Ok(None)
}

pub(super) fn templates(
    crate_dir: &Path,
    name: &str,
    description: &str,
    backend: Backend,
) -> Result<Vec<RenderedTemplate>> {
    let mut templates = worker_templates(crate_dir, name, description);
    let data = rust_crate_template_data(name, description, true);
    for template in &mut templates {
        if template.path == crate_dir.join("src/main.rs") {
            template.content =
                render_rust_template(&format!("rust/{}/main.rs", backend.template()), &data);
        } else if template.path == crate_dir.join("src/lib.rs") {
            template.content.push_str("\npub mod handler;\n");
        } else if template.path == crate_dir.join("Cargo.toml") {
            template.content = cargo_with_backend(&template.content, backend)?;
        }
    }
    for (output, source) in [
        ("src/handler.rs", "handler.rs"),
        (".env.example", "env.example"),
        ("README.md", "README.md"),
    ] {
        templates.push(RenderedTemplate {
            path: crate_dir.join(output),
            content: render_rust_template(&format!("rust/{}/{source}", backend.template()), &data),
            context: None,
        });
    }
    Ok(templates
        .into_iter()
        .filter(|template| !template.path.exists())
        .collect())
}

fn cargo_with_backend(content: &str, backend: Backend) -> Result<String> {
    let mut cargo: toml_edit::DocumentMut = content.parse()?;
    for (name, path, features) in [
        (
            "forklaunch-worker-database",
            "../../.forklaunch/blueprint-rs/crates/implementations/worker/forklaunch-worker-database",
            if backend == Backend::Postgres {
                vec!["postgres", "http-control"]
            } else {
                vec!["http-control"]
            },
        ),
        (
            "forklaunch-worker-interface",
            "../../.forklaunch/blueprint-rs/crates/interfaces/forklaunch-worker-interface",
            vec![],
        ),
        (
            "forklaunch-blueprint-core",
            "../../.forklaunch/blueprint-rs/crates/forklaunch-blueprint-core",
            vec![],
        ),
    ] {
        let mut dependency = toml_edit::InlineTable::new();
        dependency.insert("path", path.into());
        if !features.is_empty() {
            dependency.insert("features", toml_edit::Array::from_iter(features).into());
        }
        cargo["dependencies"][name] = toml_edit::value(dependency);
    }
    cargo["dependencies"]["serde_json"] = toml_edit::value("1");
    if backend == Backend::Redis {
        let mut redis = toml_edit::InlineTable::new();
        redis.insert(
            "path",
            "../../.forklaunch/blueprint-rs/crates/implementations/worker/forklaunch-worker-redis"
                .into(),
        );
        redis.insert(
            "features",
            toml_edit::Array::from_iter(["redis-client"]).into(),
        );
        cargo["dependencies"]["forklaunch-worker-redis"] = toml_edit::value(redis);
    } else {
        let mut sqlx = toml_edit::InlineTable::new();
        sqlx.insert("version", "0.8".into());
        sqlx.insert("default-features", false.into());
        sqlx.insert(
            "features",
            toml_edit::Array::from_iter(["runtime-tokio", "tls-rustls", "postgres"]).into(),
        );
        cargo["dependencies"]["sqlx"] = toml_edit::value(sqlx);
    }
    Ok(cargo.to_string())
}

pub(super) fn convert(root: &Path, name: &str, backend: Backend) -> Result<()> {
    iam::ensure_blueprint_sources(root, false)?;
    let directory = root.join("crates").join(name);
    let cargo = directory.join("Cargo.toml");
    write(
        &cargo,
        cargo_with_backend(&read_to_string(&cargo)?, backend)?,
    )?;
    let data = rust_crate_template_data(name, "Durable worker", true);
    write(
        directory.join("src/main.rs"),
        render_rust_template(&format!("rust/{}/main.rs", backend.template()), &data),
    )?;
    let library = directory.join("src/lib.rs");
    let mut source = read_to_string(&library)?;
    if !source.contains("pub mod handler;") {
        source.push_str("\npub mod handler;\n");
        write(library, source)?;
    }
    // Handler code and user documentation survive backend changes.
    for (output, template) in [
        ("src/handler.rs", "handler.rs"),
        (".env.example", "env.example"),
        ("README.md", "README.md"),
    ] {
        let path = directory.join(output);
        if !path.exists() {
            write(
                path,
                render_rust_template(&format!("rust/{}/{template}", backend.template()), &data),
            )?;
        }
    }
    mark(root, name, backend)?;
    compose(root, name, backend)
}

pub(super) fn mark(root: &Path, name: &str, backend: Backend) -> Result<()> {
    let path = root.join(".forklaunch/manifest.toml");
    let mut manifest = read_manifest_value(&path)?;
    let project = project_table_mut(&mut manifest, name).context("Expected worker project")?;
    project.insert("variant".into(), Value::String(backend.variant().into()));
    let metadata = project
        .entry("metadata")
        .or_insert_with(|| Value::Table(Map::new()))
        .as_table_mut()
        .context("Expected worker metadata")?;
    metadata.insert(
        "workerType".into(),
        Value::String(
            match backend {
                Backend::Postgres => "database",
                Backend::Redis => "redis",
            }
            .into(),
        ),
    );
    let resources = project
        .entry("resources")
        .or_insert_with(|| Value::Table(Map::new()))
        .as_table_mut()
        .context("Expected worker resources")?;
    for key in ["database", "cache", "queue"] {
        resources.remove(key);
    }
    let (resource, value) = match backend {
        Backend::Postgres => ("database", "postgresql"),
        Backend::Redis => ("cache", "redis"),
    };
    resources.insert(resource.into(), Value::String(value.into()));
    write_manifest_value(&path, &manifest)
}

pub(super) fn compose(root: &Path, name: &str, backend: Backend) -> Result<()> {
    ensure_rust_docker_compose_project(root, RustProjectKind::Worker, name, false)?;
    let path = root.join("docker-compose.yaml");
    let mut compose = read_or_create_rust_docker_compose(&path)?;
    let service = compose
        .services
        .get_mut(name)
        .context("Expected worker Compose service")?;
    let env = service.environment.get_or_insert_with(IndexMap::new);
    match backend {
        Backend::Postgres => {
            env.entry("DATABASE_URL".into()).or_insert_with(|| {
                "${WORKER_DATABASE_URL:?Configure a restricted PostgreSQL role}".into()
            });
        }
        Backend::Redis => {
            env.entry("REDIS_URL".into())
                .or_insert_with(|| "${WORKER_REDIS_URL:?Configure Redis}".into());
            env.entry("WORKER_NAMESPACE".into())
                .or_insert_with(|| "${WORKER_NAMESPACE:?Configure a worker namespace}".into());
        }
    }
    env.entry("TENANT_ID".into())
        .or_insert_with(|| "${WORKER_TENANT_ID:?Configure a trusted tenant}".into());
    write(path, yaml_to_string(&compose)?)?;
    Ok(())
}

pub(super) fn rename_sources(root: &Path, old: &str, new: &str) -> Result<()> {
    let directory = root.join("crates").join(new);
    for relative in ["src/main.rs", "tests/smoke.rs"] {
        let path = directory.join(relative);
        if path.exists() {
            let source = read_to_string(&path)?.replace(
                &format!("use {}::", old.to_case(Case::Snake)),
                &format!("use {}::", new.to_case(Case::Snake)),
            );
            write(path, source)?;
        }
    }
    for relative in ["src/lib.rs", "src/config/mod.rs"] {
        let path = directory.join(relative);
        if path.exists() {
            write(
                &path,
                read_to_string(&path)?.replace(&format!("\"{old}\""), &format!("\"{new}\"")),
            )?;
        }
    }
    let path = root.join("docker-compose.yaml");
    let mut compose = read_or_create_rust_docker_compose(&path)?;
    if let Some(mut service) = compose.services.shift_remove(old) {
        service.hostname = Some(new.to_owned());
        service.command = Some(DockerCommand::Multiple(vec![format!(
            "./target/release/{new}"
        )]));
        compose.services.insert(new.to_owned(), service);
        write(path, yaml_to_string(&compose)?)?;
    }
    Ok(())
}
