use super::*;

static BLUEPRINT_CRATES: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/src/templates/rust/blueprint-workspace/crates");
const BLUEPRINT_CARGO: &str =
    include_str!("../../templates/rust/blueprint-workspace/Cargo.toml.template");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinModule {
    Iam,
    IamBetterAuth,
    Cac,
    MessagingBase,
    MessagingTwilio,
    EcommerceStripe,
    BillingBase,
    BillingStripe,
}

impl BuiltinModule {
    pub(super) fn from_variant(variant: &str) -> Option<Self> {
        match variant {
            "iam-base" => Some(Self::Iam),
            "iam-better-auth" => Some(Self::IamBetterAuth),
            "cac-base" => Some(Self::Cac),
            "messaging-base" => Some(Self::MessagingBase),
            "messaging-twilio" => Some(Self::MessagingTwilio),
            "ecommerce-stripe" => Some(Self::EcommerceStripe),
            "billing-base" => Some(Self::BillingBase),
            "billing-stripe" => Some(Self::BillingStripe),
            _ => None,
        }
    }
    fn variant(self) -> &'static str {
        match self {
            Self::Iam => "iam-base",
            Self::IamBetterAuth => "iam-better-auth",
            Self::Cac => "cac-base",
            Self::MessagingBase => "messaging-base",
            Self::MessagingTwilio => "messaging-twilio",
            Self::EcommerceStripe => "ecommerce-stripe",
            Self::BillingBase => "billing-base",
            Self::BillingStripe => "billing-stripe",
        }
    }
    pub(super) fn template(self) -> &'static str {
        match self {
            Self::Iam => "iam",
            Self::IamBetterAuth => "iam-better-auth",
            Self::Cac => "cac",
            Self::MessagingBase | Self::MessagingTwilio => "messaging",
            Self::EcommerceStripe => "ecommerce",
            Self::BillingBase | Self::BillingStripe => "billing",
        }
    }
    pub(super) fn service_name(self) -> &'static str {
        if self == Self::IamBetterAuth {
            "iam"
        } else {
            self.template()
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Iam => "Persistent PostgreSQL IAM",
            Self::IamBetterAuth => "Encrypted PostgreSQL Better Auth",
            Self::Cac => "CAC code-set service",
            Self::MessagingBase => "Persistent messaging hooks",
            Self::MessagingTwilio => "Persistent Twilio SMS messaging",
            Self::EcommerceStripe => "Persistent ecommerce with Stripe and optional PayPal",
            Self::BillingBase => "Persistent billing services",
            Self::BillingStripe => "Persistent billing with Stripe",
        }
    }
}

#[derive(Content)]
struct RustModuleTemplate {
    name: String,
    description: String,
    crate_module: String,
    twilio: bool,
    stripe: bool,
}

