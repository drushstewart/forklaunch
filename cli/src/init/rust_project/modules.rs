use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinModule {
    Iam,
    Cac,
    EcommerceStripe,
    BillingBase,
    BillingStripe,
}

impl BuiltinModule {
    pub(super) fn from_variant(variant: &str) -> Option<Self> {
        match variant {
            "iam-base" => Some(Self::Iam),
            "cac-base" => Some(Self::Cac),
            "ecommerce-stripe" => Some(Self::EcommerceStripe),
            "billing-base" => Some(Self::BillingBase),
            "billing-stripe" => Some(Self::BillingStripe),
            _ => None,
        }
    }
    fn variant(self) -> &'static str {
        match self {
            Self::Iam => "iam-base",
            Self::Cac => "cac-base",
            Self::EcommerceStripe => "ecommerce-stripe",
            Self::BillingBase => "billing-base",
            Self::BillingStripe => "billing-stripe",
        }
    }
    pub(super) fn template(self) -> &'static str {
        match self {
            Self::Iam => "iam",
            Self::Cac => "cac",
            Self::EcommerceStripe => "ecommerce",
            Self::BillingBase | Self::BillingStripe => "billing",
        }
    }
    pub(super) fn service_name(self) -> &'static str {
        self.template()
    }
    fn description(self) -> &'static str {
        match self {
            Self::Iam => "Persistent PostgreSQL IAM",
            Self::Cac => "CAC code-set service",
            Self::EcommerceStripe => "Persistent ecommerce with Stripe and optional PayPal",
            Self::BillingBase => "Persistent billing services",
            Self::BillingStripe => "Persistent billing with Stripe",
        }
    }
}

/// Modules implemented only in TypeScript. Messaging is not a module in either
/// language: services send through the managed gateway (`forklaunch infra add`).
pub(super) const TYPESCRIPT_ONLY_MODULES: &[&str] = &["iam-better-auth", "relay"];

#[derive(Content)]
struct RustModuleTemplate {
    name: String,
    description: String,
    crate_module: String,
    stripe: bool,
}

