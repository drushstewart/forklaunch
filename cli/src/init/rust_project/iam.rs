use super::*;

static BLUEPRINT_CRATES: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/src/templates/rust/blueprint-workspace/crates");
const BLUEPRINT_CARGO: &str =
    include_str!("../../templates/rust/blueprint-workspace/Cargo.toml.template");

pub(crate) fn generate_rust_iam_module(matches: &ArgMatches) -> Result<()> {
    if option(matches, "module").as_deref() != Some("iam-base") {
        bail!("Rust module generation currently supports --module iam-base");
    }
    if option(matches, "database").is_some_and(|database| database != "postgresql") {
        bail!("Rust IAM requires --database postgresql");
    }
    let name = option(matches, "name").unwrap_or_else(|| "iam".to_owned());
    if !validate_name(&name) {
        bail!("Invalid IAM service name");
    }
    let root = find_manifest_root(
        &option(matches, "base_path")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?),
    )?;
    if !is_rust_app_root(&root)? {
        bail!("Rust IAM requires a Rust or mixed application");
    }
    let dryrun = matches.get_flag("dryrun");
    generate_iam_at(&root, &name, matches, dryrun)
}

pub(super) fn generate_iam_at(
    root: &Path,
    name: &str,
    matches: &ArgMatches,
    dryrun: bool,
) -> Result<()> {
    let manifest_path = root.join(".forklaunch/manifest.toml");
    let manifest = if manifest_path.exists() {
        read_manifest_value(&manifest_path)?
    } else {
        Value::Table(Map::new())
    };
    if let Some(project) = project_table(&manifest, &name) {
        if project.get("variant").and_then(Value::as_str) != Some("iam-base")
            || !is_rust_project(root, name)?
        {
            bail!("Project {name} already exists and is not an IAM module");
        }
    }
    let crate_path = Path::new("crates").join(&name);
    let crate_dir = root.join(&crate_path);
    if project_table(&manifest, name).is_none() && crate_dir.join("Cargo.toml").exists() {
        bail!(
            "An existing crate occupies the IAM destination: {}",
            crate_dir.display()
        );
    }
    let data = rust_crate_template_data(&name, "Persistent PostgreSQL IAM", true);
    let templates = [
        ("Cargo.toml", "rust/iam/Cargo.toml.template"),
        ("src/lib.rs", "rust/iam/lib.rs"),
        ("src/main.rs", "rust/iam/main.rs"),
        (".env.example", "rust/iam/env.example"),
        ("README.md", "rust/iam/README.md"),
    ]
    .into_iter()
    .map(|(output, template)| RenderedTemplate {
        path: crate_dir.join(output),
        content: render_rust_template(template, &data),
        context: None,
    })
    .filter(|template| !template.path.exists())
    .collect::<Vec<_>>();
    ensure_embedded_framework(&root, dryrun)?;
    ensure_blueprint_sources(&root, dryrun)?;
    write_rust_rendered_templates(
        &templates,
        dryrun,
        &mut StandardStream::stdout(ColorChoice::Always),
    )?;
    add_cargo_member(&root, &crate_path.to_string_lossy(), dryrun)?;
    add_project_to_manifest(
        &root,
        RustProjectKind::Service,
        &name,
        "Persistent PostgreSQL IAM",
        matches,
        dryrun,
    )?;
    if !dryrun {
        mark_iam_module(&root, &name)?;
    }
    ensure_iam_compose(&root, &name, dryrun)?;
    ensure_rust_package_scripts(&root, RustProjectKind::Service, &name, dryrun)?;
    Ok(())
}

pub(super) fn ensure_blueprint_sources(root: &Path, dryrun: bool) -> Result<()> {
    if dryrun {
        return Ok(());
    }
    let destination = root.join(".forklaunch/blueprint-rs");
    create_dir_all(&destination)?;
    let cargo = destination.join("Cargo.toml");
    if !cargo.exists() {
        write(cargo, BLUEPRINT_CARGO)?;
    }
    write_embedded_sources(&BLUEPRINT_CRATES, &destination.join("crates"))?;
    let cargo_path = root.join("Cargo.toml");
    let mut cargo: toml_edit::DocumentMut = read_to_string(&cargo_path)?.parse()?;
    if cargo["workspace"]["exclude"].is_none() {
        cargo["workspace"]["exclude"] = toml_edit::value(toml_edit::Array::new());
    }
    let excludes = cargo["workspace"]["exclude"]
        .as_array_mut()
        .context("Expected workspace.exclude array")?;
    if !excludes
        .iter()
        .any(|value| value.as_str() == Some(".forklaunch/blueprint-rs"))
    {
        excludes.push(".forklaunch/blueprint-rs");
    }
    write(cargo_path, cargo.to_string())?;
    Ok(())
}

fn mark_iam_module(root: &Path, name: &str) -> Result<()> {
    let path = root.join(".forklaunch/manifest.toml");
    let mut manifest = read_manifest_value(&path)?;
    let projects = manifest
        .get_mut("projects")
        .and_then(Value::as_array_mut)
        .context("Expected manifest projects")?;
    let project = projects
        .iter_mut()
        .find(|project| project.get("name").and_then(Value::as_str) == Some(name))
        .and_then(Value::as_table_mut)
        .context("Expected IAM project")?;
    project.insert("variant".to_owned(), Value::String("iam-base".to_owned()));
    write_manifest_value(&path, &manifest)
}

pub(super) fn is_iam_module(root: &Path, name: &str) -> Result<bool> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    Ok(project_table(&manifest, name)
        .and_then(|project| project.get("variant"))
        .and_then(Value::as_str)
        == Some("iam-base"))
}

pub(super) fn ensure_iam_compose(root: &Path, name: &str, dryrun: bool) -> Result<()> {
    if dryrun {
        return Ok(());
    }
    let path = root.join("docker-compose.yaml");
    let compose = read_or_create_rust_docker_compose(&path)?;
    if !compose.services.contains_key(name) {
        ensure_rust_docker_compose_project(root, RustProjectKind::Service, name, false)?;
    }
    let mut compose = read_or_create_rust_docker_compose(&path)?;
    let service = compose
        .services
        .get_mut(name)
        .context("Expected IAM Compose service")?;
    let env = service.environment.get_or_insert_with(IndexMap::new);
    env.entry("DATABASE_URL".to_owned()).or_insert_with(|| {
        "${IAM_DATABASE_URL:?Configure a restricted PostgreSQL role}".to_owned()
    });
    env.entry("TENANT_ID".to_owned())
        .or_insert_with(|| "${IAM_TENANT_ID:?Configure a trusted tenant}".to_owned());
    write(path, yaml_to_string(&compose)?)?;
    Ok(())
}

pub(super) fn rename_iam_sources(root: &Path, old: &str, new: &str) -> Result<()> {
    let crate_dir = root.join("crates").join(new);
    for filename in ["src/lib.rs", "src/main.rs"] {
        let path = crate_dir.join(filename);
        if path.exists() {
            let source = read_to_string(&path)?
                .replace(
                    &format!("{}::", old.to_case(Case::Snake)),
                    &format!("{}::", new.to_case(Case::Snake)),
                )
                .replace(
                    &format!("pub const SERVICE_NAME: &str = \"{old}\";"),
                    &format!("pub const SERVICE_NAME: &str = \"{new}\";"),
                );
            write(path, source)?;
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
