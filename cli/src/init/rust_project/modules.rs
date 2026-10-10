use super::*;

static BLUEPRINT_CRATES: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/src/templates/rust/blueprint-workspace/crates");
const BLUEPRINT_CARGO: &str =
    include_str!("../../templates/rust/blueprint-workspace/Cargo.toml.template");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinModule {
    Iam,
    Cac,
    MessagingBase,
    MessagingTwilio,
}

impl BuiltinModule {
    pub(super) fn from_variant(variant: &str) -> Option<Self> {
        match variant {
            "iam-base" => Some(Self::Iam),
            "cac-base" => Some(Self::Cac),
            "messaging-base" => Some(Self::MessagingBase),
            "messaging-twilio" => Some(Self::MessagingTwilio),
            _ => None,
        }
    }
    fn variant(self) -> &'static str {
        match self {
            Self::Iam => "iam-base",
            Self::Cac => "cac-base",
            Self::MessagingBase => "messaging-base",
            Self::MessagingTwilio => "messaging-twilio",
        }
    }
    pub(super) fn template(self) -> &'static str {
        match self {
            Self::Iam => "iam",
            Self::Cac => "cac",
            Self::MessagingBase | Self::MessagingTwilio => "messaging",
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Iam => "Persistent PostgreSQL IAM",
            Self::Cac => "CAC code-set service",
            Self::MessagingBase => "Persistent messaging hooks",
            Self::MessagingTwilio => "Persistent Twilio SMS messaging",
        }
    }
}

#[derive(Content)]
struct RustModuleTemplate {
    name: String,
    description: String,
    crate_module: String,
    twilio: bool,
}

pub(crate) fn generate_rust_module(matches: &ArgMatches) -> Result<()> {
    let kind = option(matches, "module")
        .as_deref()
        .and_then(BuiltinModule::from_variant)
        .context("Rust module generation currently supports --module iam-base, cac-base, messaging-base or messaging-twilio")?;
    if option(matches, "database").is_some_and(|database| database != "postgresql") {
        bail!("Rust modules currently support --database postgresql");
    }
    let name = option(matches, "name").unwrap_or_else(|| kind.template().to_owned());
    if !validate_name(&name) {
        bail!("Invalid module service name");
    }
    let root = find_manifest_root(
        &option(matches, "base_path")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?),
    )?;
    if !is_rust_app_root(&root)? {
        bail!("Rust modules require a Rust or mixed application");
    }
    let dryrun = matches.get_flag("dryrun");
    generate_module_at(&root, &name, matches, dryrun, kind)
}