pub(crate) fn generate_rust_module(matches: &ArgMatches) -> Result<()> {
    let requested = option(matches, "module").unwrap_or_default();
    if let Ok(module) = requested.parse::<crate::constants::Module>() {
        crate::constants::ensure_module_offered(&module)?;
    }
    if TYPESCRIPT_ONLY_MODULES.contains(&requested.as_str()) {
        bail!("{requested} is a TypeScript module; omit --language rust. Rust services call it through its generated Rust SDK");
    }
    let kind = BuiltinModule::from_variant(&requested)
        .context("Rust module generation supports --module iam-base, cac-base, ecommerce-stripe, billing-base or billing-stripe")?;
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
    generate_module_at(&root, &name, matches, dryrun, kind)?;
    if !dryrun {
        let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
        let crate_path = crate_path_for_project(&manifest, &name).unwrap_or_else(|| name.clone());
        let mut stdout = StandardStream::stdout(ColorChoice::Always);
        log_ok!(stdout, "Generated Rust {} module {} at {}", kind.variant(), name, root.join(crate_path).display());
    }
    Ok(())
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
    let crate_path = crate_path_for_project(&manifest, name)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(manifest.get("modules_path").and_then(Value::as_str).unwrap_or("src")).join(name));
    let crate_dir = root.join(&crate_path);
    if project_table(&manifest, name).is_none() && crate_dir.join("Cargo.toml").exists() {
        bail!(
            "An existing crate occupies the module destination: {}",
            crate_dir.display()
        );
    }
    let shared_contracts = !crate_dir.join("Cargo.toml").exists();
    if shared_contracts {
        for destination in [".forklaunch/contracts", "sdk/rust", "sdk/typescript"] {
            anyhow::ensure!(!crate_dir.join(destination).exists(),
                "Module SDK destination already exists: {}", crate_dir.join(destination).display());
        }
    }
    if manifest_path.exists() {
        mixed::ensure_workspace(root, dryrun)?;
    }
    let data = RustModuleTemplate {
        name: escape_toml_string(name),
        description: escape_toml_string(kind.description()),
        crate_module: name.to_case(Case::Snake),
        stripe: kind == BuiltinModule::BillingStripe,
    };
    let templates = [
        "Cargo.toml",
        "src/lib.rs",
        "src/main.rs",
        "src/routes/mod.rs",
        "build.rs",
        "persistence/compliance_schema.rs",
        ".env.example",
        "README.md",
    ]
    .into_iter()
    .map(|output| {
        let filename = match output {
            "Cargo.toml" => "Cargo.toml.template",
            "src/lib.rs" => "lib.rs",
            "src/main.rs" => "main.rs",
            "persistence/compliance_schema.rs" => "compliance_schema.rs",
            ".env.example" => "env.example",
            _ => "README.md",
        };
        let template = if output == "build.rs" {
            if shared_contracts { "rust/project/module-sdk-build.rs" } else { "rust/project/module-compliance-build.rs" }.to_owned()
        } else if output == "src/routes/mod.rs" {
            "rust/router/native_routes_mod.rs".to_owned()
        } else {
            format!("rust/{}/{filename}", kind.template())
        };
        RenderedTemplate {
            path: crate_dir.join(output),
            content: if output == "src/routes/mod.rs" {
                native_routes_mod_rs(&[])
            } else {
                let mut rendered = render_rust_template(&template, &data);
                if output == "Cargo.toml" {
                    rendered.push_str("\n[build-dependencies]\nforklaunch-core = { path = \"../../.forklaunch/framework-rs/crates/core\", features = [\"compliance-artifact\"] }\n");
                    if shared_contracts { rendered = with_contract_build_dependencies(&rendered).expect("valid generated module manifest"); }
                }
                rendered
            },
            context: None,
        }
    })
    .filter(|template| !template.path.exists())
    .collect::<Vec<_>>();
    ensure_embedded_framework(&root, dryrun)?;
    let mut templates = if !crate_dir.join("src/lib.rs").exists() {
        let mut templates = native_layout::builtin_templates(&crate_dir, name, templates);
        if !crate_dir.join("api/http").exists() {
            http_adapter::embed(&crate_dir, kind, &mut templates)?;
        }
        templates
    } else {
        templates
    };
    if shared_contracts {
        add_local_sdk_templates(&crate_dir, name, &mut templates)?;
    }
    // Older generated modules have no build script. Add only the required host
    // dependency alongside the new exporter; preserve user source and settings.
    if crate_dir.join("Cargo.toml").exists() && !crate_dir.join("build.rs").exists() {
        let cargo_path = crate_dir.join("Cargo.toml");
        let mut cargo: toml_edit::DocumentMut = read_to_string(&cargo_path)?.parse()?;
        let mut dependency = cargo["dependencies"]["forklaunch-core"].clone();
        dependency["features"] = toml_edit::value(toml_edit::Array::from_iter(["compliance-artifact"]));
        cargo["build-dependencies"]["forklaunch-core"] = dependency;
        templates.push(RenderedTemplate { path: cargo_path, content: cargo.to_string(), context: None });
    }
    let templates = native_layout::framework_paths(root, templates);
    write_rust_rendered_templates(
        &templates,
        dryrun,
        &mut StandardStream::stdout(ColorChoice::Always),
    )?;
    add_cargo_member(&root, &crate_path.to_string_lossy(), dryrun)?;
    let local_sdk = shared_contracts || crate_dir.join("sdk/rust/.forklaunch-sdk-local").is_file();
    if local_sdk {
        for relative in [".forklaunch/contracts", "sdk/rust"] {
            add_cargo_member(root, &crate_path.join(relative).to_string_lossy(), dryrun)?;
        }
    }
    add_project_to_manifest(
        &root,
        RustProjectKind::Service,
        &name,
        kind.description(),
        matches,
        dryrun,
    )?;
    if !dryrun {
        let path = root.join(".forklaunch/manifest.toml");
        let mut manifest = read_manifest_value(&path)?;
        let metadata = project_table_mut(&mut manifest, name)
            .context("Expected generated module")?
            .get_mut("metadata")
            .and_then(Value::as_table_mut)
            .context("Expected module metadata")?;
        metadata.insert(
                "cratePath".into(),
                Value::String(crate_path.to_string_lossy().into_owned()),
            );
        if local_sdk {
            metadata.insert("sdkLayout".into(), "project-local-v1".into());
            contracts_layout::set_artifact_paths(metadata, name);
        }
        write_manifest_value(&path, &manifest)?;
        mark_module(&root, &name, kind)?;
    }
    ensure_module_compose(&root, &name, kind, dryrun)?;
    ensure_rust_package_scripts(&root, RustProjectKind::Service, &name, dryrun)?;
    Ok(())
}

