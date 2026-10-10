use super::*;

pub(super) fn is_postgres_worker(root: &Path, name: &str) -> Result<bool> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    if project_table(&manifest, name)
        .and_then(|project| project.get("variant"))
        .and_then(Value::as_str)
        == Some("worker-postgres")
    {
        return Ok(true);
    }
    // Discover copied generated crates without replacing their application code.
    let directory = root.join("crates").join(name);
    Ok(directory.join("src/handler.rs").exists()
        && read_to_string(directory.join("src/main.rs"))
            .is_ok_and(|source| source.contains("PostgresWorkerQueue")))
}

pub(super) fn templates(
    crate_dir: &Path,
    name: &str,
    description: &str,
) -> Result<Vec<RenderedTemplate>> {
    let mut templates = worker_templates(crate_dir, name, description);
    let data = rust_crate_template_data(name, description, true);
    for template in &mut templates {
        if template.path == crate_dir.join("src/main.rs") {
            template.content = render_rust_template("rust/postgres-worker/main.rs", &data);
        } else if template.path == crate_dir.join("src/lib.rs") {
            template.content.push_str("\npub mod handler;\n");
        } else if template.path == crate_dir.join("Cargo.toml") {
            template.content = cargo_with_postgres(&template.content)?;
        }
    }
    for (output, source) in [
        ("src/handler.rs", "handler.rs"),
        (".env.example", "env.example"),
        ("README.md", "README.md"),
    ] {
        templates.push(RenderedTemplate {
            path: crate_dir.join(output),
            content: render_rust_template(&format!("rust/postgres-worker/{source}"), &data),
            context: None,
        });
    }
    Ok(templates
        .into_iter()
        .filter(|template| !template.path.exists())
        .collect())
}

fn cargo_with_postgres(content: &str) -> Result<String> {
    let mut cargo: toml_edit::DocumentMut = content.parse()?;
    for (name, path, features) in [
        (
            "forklaunch-worker-database",
            "../../.forklaunch/blueprint-rs/crates/implementations/worker/forklaunch-worker-database",
            vec!["postgres", "runtime"],
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
    let mut sqlx = toml_edit::InlineTable::new();
    sqlx.insert("version", "0.8".into());
    sqlx.insert("default-features", false.into());
    sqlx.insert(
        "features",
        toml_edit::Array::from_iter(["runtime-tokio", "tls-rustls", "postgres"]).into(),
    );
    cargo["dependencies"]["sqlx"] = toml_edit::value(sqlx);
    Ok(cargo.to_string())
}

pub(super) fn convert(root: &Path, name: &str) -> Result<()> {
    iam::ensure_blueprint_sources(root, false)?;
    let directory = root.join("crates").join(name);
    let cargo = directory.join("Cargo.toml");
    write(&cargo, cargo_with_postgres(&read_to_string(&cargo)?)?)?;
    let data = rust_crate_template_data(name, "Durable PostgreSQL worker", true);
    write(
        directory.join("src/main.rs"),
        render_rust_template("rust/postgres-worker/main.rs", &data),
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
                render_rust_template(&format!("rust/postgres-worker/{template}"), &data),
            )?;
        }
    }
    mark(root, name)?;
    compose(root, name)
}

pub(super) fn mark(root: &Path, name: &str) -> Result<()> {
    let path = root.join(".forklaunch/manifest.toml");
    let mut manifest = read_manifest_value(&path)?;
    project_table_mut(&mut manifest, name)
        .context("Expected worker project")?
        .insert("variant".into(), Value::String("worker-postgres".into()));
    write_manifest_value(&path, &manifest)
}

pub(super) fn compose(root: &Path, name: &str) -> Result<()> {
    ensure_rust_docker_compose_project(root, RustProjectKind::Worker, name, false)?;
    let path = root.join("docker-compose.yaml");
    let mut compose = read_or_create_rust_docker_compose(&path)?;
    let service = compose
        .services
        .get_mut(name)
        .context("Expected worker Compose service")?;
    let env = service.environment.get_or_insert_with(IndexMap::new);
    env.entry("DATABASE_URL".into())
        .or_insert_with(|| "${WORKER_DATABASE_URL:?Configure a restricted PostgreSQL role}".into());
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