pub(super) fn generate_module_at(
    root: &Path,
    name: &str,
    matches: &ArgMatches,
    dryrun: bool,
    kind: BuiltinModule,
) -> Result<()> {
    let manifest_path = root.join(".forklaunch/manifest.toml");
    let manifest = if manifest_path.exists() {
        read_manifest_value(&manifest_path)?
    } else {
        Value::Table(Map::new())
    };
    if let Some(project) = project_table(&manifest, &name) {
        if project.get("variant").and_then(Value::as_str) != Some(kind.variant())
            || !is_rust_project(root, name)?
        {
            bail!("Project {name} already exists and is not the requested Rust module");
        }
    }
    let crate_path = Path::new("crates").join(&name);
    let crate_dir = root.join(&crate_path);
    if project_table(&manifest, name).is_none() && crate_dir.join("Cargo.toml").exists() {
        bail!(
            "An existing crate occupies the module destination: {}",
            crate_dir.display()
        );
    }
    let data = RustModuleTemplate {
        name: escape_toml_string(name),
        description: escape_toml_string(kind.description()),
        crate_module: name.to_case(Case::Snake),
        twilio: kind == BuiltinModule::MessagingTwilio,
    };
    let templates = [
        "Cargo.toml",
        "src/lib.rs",
        "src/main.rs",
        ".env.example",
        "README.md",
    ]
    .into_iter()
    .map(|output| {
        let filename = match output {
            "Cargo.toml" => "Cargo.toml.template",
            "src/lib.rs" => "lib.rs",
            "src/main.rs" => "main.rs",
            ".env.example" => "env.example",
            _ => "README.md",
        };
        let template = format!("rust/{}/{filename}", kind.template());
        RenderedTemplate {
            path: crate_dir.join(output),
            content: render_rust_template(&template, &data),
            context: None,
        }
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
        kind.description(),
        matches,
        dryrun,
    )?;
    if !dryrun {
        mark_module(&root, &name, kind)?;
    }
    ensure_module_compose(&root, &name, kind, dryrun)?;
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
        write(&cargo, BLUEPRINT_CARGO)?;
    } else {
        let required: toml_edit::DocumentMut = BLUEPRINT_CARGO.parse()?;
        let mut existing: toml_edit::DocumentMut = read_to_string(&cargo)?.parse()?;
        let members = existing["workspace"]["members"]
            .as_array_mut()
            .context("Expected embedded blueprint members")?;
        for member in required["workspace"]["members"]
            .as_array()
            .context("Expected packaged blueprint members")?
            .iter()
            .filter_map(|member| member.as_str())
        {
            if !members.iter().any(|value| value.as_str() == Some(member)) {
                members.push(member);
            }
        }
        write(&cargo, existing.to_string())?;
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

fn mark_module(root: &Path, name: &str, kind: BuiltinModule) -> Result<()> {
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
        .context("Expected Rust module project")?;
    project.insert(
        "variant".to_owned(),
        Value::String(kind.variant().to_owned()),
    );
    if kind == BuiltinModule::Cac {
        if let Some(resources) = project.get_mut("resources").and_then(Value::as_table_mut) {
            resources.remove("database");
        }
    }
    write_manifest_value(&path, &manifest)
}

pub(super) fn module_kind(root: &Path, name: &str) -> Result<Option<BuiltinModule>> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    Ok(project_table(&manifest, name)
        .and_then(|project| project.get("variant"))
        .and_then(Value::as_str)
        .and_then(BuiltinModule::from_variant))
}

pub(super) fn ensure_module_compose(
    root: &Path,
    name: &str,
    kind: BuiltinModule,
    dryrun: bool,
) -> Result<()> {
    if kind == BuiltinModule::Iam {
        return ensure_iam_compose(root, name, dryrun);
    }
    if matches!(
        kind,
        BuiltinModule::MessagingBase | BuiltinModule::MessagingTwilio
    ) {
        return ensure_messaging_compose(root, name, kind, dryrun);
    }
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
        .context("Expected CAC Compose service")?;
    let env = service.environment.get_or_insert_with(IndexMap::new);
    env.entry("HMAC_SECRET_KEY".to_owned())
        .or_insert_with(|| "${CAC_HMAC_SECRET_KEY:?Configure a service HMAC secret}".to_owned());
    env.entry("JWKS_PUBLIC_KEY_URL".to_owned())
        .or_insert_with(|| "${CAC_JWKS_PUBLIC_KEY_URL:?Configure trusted JWKS}".to_owned());
    write(path, yaml_to_string(&compose)?)?;
    Ok(())
}

fn ensure_messaging_compose(
    root: &Path,
    name: &str,
    kind: BuiltinModule,
    dryrun: bool,
) -> Result<()> {
    if dryrun {
        return Ok(());
    }
    let path = root.join("docker-compose.yaml");
    if !read_or_create_rust_docker_compose(&path)?
        .services
        .contains_key(name)
    {
        ensure_rust_docker_compose_project(root, RustProjectKind::Service, name, false)?;
    }
    let mut compose = read_or_create_rust_docker_compose(&path)?;
    let service = compose
        .services
        .get_mut(name)
        .context("Expected messaging Compose service")?;
    let env = service.environment.get_or_insert_with(IndexMap::new);
    for (key, value) in [
        (
            "DATABASE_URL",
            "${MESSAGING_DATABASE_URL:?Configure a restricted PostgreSQL role}",
        ),
        (
            "TENANT_ID",
            "${MESSAGING_TENANT_ID:?Configure a trusted tenant}",
        ),
        (
            "ENCRYPTION_KEY",
            "${ENCRYPTION_KEY:?Configure a field encryption key}",
        ),
        (
            "HMAC_SECRET_KEY",
            "${MESSAGING_HMAC_SECRET_KEY:?Configure a service HMAC secret}",
        ),
        (
            "JWKS_PUBLIC_KEY_URL",
            "${MESSAGING_JWKS_PUBLIC_KEY_URL:?Configure trusted JWKS}",
        ),
    ] {
        env.entry(key.to_owned())
            .or_insert_with(|| value.to_owned());
    }
    if kind == BuiltinModule::MessagingTwilio {
        for key in [
            "TWILIO_ACCOUNT_SID",
            "TWILIO_AUTH_TOKEN",
            "TWILIO_FROM_NUMBER",
            "TWILIO_STATUS_CALLBACK_URL",
        ] {
            env.entry(key.to_owned())
                .or_insert_with(|| format!("${{{key}:?Configure Twilio delivery}}"));
        }
    }
    write(path, yaml_to_string(&compose)?)?;
    Ok(())
}

fn ensure_iam_compose(root: &Path, name: &str, dryrun: bool) -> Result<()> {
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

pub(super) fn rename_sources(root: &Path, old: &str, new: &str) -> Result<()> {
    let crate_dir = root.join("crates").join(new);
    for filename in ["src/lib.rs", "src/main.rs"] {
        let path = crate_dir.join(filename);
        if path.exists() {
            let namespace = regex::Regex::new(&format!(
                r"\b{}::",
                regex::escape(&old.to_case(Case::Snake))
            ))?;
            let source = namespace
                .replace_all(
                    &read_to_string(&path)?,
                    format!("{}::", new.to_case(Case::Snake)),
                )
                .into_owned()
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