const REGISTRY_ENVIRONMENT: &[&str] = &[
    "BILLING_REQUEST_DATE_PROFILE", "BILLING_IDENTITY_PROFILE", "BILLING_BOUND_CHECKOUT_CONFIG",
    "BILLING_BOUND_BILLING_CONFIG", "BILLING_PUBLISHER_DATABASE_URL", "BILLING_HMAC_PROFILE",
    "BILLING_PAYMENT_LINK_UPDATE_PROFILE",
];

/// Fresh modules use the ordinary service-owned SDK packages. Existing modules
/// retain their generated output layout until the application explicitly migrates.
fn add_local_sdk_templates(dir: &Path, name: &str, templates: &mut Vec<RenderedTemplate>) -> Result<()> {
    let host = contracts_layout::new_host_directory(dir);
    let registry_uses_http = templates
        .iter()
        .any(|file| file.path == dir.join("api/contracts/registry.rs") && file.content.contains("crate::api::http"));
    let service = templates.iter_mut().find(|file| file.path == dir.join("Cargo.toml"))
        .context("Missing fresh module manifest")?;
    let mut cargo: toml_edit::DocumentMut = service.content.parse()?;
    let mut host_cargo: toml_edit::DocumentMut = format!(
        "[package]\nname = \"{name}-contracts\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\npublish = false\n[lib]\npath = \"lib.rs\"\n"
    ).parse()?;
    host_cargo["dependencies"] = cargo["build-dependencies"].clone();
    // Registries that project contracts through the module's HTTP layer compile
    // that layer in the host too, with the dependencies it needs.
    if registry_uses_http {
        let host_dependencies = host_cargo["dependencies"].as_table_mut().context("Expected host dependencies")?;
        for (dependency, spec) in cargo["dependencies"].as_table().context("Expected module dependencies")?.iter() {
            if dependency != "contracts" && !host_dependencies.contains_key(dependency) {
                host_dependencies.insert(dependency, spec.clone());
            }
        }
        if let Some(features) = cargo.get("features") {
            host_cargo["features"] = features.clone();
        }
        if let Some(development) = cargo.get("dev-dependencies") {
            host_cargo["dev-dependencies"] = development.clone();
        }
    }
    let features = host_cargo["dependencies"]["forklaunch-core"]["features"].as_array_mut()
        .context("Missing registry core features")?;
    if !features.iter().any(|feature| feature.as_str() == Some("typed-schema")) {
        features.push("typed-schema");
    }
    let mut contracts = toml_edit::InlineTable::new();
    contracts.insert("package", format!("{name}-contracts").into());
    contracts.insert("path", ".forklaunch/contracts".into());
    cargo["dependencies"]["contracts"] = toml_edit::value(contracts.clone());
    cargo["build-dependencies"]["contracts"] = toml_edit::value(contracts);
    service.content = cargo.to_string();
    let build = templates.iter_mut().find(|file| file.path == dir.join("build.rs"))
        .context("Missing fresh module build script")?;
    build.content = build.content.replace(
        "#[allow(dead_code)]\n#[path = \"api/contracts/mod.rs\"]\nmod contracts;\n", ""
    ).replace("contracts::registry::configured_registry()?", "contracts::configured_registry()?");
    templates.extend([
        RenderedTemplate { path: host.join("Cargo.toml"), content: host_cargo.to_string(), context: None },
        RenderedTemplate { path: host.join("lib.rs"), content: format!("//! Private registry compiler host; declarations remain owned by the service.\n#[path = \"../../api/contracts/registry.rs\"]\npub mod registry;\n#[path = \"../../api/contracts/extensions/mod.rs\"]\npub mod extensions;\npub use registry::configured_registry;\n{}",
            if registry_uses_http { "pub mod api;\n" } else { "" }), context: None },
    ]);
    if registry_uses_http {
        // One copy only: the service uses the host's layer, so registry and
        // runtime share the same configuration types.
        let api = templates.iter_mut().find(|file| file.path == dir.join("api/mod.rs"))
            .context("Missing module api exports")?;
        api.content = api.content.replace("pub mod http;\n", "pub use ::contracts::api::http;\n");
        templates.push(RenderedTemplate {
            path: host.join("api.rs"),
            content: "//! The module's HTTP layer, which its registry uses to project contracts.\n#[path = \"../../api/http/mod.rs\"]\npub mod http;\n".into(),
            context: None,
        });
    }
    let exports = templates.iter_mut().find(|file| file.path == dir.join("api/contracts/mod.rs"))
        .context("Missing fresh module contract exports")?;
    exports.content = "//! Shared declarations used by both runtime and contract compilation.\npub use contracts::{extensions, registry};\n".into();
    templates.extend(sdk_consumer::local_templates_with_registry(
        dir, name, &host, "contracts::configured_registry()?", REGISTRY_ENVIRONMENT,
    ));
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
    ensure_module_compose_sources(root, name, kind, dryrun)?;
    if !dryrun {
        // Modules may add another process (ecommerce inventory) or rename the
        // existing binary after generic project scaffolding has run.
        language_images::reconcile(root)?;
    }
    Ok(())
}