pub(crate) fn generate_rust_module(matches: &ArgMatches) -> Result<()> {
    let kind = option(matches, "module")
        .as_deref()
        .and_then(BuiltinModule::from_variant)
        .context("Rust module generation currently supports --module iam-base, iam-better-auth, cac-base, messaging-base, messaging-twilio, ecommerce-stripe, billing-base or billing-stripe")?;
    if option(matches, "database").is_some_and(|database| database != "postgresql") {
        bail!("Rust modules currently support --database postgresql");
    }
    let name = option(matches, "name").unwrap_or_else(|| kind.service_name().to_owned());
    if !validate_name(&name) {
        bail!("Invalid module service name");
    }
    let root = find_manifest_root(
        &option(matches, "base_path")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?),
    )?;
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
    if manifest_path.exists() {
        mixed::ensure_workspace(root, dryrun)?;
    }
    let data = RustModuleTemplate {
        name: escape_toml_string(name),
        description: escape_toml_string(kind.description()),
        crate_module: name.to_case(Case::Snake),
        twilio: kind == BuiltinModule::MessagingTwilio,
        stripe: kind == BuiltinModule::BillingStripe,
    };
    let templates = [
        "Cargo.toml",
        "src/lib.rs",
        "src/main.rs",
        "src/routes/mod.rs",
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
        let template = if output == "src/routes/mod.rs" {
            "rust/router/native_routes_mod.rs".to_owned()
        } else { format!("rust/{}/{filename}", kind.template()) };
        RenderedTemplate {
            path: crate_dir.join(output),
            content: if output == "src/routes/mod.rs" { native_routes_mod_rs(&[]) }
                else { render_rust_template(&template, &data) },
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
    if matches!(kind, BuiltinModule::Iam | BuiltinModule::IamBetterAuth) {
        return ensure_iam_compose(root, name, kind, dryrun);
    }
    if kind == BuiltinModule::EcommerceStripe {
        return ensure_ecommerce_compose(root, name, dryrun);
    }
    if matches!(kind, BuiltinModule::BillingBase | BuiltinModule::BillingStripe) {
        return ensure_billing_compose(root, name, kind, dryrun);
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

fn ensure_ecommerce_compose(root: &Path, name: &str, dryrun: bool) -> Result<()> {
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
        .context("Expected ecommerce Compose service")?;
    let env = service.environment.get_or_insert_with(IndexMap::new);
    for (key, value) in [
        (
            "DATABASE_URL",
            "${ECOMMERCE_DATABASE_URL:?Configure a restricted PostgreSQL role}",
        ),
        (
            "TENANT_ID",
            "${ECOMMERCE_TENANT_ID:?Configure a trusted tenant}",
        ),
        (
            "ENCRYPTION_KEY",
            "${ENCRYPTION_KEY:?Configure a field encryption key}",
        ),
        (
            "HMAC_SECRET_KEY",
            "${ECOMMERCE_HMAC_SECRET_KEY:?Configure a service HMAC secret}",
        ),
        (
            "JWKS_PUBLIC_KEY_URL",
            "${ECOMMERCE_JWKS_PUBLIC_KEY_URL:?Configure trusted JWKS}",
        ),
        (
            "STRIPE_API_KEY",
            "${STRIPE_API_KEY:?Configure Stripe payment creation}",
        ),
        (
            "STRIPE_WEBHOOK_SECRET",
            "${STRIPE_WEBHOOK_SECRET:?Configure Stripe webhook verification}",
        ),
    ] {
        env.entry(key.to_owned())
            .or_insert_with(|| value.to_owned());
    }
    let service = service.clone();
    let worker_name = format!("{name}-inventory-worker");
    if let Some(existing) = compose.services.get(&worker_name) {
        anyhow::ensure!(
            ecommerce_worker_owner(existing) == Some(name),
            "Compose service {worker_name} already exists and is not this module's inventory worker"
        );
    } else {
        let app_name = rust_app_name(root)?;
        let mut worker =
            rust_docker_service(&app_name, &worker_name, RustProjectKind::Worker, None);
        worker.image = service.image.clone();
        worker.build = service.build.clone();
        worker.networks = service.networks.clone();
        worker.command = Some(DockerCommand::Multiple(vec![
            format!("./target/release/{name}"),
            "--worker".to_owned(),
        ]));
        let mut worker_env = IndexMap::new();
        for (key, value) in service.environment.as_ref().into_iter().flatten() {
            if matches!(
                key.as_str(),
                "DATABASE_URL"
                    | "TENANT_ID"
                    | "ENCRYPTION_KEY"
                    | "LEGACY_ENCRYPTION_KEYS"
                    | "RUST_LOG"
            ) || key.starts_with("OTEL_")
            {
                worker_env.insert(key.clone(), value.clone());
            }
        }
        worker_env.insert("WORKER_HTTP_ADDR".to_owned(), "0.0.0.0:9090".to_owned());
        worker.environment = Some(worker_env);
        worker.healthcheck = Some(Healthcheck {
            test: HealthTest::List(vec![
                "CMD".to_owned(),
                "curl".to_owned(),
                "-fsS".to_owned(),
                "http://localhost:9090/ready".to_owned(),
            ]),
            interval: "10s".to_owned(),
            timeout: "5s".to_owned(),
            retries: 3,
            start_period: "10s".to_owned(),
            additional_properties: Default::default(),
        });
        worker.additional_properties.insert(
            ECOMMERCE_WORKER_OWNER.to_owned(),
            serde_yml::Value::String(name.to_owned()),
        );
        compose.services.insert(worker_name, worker);
    }
    write(path, yaml_to_string(&compose)?)?;
    Ok(())
}

const ECOMMERCE_WORKER_OWNER: &str = "x-forklaunch-ecommerce-worker-for";
pub(super) fn ecommerce_worker_owner(service: &DockerService) -> Option<&str> {
    service
        .additional_properties
        .get(ECOMMERCE_WORKER_OWNER)?
        .as_str()
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

fn ensure_billing_compose(
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
        .context("Expected billing Compose service")?;
    let env = service.environment.get_or_insert_with(IndexMap::new);
    for (key, value) in [
        (
            "DATABASE_URL",
            "${BILLING_DATABASE_URL:?Configure a restricted PostgreSQL role}",
        ),
        (
            "TENANT_ID",
            "${BILLING_TENANT_ID:?Configure a trusted tenant}",
        ),
        (
            "ENCRYPTION_KEY",
            "${ENCRYPTION_KEY:?Configure a field encryption key}",
        ),
        (
            "HMAC_SECRET_KEY",
            "${BILLING_HMAC_SECRET_KEY:?Configure a service HMAC secret}",
        ),
        (
            "JWKS_PUBLIC_KEY_URL",
            "${BILLING_JWKS_PUBLIC_KEY_URL:?Configure trusted JWKS}",
        ),
    ] {
        env.entry(key.to_owned())
            .or_insert_with(|| value.to_owned());
    }
    if kind == BuiltinModule::BillingStripe {
        for key in ["STRIPE_SECRET_KEY", "STRIPE_API_KEY", "STRIPE_WEBHOOK_SECRET",
            "PLATFORM_GATEWAY_URL", "INSTANCE_ID", "INSTANCE_HMAC_KEY", "FORKLAUNCH_LOCAL_GATEWAY_MOCK"] {
            env.entry(key.to_owned())
                .or_insert_with(|| format!("${{{key}:-}}"));
        }
    }
    write(path, yaml_to_string(&compose)?)?;
    Ok(())
}

fn ensure_iam_compose(root: &Path, name: &str, kind: BuiltinModule, dryrun: bool) -> Result<()> {
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
    for (key, value) in [
        ("ENCRYPTION_KEY", "${IAM_ENCRYPTION_KEY:?Configure field encryption}"),
        ("HMAC_SECRET_KEY", "${IAM_HMAC_SECRET_KEY:?Configure service authentication}"),
        ("JWKS_PUBLIC_KEY_URL", "${IAM_JWKS_PUBLIC_KEY_URL:?Configure JWT verification}"),
    ] {
        env.entry(key.to_owned()).or_insert_with(|| value.to_owned());
    }
    if kind == BuiltinModule::IamBetterAuth {
        for (key, value) in [
            ("BETTER_AUTH_URL", "${IAM_BETTER_AUTH_URL:?Configure the public authentication origin}"),
            ("BETTER_AUTH_BASE_PATH", "${IAM_BETTER_AUTH_BASE_PATH:-/api/auth}"),
            ("BETTER_AUTH_SECRET", "${IAM_BETTER_AUTH_SECRET:?Configure a 32-byte authentication secret}"),
            ("CORS_ORIGINS", "${CORS_ORIGINS:-}"),
            ("GOOGLE_CLIENT_ID", "${GOOGLE_CLIENT_ID:-}"),
            ("GOOGLE_CLIENT_SECRET", "${GOOGLE_CLIENT_SECRET:-}"),
        ] {
            env.entry(key.to_owned()).or_insert_with(|| value.to_owned());
        }
    }
    write(path, yaml_to_string(&compose)?)?;
    Ok(())
}

pub(super) fn rename_sources(root: &Path, old: &str, new: &str) -> Result<()> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    let crate_dir = root.join(crate_path_for_project(&manifest, new).unwrap_or_else(||format!("crates/{new}")));
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
        let old_worker = format!("{old}-inventory-worker");
        if compose
            .services
            .get(&old_worker)
            .is_some_and(|worker| ecommerce_worker_owner(worker) == Some(old))
        {
            let new_worker = format!("{new}-inventory-worker");
            anyhow::ensure!(
                !compose.services.contains_key(&new_worker),
                "Compose service {new_worker} already exists"
            );
            let mut worker = compose
                .services
                .shift_remove(&old_worker)
                .context("Expected module inventory worker")?;
            worker.hostname = Some(new_worker.clone());
            worker.container_name = Some(format!("{}-{new_worker}-rust", rust_app_name(root)?));
            if matches!(&worker.command, Some(DockerCommand::Multiple(args)) if args == &vec![format!("./target/release/{old}"), "--worker".to_owned()])
            {
                worker.command = Some(DockerCommand::Multiple(vec![
                    format!("./target/release/{new}"),
                    "--worker".to_owned(),
                ]));
            }
            worker.additional_properties.insert(
                ECOMMERCE_WORKER_OWNER.to_owned(),
                serde_yml::Value::String(new.to_owned()),
            );
            compose.services.insert(new_worker, worker);
        }
        write(path, yaml_to_string(&compose)?)?;
    }
    Ok(())
}