fn ensure_module_compose_sources(
    root: &Path,
    name: &str,
    kind: BuiltinModule,
    dryrun: bool,
) -> Result<()> {
    if kind == BuiltinModule::Iam {
        return ensure_iam_compose(root, name, dryrun);
    }
    if kind == BuiltinModule::EcommerceStripe {
        return ensure_ecommerce_compose(root, name, dryrun);
    }
    if matches!(
        kind,
        BuiltinModule::BillingBase | BuiltinModule::BillingStripe
    ) {
        return ensure_billing_compose(root, name, kind, dryrun);
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
        for key in [
            "STRIPE_SECRET_KEY",
            "STRIPE_API_KEY",
            "STRIPE_WEBHOOK_SECRET",
            "PLATFORM_GATEWAY_URL",
            "INSTANCE_ID",
            "INSTANCE_HMAC_KEY",
            "FORKLAUNCH_LOCAL_GATEWAY_MOCK",
        ] {
            env.entry(key.to_owned())
                .or_insert_with(|| format!("${{{key}:-}}"));
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
    for (key, value) in [
        (
            "ENCRYPTION_KEY",
            "${IAM_ENCRYPTION_KEY:?Configure field encryption}",
        ),
        (
            "HMAC_SECRET_KEY",
            "${IAM_HMAC_SECRET_KEY:?Configure service authentication}",
        ),
        (
            "JWKS_PUBLIC_KEY_URL",
            "${IAM_JWKS_PUBLIC_KEY_URL:?Configure JWT verification}",
        ),
    ] {
        env.entry(key.to_owned())
            .or_insert_with(|| value.to_owned());
    }
    write(path, yaml_to_string(&compose)?)?;
    Ok(())
}

pub(super) fn rename_sources(root: &Path, old: &str, new: &str) -> Result<()> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    let crate_dir = root
        .join(crate_path_for_project(&manifest, new).unwrap_or_else(|| format!("crates/{new}")));
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
            if let Some(DockerCommand::Multiple(args)) = &mut worker.command {
                if args.len() == 2 && args[1] == "--worker" {
                    for prefix in ["./target/release/", "/app/bin/"] {
                        if args[0] == format!("{prefix}{old}") {
                            args[0] = format!("{prefix}{new}");
                            break;
                        }
                    }
                }
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

/// Registry compilation shares the runtime descriptors; no server or database starts at build time.
fn with_contract_build_dependencies(source: &str) -> Result<String> {
    let mut cargo: toml_edit::DocumentMut = source.parse()?;
    for name in ["forklaunch-core", "forklaunch-iam-interface", "forklaunch-cac-interface", "forklaunch-ecommerce-interface", "forklaunch-billing-interface", "forklaunch-billing-axum"] {
        if let Some(dependency) = cargo["dependencies"].get(name).cloned() {
            cargo["build-dependencies"][name] = dependency;
        }
    }
    let features = cargo["build-dependencies"]["forklaunch-core"]["features"].as_array_mut().context("Core build features are required")?;
    for required in ["build", "compliance-artifact"] {
        if !features.iter().any(|v| v.as_str() == Some(required)) { features.push(required); }
    }
    Ok(cargo.to_string())
}

pub(super) fn extension_contracts_mod(modules: &[String]) -> String {
    native_routes_mod_rs(modules).split("pub fn register_router(").next().unwrap()
        .replace("use forklaunch_axum::ForkLaunchRouter;", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecommerce_rename_preserves_worker_settings_in_shared_and_legacy_images() {
        for executable in ["/app/bin/shop", "./target/release/shop", "/custom/inventory"] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join(".forklaunch")).unwrap();
            write(root.path().join(".forklaunch/manifest.toml"), "app_name = 'app'\n").unwrap();
            write(root.path().join("docker-compose.yaml"), format!(
                "services:\n  shop:\n    image: app-rust\n    command: ['/app/bin/shop']\n  shop-inventory-worker:\n    image: app-rust\n    command: ['{executable}', '--worker']\n    x-forklaunch-ecommerce-worker-for: shop\n    x-user-marker: preserved\n    environment:\n      WORKER_BATCH_SIZE: '2'\n"
            )).unwrap();
            rename_sources(root.path(), "shop", "store").unwrap();
            let compose = read_or_create_rust_docker_compose(&root.path().join("docker-compose.yaml")).unwrap();
            assert!(!compose.services.contains_key("shop-inventory-worker"));
            let worker = &compose.services["store-inventory-worker"];
            let expected = if executable == "/custom/inventory" { executable.to_owned() }
                else { executable.replace("shop", "store") };
            assert!(matches!(&worker.command, Some(DockerCommand::Multiple(args)) if args == &vec![expected, "--worker".to_owned()]));
            assert_eq!(ecommerce_worker_owner(worker), Some("store"));
            assert_eq!(worker.additional_properties["x-user-marker"].as_str(), Some("preserved"));
            assert_eq!(worker.environment.as_ref().unwrap()["WORKER_BATCH_SIZE"], "2");
        }
    }

    #[test]
    fn all_builtin_variants_render_valid_runtime_and_contract_host_manifests() {
        for kind in [
            BuiltinModule::Iam,
            BuiltinModule::Cac,
            BuiltinModule::EcommerceStripe,
            BuiltinModule::BillingBase,
            BuiltinModule::BillingStripe,
        ] {
            let data = RustModuleTemplate {
                name: "test-module".into(),
                description: escape_toml_string(kind.description()),
                crate_module: "test_module".into(),
                        stripe: kind == BuiltinModule::BillingStripe,
            };
            let mut runtime = render_rust_template(
                &format!("rust/{}/Cargo.toml.template", kind.template()),
                &data,
            );
            runtime.push_str("\n[build-dependencies]\nforklaunch-core = { path = \"../../.forklaunch/framework-rs/crates/core\", features = [\"compliance-artifact\"] }\n");
            let runtime_manifest: toml::Value = toml::from_str(&runtime)
                .unwrap_or_else(|error| panic!("{} runtime manifest: {error}", kind.variant()));
            let runtime_features = runtime_manifest["dependencies"]["forklaunch-core"]["features"]
                .as_array().unwrap();
            for feature in ["openapi", "auth", "otlp", "remote-jwks"] {
                assert!(runtime_features.iter().any(|value| value.as_str() == Some(feature)),
                    "{} lost {feature}", kind.variant());
            }
            assert_eq!(runtime_features.iter().any(|value| value.as_str() == Some("postgres")), kind != BuiltinModule::Cac);
            let compiled = with_contract_build_dependencies(&runtime)
                .unwrap_or_else(|error| panic!("{} contract host manifest: {error}", kind.variant()));
            let host: toml::Value = toml::from_str(&compiled).unwrap();
            assert_eq!(host["dependencies"], runtime_manifest["dependencies"],
                "Contract compilation must preserve {} runtime dependencies", kind.variant());
            let build_features = host["build-dependencies"]["forklaunch-core"]["features"]
                .as_array().unwrap();
            assert!(runtime_features.iter().all(|feature| build_features.contains(feature)));
            assert!(build_features.iter().any(|value| value.as_str() == Some("build")));
            assert!(build_features.iter().any(|value| value.as_str() == Some("compliance-artifact")),
                "{} must compile its compliance manifest independently", kind.variant());
            for (name, dependency) in runtime_manifest["dependencies"].as_table().unwrap() {
                // Ecommerce's worker interface is used only by its inventory
                // process; the declaration host imports the domain interface.
                if name.ends_with("-interface") && name != "forklaunch-worker-interface" {
                    assert_eq!(&host["build-dependencies"][name.as_str()], dependency,
                        "{} declarations need the same interface dependency", kind.variant());
                }
            }
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("backend/domains/test-module");
            let mut files = vec![
                RenderedTemplate { path: dir.join("Cargo.toml"), content: compiled, context: None },
                RenderedTemplate { path: dir.join("build.rs"), content: render_rust_template("rust/project/module-sdk-build.rs", &data), context: None },
                RenderedTemplate { path: dir.join("api/contracts/mod.rs"), content: "pub mod registry;\npub mod extensions;\n".into(), context: None },
            ];
            add_local_sdk_templates(&dir, "test-module", &mut files).unwrap();
            let files = native_layout::framework_paths(root.path(), files);
            let source = |relative: &str| files.iter().find(|file| file.path == dir.join(relative)).unwrap().content.as_str();
            let service: Value = toml::from_str(source("Cargo.toml")).unwrap();
            // The host boundary changes no runtime dependencies; paths alone
            // follow the configured module root.
            let normalized = native_layout::framework_paths(root.path(), vec![RenderedTemplate {
                path: dir.join("Cargo.toml"), content: runtime.clone(), context: None,
            }]);
            let expected: Value = toml::from_str(&normalized[0].content).unwrap();
            let mut runtime_dependencies = service["dependencies"].as_table().unwrap().clone();
            runtime_dependencies.remove("contracts");
            assert_eq!(Value::Table(runtime_dependencies), expected["dependencies"]);
            assert_eq!(service["build-dependencies"]["contracts"]["path"].as_str(), Some(".forklaunch/contracts"));
            assert_eq!(service["dependencies"]["contracts"], service["build-dependencies"]["contracts"],
                "Runtime and compiler must use exactly the same declaration package");
            let host: Value = toml::from_str(source(".forklaunch/contracts/Cargo.toml")).unwrap();
            assert_eq!(host["package"]["publish"].as_bool(), Some(false));
            assert!(host["dependencies"].get("test-module").is_none());
            assert!(host["dependencies"]["forklaunch-core"]["features"].as_array().unwrap().iter().any(|value| value.as_str() == Some("typed-schema")));
            assert_eq!(source("sdk/rust/.forklaunch-sdk-local"), "forklaunch.sdk.local.v1\n");
            let sdk: Value = toml::from_str(source("sdk/rust/Cargo.toml")).unwrap();
            assert_eq!(sdk["build-dependencies"]["contracts"]["path"].as_str(), Some("../../.forklaunch/contracts"));
            assert!(sdk["dependencies"].get("test-module").is_none());
            assert!(!source("build.rs").contains("mod contracts;"));
            for build in [source("build.rs"), source("sdk/rust/build.rs")] {
                syn::parse_file(build).unwrap();
                assert!(build.contains("contracts::configured_registry()?"));
                for environment in REGISTRY_ENVIRONMENT {
                    assert!(build.contains(environment));
                }
            }
            assert!(source("sdk/rust/build.rs").contains("\"Client\""));
            assert!(source(".forklaunch/contracts/lib.rs").contains("../../api/contracts/registry.rs"));
            assert!(source(".forklaunch/contracts/lib.rs").contains("../../api/contracts/extensions/mod.rs"));
            assert!(source("api/contracts/mod.rs").contains("pub use contracts::{extensions, registry};"));
            assert!(!source(".forklaunch/contracts/lib.rs").contains("server"));
            for file in &files {
                create_dir_all(file.path.parent().unwrap()).unwrap();
                write(&file.path, &file.content).unwrap();
            }
            create_dir_all(root.path().join(".forklaunch")).unwrap();
            write(root.path().join(".forklaunch/manifest.toml"), "modules_path='backend/domains'\n").unwrap();
            assert_eq!(
                forklaunch_core::build::provenance::contract_directory(&dir).unwrap(),
                Some(dir.join(".forklaunch/contracts").canonicalize().unwrap()),
                "The actual compiler provenance guard must accept the generated host boundary",
            );
            create_dir_all(dir.join("api/routes")).unwrap();
            create_dir_all(dir.join("api/contracts")).unwrap();
            write(dir.join("api/contracts/registry.rs"), "// Existing module registry\n").unwrap();
            assert!(contracts_layout::directory(&dir).unwrap().join("Cargo.toml").is_file());
            assert!(contracts_layout::has_local_sdk(&dir));
            assert!(!contracts_layout::is_native_service(&dir), "Module extension routers must retain their module adapter");
            let mut metadata = Map::new();
            metadata.insert("cratePath".into(), "backend/domains/test-module".into());
            metadata.insert("sdkLayout".into(), "project-local-v1".into());
            contracts_layout::set_artifact_paths(&mut metadata, "test-module");
            assert_eq!(metadata["contractArtifactPath"].as_str(), Some("backend/domains/test-module/sdk/typescript/contract.json"));
        }
    }
}
