use std::{
    collections::HashSet,
    fs::{create_dir_all, read_to_string, remove_dir_all, remove_file, rename, write},
    io::Write,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use convert_case::{Case, Casing};
use include_dir::{Dir, include_dir};
use indexmap::IndexMap;
use ramhorns::{Content, Template};
use serde_json::{Value as JsonValue, json};
use serde_yml::{from_str as yaml_from_str, to_string as yaml_to_string};
use termcolor::{ColorChoice, StandardStream, WriteColor};
use toml::{Value, map::Map};
use uuid::Uuid;

use crate::core::{
    docker::{
        Command as DockerCommand, DockerBuild, DockerCompose, DockerNetwork, DockerService,
        HealthTest, Healthcheck, Restart,
    },
    name::validate_name,
    rendered_template::{RenderedTemplate, TEMPLATES_DIR, write_rendered_templates},
};

// Embed source only, so released CLIs generate portable apps without a repository checkout.
static FRAMEWORK_CRATES: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/src/templates/rust/framework/crates");
const FRAMEWORK_CARGO: &str = include_str!("../templates/rust/framework/Cargo.toml.template");

mod iam;
mod worker;
pub(crate) use iam::generate_rust_iam_module;

pub(crate) const TYPESCRIPT_SDK_TSCONFIG: &str =
    include_str!("../templates/rust/application/typescript-sdk-tsconfig.json");

pub(crate) const PROJECT_LANGUAGE_VARIANTS: [&str; 3] = ["typescript", "rust", "mixed"];
pub(crate) const SDK_TARGET_VARIANTS: [&str; 4] = ["typescript", "ts", "rust", "all"];

pub(crate) fn selected_language(matches: &ArgMatches) -> &str {
    matches
        .get_one::<String>("language")
        .map(String::as_str)
        .unwrap_or("typescript")
}

pub(crate) fn uses_rust_generation(matches: &ArgMatches) -> bool {
    matches!(selected_language(matches), "rust" | "mixed")
}

fn sdk_targets_from_matches(matches: &ArgMatches) -> Vec<String> {
    matches
        .get_many::<String>("sdk-targets")
        .map(|targets| normalize_sdk_targets(targets.map(String::as_str)))
        .unwrap_or_else(default_sdk_targets)
}

fn default_sdk_targets() -> Vec<String> {
    vec!["typescript".to_string(), "rust".to_string()]
}

fn normalize_sdk_targets<'a>(targets: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut include_typescript = false;
    let mut include_rust = false;

    for target in targets {
        match target {
            "typescript" | "ts" => include_typescript = true,
            "rust" => include_rust = true,
            "all" => {
                include_typescript = true;
                include_rust = true;
            }
            _ => {}
        }
    }

    let mut normalized = Vec::new();
    if include_typescript {
        normalized.push("typescript".to_string());
    }
    if include_rust {
        normalized.push("rust".to_string());
    }

    if normalized.is_empty() {
        default_sdk_targets()
    } else {
        normalized
    }
}

fn sdk_targets_from_manifest(manifest: &Value) -> Vec<String> {
    manifest
        .get("sdkTargets")
        .and_then(Value::as_array)
        .map(|targets| normalize_sdk_targets(targets.iter().filter_map(Value::as_str)))
        .unwrap_or_else(default_sdk_targets)
}

fn sdk_targets_toml_array(sdk_targets: &[String]) -> String {
    sdk_targets
        .iter()
        .map(|target| format!("\"{}\"", escape_toml_string(target)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn has_typescript_sdk_target(sdk_targets: &[String]) -> bool {
    sdk_targets.iter().any(|target| target == "typescript")
}

pub(crate) fn generate_rust_application(matches: &ArgMatches) -> Result<()> {
    let name = required_name(matches, "application")?;
    let modules: Vec<_> = matches
        .try_get_many::<String>("modules")
        .ok()
        .flatten()
        .into_iter()
        .flatten()
        .collect();
    if modules.iter().any(|module| module.as_str() != "iam-base") {
        bail!(
            "Rust application module generation currently supports only iam-base; add other modules after their Rust adapters are implemented"
        );
    }
    if !modules.is_empty()
        && option(matches, "database").is_some_and(|database| database != "postgresql")
    {
        bail!("Rust IAM requires PostgreSQL");
    }
    let language = selected_language(matches).to_string();
    let dryrun = matches.get_flag("dryrun");
    let root_path = matches
        .get_one::<String>("path")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&name));
    let description =
        option(matches, "description").unwrap_or_else(|| format!("{name} application"));
    let author = option(matches, "author").unwrap_or_default();
    let license = option(matches, "license").unwrap_or_else(|| "MIT".to_string());
    let sdk_targets = sdk_targets_from_matches(matches);
    let manifest = application_manifest(
        &name,
        &description,
        &author,
        &license,
        &language,
        &sdk_targets,
    );

    let mut templates = vec![
        RenderedTemplate {
            path: root_path.join("Cargo.toml"),
            content: root_cargo_toml(&license, &author),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("rustfmt.toml"),
            content: render_static_rust_template("rust/application/rustfmt.toml"),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("clippy.toml"),
            content: render_static_rust_template("rust/application/clippy.toml"),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join(".gitignore"),
            content: render_static_rust_template("rust/application/gitignore"),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("README.md"),
            content: render_rust_template(
                "rust/application/README.md",
                &RustReadmeTemplate {
                    name: name.clone(),
                    description: description.clone(),
                },
            ),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join(".dockerignore"),
            content: render_static_rust_template("rust/application/dockerignore"),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("Dockerfile"),
            content: render_static_rust_template("rust/application/Dockerfile"),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("docker-compose.yaml"),
            content: rust_docker_compose_yaml(&name)?,
            context: None,
        },
        RenderedTemplate {
            path: root_path.join(".forklaunch").join("manifest.toml"),
            content: manifest,
            context: None,
        },
        RenderedTemplate {
            path: root_path
                .join(".forklaunch")
                .join("contracts")
                .join(".gitkeep"),
            content: String::new(),
            context: None,
        },
        RenderedTemplate {
            path: root_path
                .join(".forklaunch")
                .join("openapi")
                .join(".gitkeep"),
            content: String::new(),
            context: None,
        },
        RenderedTemplate {
            path: root_path
                .join(".forklaunch")
                .join("asyncapi")
                .join(".gitkeep"),
            content: String::new(),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("config").join("local.toml"),
            content: render_static_rust_template("rust/application/config/local.toml"),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("config").join("test.toml"),
            content: render_static_rust_template("rust/application/config/test.toml"),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join(".env.example"),
            content: render_static_rust_template("rust/application/env.example"),
            context: None,
        },
    ];

    if has_typescript_sdk_target(&sdk_targets) {
        templates.extend(mixed_typescript_sdk_templates(&root_path, &name));
    }

    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    write_rust_rendered_templates(&templates, dryrun, &mut stdout)?;
    ensure_embedded_framework(&root_path, dryrun)?;
    if !dryrun {
        create_dir_all(root_path.join("crates"))
            .with_context(|| format!("Failed to create {}", root_path.join("crates").display()))?;
    }
    if !modules.is_empty() {
        iam::generate_iam_at(&root_path, "iam", matches, dryrun)?;
    }
    log_ok!(
        stdout,
        "Generated {} application at {}",
        language,
        root_path.display()
    );

    Ok(())
}

pub(crate) fn generate_rust_service(matches: &ArgMatches) -> Result<()> {
    generate_rust_project(matches, RustProjectKind::Service)
}

pub(crate) fn generate_rust_worker(matches: &ArgMatches) -> Result<()> {
    generate_rust_project(matches, RustProjectKind::Worker)
}

pub(crate) fn generate_rust_library(matches: &ArgMatches) -> Result<()> {
    generate_rust_project(matches, RustProjectKind::Library)
}

pub(crate) fn generate_rust_router(matches: &ArgMatches) -> Result<()> {
    let dryrun = matches.get_flag("dryrun");
    let router_name = required_name(matches, "router")?;
    let router_module = router_name.to_case(Case::Snake);
    let service_path = matches
        .get_one::<String>("base_path")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().context("Failed to read current directory")?);
    let app_root = find_manifest_root(&service_path)?;
    let service_name = service_path
        .file_name()
        .and_then(|part| part.to_str())
        .context("Rust router path must point at a service crate directory")?
        .to_string();
    let routes_dir = service_path.join("src").join("routes");
    let route_file = routes_dir.join(format!("{router_module}.rs"));
    let mut stdout = StandardStream::stdout(ColorChoice::Always);

    write_text_file_preserving_existing(
        &route_file,
        &router_rs(&router_name),
        dryrun,
        &mut stdout,
    )?;

    rewrite_routes_mod(&routes_dir, dryrun, &mut stdout)?;
    add_router_to_manifest(&app_root, &service_name, &router_name, dryrun)?;

    log_ok!(
        stdout,
        "Generated Rust router {} in {}",
        router_name,
        service_path.display()
    );

    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RustProjectKind {
    Service,
    Worker,
    Library,
}

#[derive(Content)]
struct EmptyRustTemplate {}

#[derive(Content)]
struct RustRootCargoTemplate {
    license: String,
    authors: String,
}

#[derive(Content)]
struct RustReadmeTemplate {
    name: String,
    description: String,
}

#[derive(Content)]
struct RustApplicationManifestTemplate {
    id: String,
    cli_version: String,
    name: String,
    description: String,
    author: String,
    license: String,
    language: String,
    sdk_targets: String,
}

#[derive(Content)]
struct RustCrateTemplate {
    name: String,
    description: String,
    crate_module: String,
    has_binary: bool,
    uses_framework: bool,
    framework_rs_path: String,
}

#[derive(Content)]
struct RustRouterTemplate {
    router_name: String,
    route_title: String,
    response_name: String,
}

#[derive(Content)]
struct RustRoutesModTemplate {
    modules: Vec<RustRouteModuleTemplate>,
    has_health: bool,
}

#[derive(Content)]
struct RustRouteModuleTemplate {
    module: String,
}

#[derive(Clone)]
struct RustWorkspaceProject {
    name: String,
    kind: RustProjectKind,
    crate_path: String,
}

impl RustProjectKind {
    pub(crate) fn manifest_type(self) -> &'static str {
        match self {
            Self::Service => "Service",
            Self::Worker => "Worker",
            Self::Library => "Library",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Worker => "worker",
            Self::Library => "library",
        }
    }
}

pub(crate) fn is_rust_app_root(app_root: &Path) -> Result<bool> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    if !manifest_path.exists() {
        return Ok(false);
    }
    let manifest = read_manifest_value(&manifest_path)?;
    Ok(manifest
        .get("language")
        .and_then(Value::as_str)
        .map(|language| language == "rust" || language == "mixed")
        .unwrap_or(false)
        || manifest
            .get("runtime")
            .and_then(Value::as_str)
            .map(|runtime| runtime == "tokio")
            .unwrap_or(false))
}

pub(crate) fn is_rust_project(app_root: &Path, name: &str) -> Result<bool> {
    if !is_rust_app_root(app_root)? {
        return Ok(false);
    }
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let manifest = read_manifest_value(&manifest_path)?;
    Ok(project_table(&manifest, name)
        .and_then(|project| project.get("metadata"))
        .and_then(Value::as_table)
        .and_then(|metadata| metadata.get("language"))
        .and_then(Value::as_str)
        .map(|language| language == "rust")
        .unwrap_or_else(|| {
            app_root
                .join("crates")
                .join(name)
                .join("Cargo.toml")
                .exists()
        }))
}

pub(crate) fn sync_rust_project(app_root: &Path, kind: RustProjectKind, name: &str) -> Result<()> {
    let crate_path = Path::new("crates").join(name);
    let crate_dir = app_root.join(&crate_path);
    if !crate_dir.join("Cargo.toml").exists() {
        delete_rust_project(app_root, kind, name)?;
        return Ok(());
    }

    add_cargo_member(app_root, &crate_path.to_string_lossy(), false)?;
    let description = cargo_description(&crate_dir.join("Cargo.toml"))?
        .unwrap_or_else(|| format!("Generated {name} {}", kind.label()));
    if iam::is_iam_module(app_root, name)? {
        iam::ensure_iam_compose(app_root, name, false)?;
        return ensure_rust_package_scripts(app_root, kind, name, false);
    }
    if kind == RustProjectKind::Worker
        && let Some(backend) = worker::backend(app_root, name)?
    {
        add_project_to_manifest(
            app_root,
            kind,
            name,
            &description,
            &ArgMatches::default(),
            false,
        )?;
        worker::mark(app_root, name, backend)?;
        worker::compose(app_root, name, backend)?;
        return ensure_rust_package_scripts(app_root, kind, name, false);
    }
    let templates = rust_crate_templates(&crate_dir, name, &description, kind)
        .into_iter()
        .filter(|template| !template.path.exists())
        .collect::<Vec<_>>();
    write_rust_rendered_templates(
        &templates,
        false,
        &mut StandardStream::stdout(ColorChoice::Always),
    )?;
    add_project_to_manifest(
        app_root,
        kind,
        name,
        &description,
        &ArgMatches::default(),
        false,
    )?;
    ensure_rust_local_library_dependencies(app_root, false)?;
    ensure_rust_docker_compose_project(app_root, kind, name, false)?;
    ensure_rust_package_scripts(app_root, kind, name, false)
}

pub(crate) fn add_rust_workspace_member(app_root: &Path, member: &str) -> Result<()> {
    add_cargo_member(app_root, member, false)
}

pub(crate) fn sync_all_rust_projects(app_root: &Path, stdout: &mut StandardStream) -> Result<()> {
    let crates_dir = app_root.join("crates");
    if !crates_dir.exists() {
        log_warn!(
            stdout,
            "Rust crates directory does not exist: {}",
            crates_dir.display()
        );
        return Ok(());
    }

    for entry in std::fs::read_dir(&crates_dir)? {
        let entry = entry?;
        let crate_dir = entry.path();
        if !crate_dir.is_dir() || !crate_dir.join("Cargo.toml").exists() {
            continue;
        }
        let Some(name) = crate_dir.file_name().and_then(|part| part.to_str()) else {
            continue;
        };
        let kind = detect_rust_project_kind(app_root, name)?;
        sync_rust_project(app_root, kind, name)?;
        log_ok!(stdout, "Rust {} '{}' synced", kind.label(), name);
    }

    cleanup_missing_rust_projects(app_root)?;
    ensure_rust_local_library_dependencies(app_root, false)?;
    Ok(())
}

pub(crate) fn delete_rust_project(
    app_root: &Path,
    kind: RustProjectKind,
    name: &str,
) -> Result<()> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let mut manifest = read_manifest_value(&manifest_path)?;
    let crate_path = crate_path_for_project(&manifest, name)
        .unwrap_or_else(|| Path::new("crates").join(name).to_string_lossy().to_string());
    remove_project_from_manifest_value(&mut manifest, name, Some(kind))?;
    write_manifest_value(&manifest_path, &manifest)?;
    remove_cargo_member(app_root, &crate_path)?;
    cleanup_generated_outputs_for_deleted_rust_project(app_root, kind, name)?;
    remove_rust_docker_compose_project(app_root, name)?;
    remove_rust_package_scripts(app_root, name)?;
    ensure_rust_local_library_dependencies(app_root, false)?;

    let crate_dir = app_root.join(&crate_path);
    if crate_dir.exists() {
        remove_dir_all(&crate_dir)
            .with_context(|| format!("Failed to remove {}", crate_dir.display()))?;
    }

    Ok(())
}

fn cleanup_generated_outputs_for_deleted_rust_project(
    app_root: &Path,
    kind: RustProjectKind,
    name: &str,
) -> Result<()> {
    if !matches!(kind, RustProjectKind::Service | RustProjectKind::Worker) {
        return Ok(());
    }

    remove_rust_project_artifacts(app_root, name)?;

    let sdk_member = Path::new("sdks")
        .join("rust")
        .join(format!("{name}-sdk"))
        .to_string_lossy()
        .to_string();
    remove_cargo_member(app_root, &sdk_member)?;
    remove_path_if_exists(&app_root.join(&sdk_member))
}

pub(crate) fn remove_rust_project_artifacts(app_root: &Path, name: &str) -> Result<()> {
    for artifact_kind in ["contracts", "openapi", "asyncapi"] {
        remove_path_if_exists(&app_root.join(".forklaunch").join(artifact_kind).join(name))?;
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if path.is_dir() {
        remove_dir_all(path).with_context(|| format!("Failed to remove {}", path.display()))?;
    } else if path.exists() {
        remove_file(path).with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn change_rust_project(
    app_root: &Path,
    current_name: &str,
    new_name: Option<&str>,
    description: Option<&str>,
    database: Option<&str>,
    worker_type: Option<&str>,
    to_kind: Option<RustProjectKind>,
    dryrun: bool,
) -> Result<String> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let mut manifest = read_manifest_value(&manifest_path)?;
    let current_crate_path = crate_path_for_project(&manifest, current_name).unwrap_or_else(|| {
        Path::new("crates")
            .join(current_name)
            .to_string_lossy()
            .to_string()
    });
    let target_name = new_name.unwrap_or(current_name);
    let target_crate_path = Path::new("crates")
        .join(target_name)
        .to_string_lossy()
        .to_string();
    let current_kind = project_table(&manifest, current_name)
        .and_then(|project| project.get("type"))
        .and_then(Value::as_str)
        .and_then(kind_from_manifest_type)
        .unwrap_or(RustProjectKind::Library);
    let target_kind = to_kind.unwrap_or(current_kind);
    let current_backend = if current_kind == RustProjectKind::Worker {
        worker::backend(app_root, current_name)?
    } else {
        None
    };
    let persistent_worker = current_backend.is_some();
    let current_project = project_table(&manifest, current_name);
    let target_database = database
        .or_else(|| {
            current_project
                .and_then(|project| project.get("resources"))
                .and_then(|resources| resources.get("database"))
                .and_then(Value::as_str)
        })
        .unwrap_or("postgresql");
    let target_worker_type = worker_type
        .or_else(|| {
            (current_kind == RustProjectKind::Worker)
                .then(|| {
                    current_project
                        .and_then(|project| project.get("metadata"))
                        .and_then(|metadata| metadata.get("workerType"))
                        .and_then(Value::as_str)
                })
                .flatten()
        })
        .unwrap_or("database");
    let target_backend = worker::Backend::from_options(target_worker_type, target_database);
    let keep_persistent_worker = persistent_worker
        && target_kind == RustProjectKind::Worker
        && target_backend == current_backend;
    let convert_worker = target_kind == RustProjectKind::Worker
        && !keep_persistent_worker
        && (current_kind != target_kind || worker_type.is_some() || database.is_some());
    let persistent_iam = iam::is_iam_module(app_root, current_name)?;
    if persistent_iam
        && (!matches!(target_kind, RustProjectKind::Service)
            || database.is_some_and(|database| database != "postgresql"))
    {
        bail!("Persistent Rust IAM must remain a PostgreSQL service");
    }

    if current_name != target_name {
        if project_table(&manifest, target_name).is_some()
            || app_root.join(&target_crate_path).exists()
        {
            bail!("Project destination {target_name} already exists");
        }
        for relative in [
            format!(".forklaunch/contracts/{target_name}"),
            format!(".forklaunch/openapi/{target_name}"),
            format!(".forklaunch/asyncapi/{target_name}"),
            format!("sdks/rust/{target_name}-sdk"),
        ] {
            if app_root.join(&relative).exists() {
                bail!("Generated output destination {relative} already exists");
            }
        }
    }
    if dryrun {
        return Ok(target_name.to_string());
    }

    if current_name != target_name {
        let from = app_root.join(&current_crate_path);
        let to = app_root.join(&target_crate_path);
        if from.exists() {
            rename(&from, &to).with_context(|| {
                format!("Failed to rename {} to {}", from.display(), to.display())
            })?;
        }
        replace_cargo_member(app_root, &current_crate_path, &target_crate_path)?;
        update_package_name(&to.join("Cargo.toml"), target_name)?;
    }

    {
        let project = project_table_mut(&mut manifest, current_name)
            .with_context(|| format!("Could not find Rust project {current_name} in manifest"))?;
        project.insert(
            "type".to_string(),
            Value::String(target_kind.manifest_type().to_string()),
        );
        project.insert("name".to_string(), Value::String(target_name.to_string()));
        if let Some(description) = description {
            project.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        if let Some(database) = database {
            let resources = project
                .entry("resources")
                .or_insert_with(|| Value::Table(Map::new()))
                .as_table_mut()
                .context("Project resources must be a TOML table")?;
            resources.insert("database".to_string(), Value::String(database.to_string()));
        }
        let metadata = project
            .entry("metadata")
            .or_insert_with(|| Value::Table(Map::new()))
            .as_table_mut()
            .context("Project metadata must be a TOML table")?;
        metadata.insert("language".to_string(), Value::String("rust".to_string()));
        metadata.insert(
            "crateName".to_string(),
            Value::String(target_name.to_string()),
        );
        metadata.insert(
            "cratePath".to_string(),
            Value::String(target_crate_path.clone()),
        );
        metadata.insert(
            "contractArtifactPath".to_string(),
            Value::String(format!(".forklaunch/contracts/{target_name}/contract.json")),
        );
        metadata.insert(
            "openapiPath".to_string(),
            Value::String(format!(".forklaunch/openapi/{target_name}/openapi.json")),
        );
        metadata.insert(
            "asyncapiPath".to_string(),
            Value::String(format!(".forklaunch/asyncapi/{target_name}/asyncapi.json")),
        );
        if let Some(worker_type) = worker_type {
            metadata.insert(
                "workerType".to_string(),
                Value::String(worker_type.to_string()),
            );
        }
    }

    if persistent_worker && !keep_persistent_worker {
        project_table_mut(&mut manifest, target_name)
            .context("Expected worker")?
            .remove("variant");
    }
    write_manifest_value(&manifest_path, &manifest)?;

    let crate_dir = app_root.join(&target_crate_path);
    if keep_persistent_worker {
        if current_name != target_name {
            worker::rename_sources(app_root, current_name, target_name)?;
            rename_rust_project_outputs(app_root, current_name, target_name)?;
            remove_rust_package_scripts(app_root, current_name)?;
        }
        worker::compose(
            app_root,
            target_name,
            current_backend.context("Expected worker backend")?,
        )?;
        return ensure_rust_package_scripts(app_root, target_kind, target_name, false)
            .map(|()| target_name.to_owned());
    }
    if persistent_iam {
        if current_name != target_name {
            iam::rename_iam_sources(app_root, current_name, target_name)?;
            rename_rust_project_outputs(app_root, current_name, target_name)?;
        }
        ensure_rust_package_scripts(app_root, target_kind, target_name, false)?;
        iam::ensure_iam_compose(app_root, target_name, false)?;
        if current_name != target_name {
            remove_rust_package_scripts(app_root, current_name)?;
        }
        return Ok(target_name.to_owned());
    }
    match target_kind {
        RustProjectKind::Service => ensure_service_shape(&crate_dir, target_name)?,
        RustProjectKind::Worker => ensure_worker_shape(&crate_dir, target_name)?,
        RustProjectKind::Library => ensure_library_shape(&crate_dir, target_name)?,
    }
    if current_name != target_name || current_kind != target_kind {
        remove_rust_docker_compose_project(app_root, current_name)?;
        remove_rust_package_scripts(app_root, current_name)?;
    }
    ensure_rust_docker_compose_project(app_root, target_kind, target_name, false)?;
    ensure_rust_package_scripts(app_root, target_kind, target_name, false)?;
    ensure_rust_local_library_dependencies(app_root, false)?;
    if convert_worker && let Some(backend) = target_backend {
        worker::convert(app_root, target_name, backend)?;
    }
    if current_name != target_name {
        rename_rust_project_outputs(app_root, current_name, target_name)?;
    }

    Ok(target_name.to_string())
}

// Generated artifacts retain their schemas while their service metadata and SDK
// names follow the renamed project. Other services' artifacts are left intact.
fn rename_rust_project_outputs(root: &Path, old: &str, new: &str) -> Result<()> {
    for (kind, filename) in [
        ("contracts", "contract.json"),
        ("openapi", "openapi.json"),
        ("asyncapi", "asyncapi.json"),
    ] {
        let source = root.join(".forklaunch").join(kind).join(old);
        let destination = root.join(".forklaunch").join(kind).join(new);
        if !source.exists() {
            continue;
        }
        rename(&source, &destination)?;
        let path = destination.join(filename);
        if path.exists() {
            let mut artifact: JsonValue = serde_json::from_str(&read_to_string(&path)?)?;
            if let Some(name) = artifact.pointer_mut("/service/name") {
                *name = json!(new);
            }
            if let Some(title) = artifact.pointer_mut("/info/title") {
                if title.as_str() == Some(old) {
                    *title = json!(new);
                }
            }
            write(&path, serde_json::to_string_pretty(&artifact)?)?;
        }
    }
    let old_member = format!("sdks/rust/{old}-sdk");
    if root.join(&old_member).exists() {
        remove_cargo_member(root, &old_member)?;
        remove_path_if_exists(&root.join(&old_member))?;
        let source = if root
            .join(format!(".forklaunch/contracts/{new}/contract.json"))
            .exists()
        {
            "contract"
        } else {
            "manifest"
        };
        crate::sdk::generate::generate_or_check_sdk_outputs(
            root,
            "rust",
            source,
            Some(new),
            false,
            &mut StandardStream::stdout(ColorChoice::Always),
        )?;
    }
    crate::sdk::generate::regenerate_typescript_sdk_if_present(
        root,
        &mut StandardStream::stdout(ColorChoice::Always),
    )
}

pub(crate) fn change_rust_application(
    app_root: &Path,
    new_name: Option<&str>,
    description: Option<&str>,
    author: Option<&str>,
    license: Option<&str>,
    dryrun: bool,
) -> Result<String> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let mut manifest = read_manifest_value(&manifest_path)?;
    let current_name = manifest
        .get("app_name")
        .and_then(Value::as_str)
        .context("Rust application manifest is missing app_name")?
        .to_string();
    let target_name = new_name.unwrap_or(&current_name).to_string();

    if dryrun {
        return Ok(target_name);
    }

    let root = manifest
        .as_table_mut()
        .context("ForkLaunch manifest root must be a TOML table")?;
    if new_name.is_some() {
        root.insert("app_name".to_string(), Value::String(target_name.clone()));
    }
    if let Some(description) = description {
        root.insert(
            "app_description".to_string(),
            Value::String(description.to_string()),
        );
    }
    if let Some(author) = author {
        root.insert("author".to_string(), Value::String(author.to_string()));
    }
    if let Some(license) = license {
        root.insert("license".to_string(), Value::String(license.to_string()));
    }

    write_manifest_value(&manifest_path, &manifest)?;
    update_rust_root_cargo_metadata(app_root, author, license)?;
    update_rust_readme(app_root, &target_name, description)?;

    Ok(target_name)
}

pub(crate) fn change_rust_router(
    app_root: &Path,
    project_name: &str,
    existing_name: &str,
    new_name: &str,
    dryrun: bool,
) -> Result<()> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let mut manifest = read_manifest_value(&manifest_path)?;
    let crate_path = crate_path_for_project(&manifest, project_name).unwrap_or_else(|| {
        Path::new("crates")
            .join(project_name)
            .to_string_lossy()
            .to_string()
    });
    let routes_dir = app_root.join(&crate_path).join("src").join("routes");
    let old_module = existing_name.to_case(Case::Snake);
    let new_module = new_name.to_case(Case::Snake);

    if !dryrun {
        let old_file = routes_dir.join(format!("{old_module}.rs"));
        let new_file = routes_dir.join(format!("{new_module}.rs"));
        if old_file.exists() {
            rename(&old_file, &new_file).with_context(|| {
                format!(
                    "Failed to rename {} to {}",
                    old_file.display(),
                    new_file.display()
                )
            })?;
        }
        write(
            routes_dir.join(format!("{new_module}.rs")),
            router_rs(new_name),
        )
        .with_context(|| format!("Failed to update Rust router {new_name}"))?;
        rewrite_routes_mod(
            &routes_dir,
            false,
            &mut StandardStream::stdout(ColorChoice::Always),
        )?;
        replace_router_in_manifest_value(&mut manifest, project_name, existing_name, new_name)?;
        write_manifest_value(&manifest_path, &manifest)?;
    }

    Ok(())
}

pub(crate) fn delete_rust_router(
    app_root: &Path,
    project_name: &str,
    router_name: &str,
) -> Result<()> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let mut manifest = read_manifest_value(&manifest_path)?;
    let crate_path = crate_path_for_project(&manifest, project_name).unwrap_or_else(|| {
        Path::new("crates")
            .join(project_name)
            .to_string_lossy()
            .to_string()
    });
    let routes_dir = app_root.join(&crate_path).join("src").join("routes");
    let router_module = router_name.to_case(Case::Snake);
    let route_file = routes_dir.join(format!("{router_module}.rs"));
    if route_file.exists() {
        remove_file(&route_file)
            .with_context(|| format!("Failed to remove {}", route_file.display()))?;
    }
    rewrite_routes_mod(
        &routes_dir,
        false,
        &mut StandardStream::stdout(ColorChoice::Always),
    )?;
    remove_router_from_manifest_value(&mut manifest, project_name, router_name)?;
    write_manifest_value(&manifest_path, &manifest)?;
    remove_rust_project_artifacts(app_root, project_name)
}

fn generate_rust_project(matches: &ArgMatches, kind: RustProjectKind) -> Result<()> {
    let dryrun = matches.get_flag("dryrun");
    let name = required_name(matches, kind.label())?;
    let description = option(matches, "description")
        .unwrap_or_else(|| format!("Generated {name} {}", kind.label()));
    let app_root = find_manifest_root(
        &matches
            .get_one::<String>("base_path")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir().context("Failed to read current directory")?),
    )?;
    let crate_path = Path::new("crates").join(&name);
    let crate_dir = app_root.join(&crate_path);
    let manifest = read_manifest_value(&app_root.join(".forklaunch/manifest.toml"))?;
    if let Some(project) = project_table(&manifest, &name) {
        if !is_rust_project(&app_root, &name)?
            || project.get("type").and_then(Value::as_str) != Some(kind.manifest_type())
            || project.get("variant").and_then(Value::as_str) == Some("iam-base")
        {
            bail!(
                "Project {name} already exists with a different project kind or module; use its change command"
            );
        }
    } else if crate_dir.join("Cargo.toml").exists() {
        bail!(
            "An existing crate occupies the destination for {name}; sync it before initialization"
        );
    }
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    ensure_embedded_framework(&app_root, dryrun)?;
    let requested_backend = worker::Backend::from_options(
        option(matches, "type").as_deref().unwrap_or("database"),
        option(matches, "database")
            .as_deref()
            .unwrap_or("postgresql"),
    );
    let backend = if kind == RustProjectKind::Worker {
        if crate_dir.join("Cargo.toml").exists() {
            worker::backend(&app_root, &name)?
        } else {
            requested_backend
        }
    } else {
        None
    };
    let templates = if let Some(backend) = backend {
        iam::ensure_blueprint_sources(&app_root, dryrun)?;
        worker::templates(&crate_dir, &name, &description, backend)?
    } else {
        rust_crate_templates(&crate_dir, &name, &description, kind)
    };

    write_rust_rendered_templates(&templates, dryrun, &mut stdout)?;
    add_cargo_member(&app_root, &crate_path.to_string_lossy(), dryrun)?;
    add_project_to_manifest(&app_root, kind, &name, &description, matches, dryrun)?;
    ensure_rust_local_library_dependencies(&app_root, dryrun)?;
    ensure_rust_docker_compose_project(&app_root, kind, &name, dryrun)?;
    ensure_rust_package_scripts(&app_root, kind, &name, dryrun)?;
    if !dryrun && let Some(backend) = backend {
        worker::mark(&app_root, &name, backend)?;
        worker::compose(&app_root, &name, backend)?;
    }

    log_ok!(
        stdout,
        "Generated Rust {} {} at {}",
        kind.label(),
        name,
        crate_dir.display()
    );

    Ok(())
}

fn rust_crate_templates(
    crate_dir: &Path,
    name: &str,
    description: &str,
    kind: RustProjectKind,
) -> Vec<RenderedTemplate> {
    match kind {
        RustProjectKind::Service => service_templates(crate_dir, name, description),
        RustProjectKind::Worker => worker_templates(crate_dir, name, description),
        RustProjectKind::Library => library_templates(crate_dir, name, description),
    }
}

fn service_templates(crate_dir: &Path, name: &str, description: &str) -> Vec<RenderedTemplate> {
    let crate_module = name.to_case(Case::Snake);
    vec![
        RenderedTemplate {
            path: crate_dir.join("Cargo.toml"),
            content: project_cargo_toml(name, description, true),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("lib.rs"),
            content: service_lib_rs(name),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("main.rs"),
            content: service_main_rs(name),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("config").join("mod.rs"),
            content: service_config_rs(name),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("domain").join("mod.rs"),
            content: service_domain_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("error").join("mod.rs"),
            content: service_error_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("persistence").join("mod.rs"),
            content: service_persistence_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("routes").join("mod.rs"),
            content: routes_mod_rs(&["health".to_string()]),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("routes").join("health.rs"),
            content: health_route_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("tests").join("smoke.rs"),
            content: service_smoke_test_rs(&crate_module),
            context: None,
        },
    ]
}

fn worker_templates(crate_dir: &Path, name: &str, description: &str) -> Vec<RenderedTemplate> {
    let crate_module = name.to_case(Case::Snake);
    vec![
        RenderedTemplate {
            path: crate_dir.join("Cargo.toml"),
            content: project_cargo_toml(name, description, true),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("lib.rs"),
            content: worker_lib_rs(name),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("main.rs"),
            content: worker_main_rs(name),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("config").join("mod.rs"),
            content: worker_config_rs(name),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("error").join("mod.rs"),
            content: worker_error_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("jobs").join("mod.rs"),
            content: worker_jobs_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("tests").join("smoke.rs"),
            content: worker_smoke_test_rs(&crate_module),
            context: None,
        },
    ]
}

fn library_templates(crate_dir: &Path, name: &str, description: &str) -> Vec<RenderedTemplate> {
    let crate_module = name.to_case(Case::Snake);
    vec![
        RenderedTemplate {
            path: crate_dir.join("Cargo.toml"),
            content: project_cargo_toml(name, description, false),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("lib.rs"),
            content: library_lib_rs(name),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("error").join("mod.rs"),
            content: library_error_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("models").join("mod.rs"),
            content: library_models_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("src").join("service").join("mod.rs"),
            content: library_service_rs(),
            context: None,
        },
        RenderedTemplate {
            path: crate_dir.join("tests").join("smoke.rs"),
            content: library_smoke_test_rs(&crate_module),
            context: None,
        },
    ]
}

fn rust_crate_template_data(name: &str, description: &str, has_binary: bool) -> RustCrateTemplate {
    RustCrateTemplate {
        name: escape_toml_string(name),
        description: escape_toml_string(description),
        crate_module: name.to_case(Case::Snake),
        has_binary,
        uses_framework: has_binary,
        framework_rs_path: framework_rs_path(),
    }
}

fn framework_rs_path() -> String {
    "../../.forklaunch/framework-rs".to_string()
}

fn ensure_embedded_framework(app_root: &Path, dryrun: bool) -> Result<()> {
    if dryrun {
        return Ok(());
    }
    let root = app_root.join(".forklaunch/framework-rs");
    create_dir_all(&root)?;
    let cargo = root.join("Cargo.toml");
    if !cargo.exists() {
        write(cargo, FRAMEWORK_CARGO)?;
    }
    write_embedded_sources(&FRAMEWORK_CRATES, &root.join("crates"))
}

fn write_embedded_sources(dir: &Dir<'_>, root: &Path) -> Result<()> {
    for file in dir.files() {
        // Cargo excludes directories containing Cargo.toml from published CLI sources.
        // Restore manifest names when writing the embedded framework.
        let mut path = root.join(file.path());
        if path
            .file_name()
            .is_some_and(|name| name == "Cargo.toml.template")
        {
            path.set_file_name("Cargo.toml");
        }
        if !path.exists() {
            create_dir_all(path.parent().context("embedded source has no parent")?)?;
            write(&path, file.contents())?;
        }
    }
    for child in dir.dirs() {
        write_embedded_sources(child, root)?;
    }
    Ok(())
}

fn service_lib_rs(name: &str) -> String {
    render_rust_template(
        "rust/project/service/src/lib.rs",
        &rust_crate_template_data(name, "", true),
    )
}

fn service_main_rs(name: &str) -> String {
    render_rust_template(
        "rust/project/service/src/main.rs",
        &rust_crate_template_data(name, "", true),
    )
}

fn service_config_rs(name: &str) -> String {
    render_rust_template(
        "rust/project/service/src/config/mod.rs",
        &rust_crate_template_data(name, "", true),
    )
}

fn service_domain_rs() -> String {
    render_static_rust_template("rust/project/service/src/domain/mod.rs")
}

fn service_error_rs() -> String {
    render_static_rust_template("rust/project/service/src/error/mod.rs")
}

fn service_persistence_rs() -> String {
    render_static_rust_template("rust/project/service/src/persistence/mod.rs")
}

fn health_route_rs() -> String {
    render_static_rust_template("rust/project/service/src/routes/health.rs")
}

fn routes_mod_rs(modules: &[String]) -> String {
    let mut modules = modules.to_vec();
    modules.sort();
    modules.dedup();
    if let Some(health_index) = modules.iter().position(|module| module == "health") {
        let health = modules.remove(health_index);
        modules.insert(0, health);
    }

    render_rust_template(
        "rust/router/routes_mod.rs",
        &RustRoutesModTemplate {
            has_health: modules.iter().any(|module| module == "health"),
            modules: modules
                .into_iter()
                .map(|module| RustRouteModuleTemplate { module })
                .collect(),
        },
    )
}

fn router_rs(router_name: &str) -> String {
    render_rust_template(
        "rust/router/router.rs",
        &RustRouterTemplate {
            router_name: escape_rust_string(router_name),
            route_title: router_name.to_case(Case::Title),
            response_name: format!("{}Response", router_name.to_case(Case::Pascal)),
        },
    )
}

fn service_smoke_test_rs(crate_module: &str) -> String {
    render_rust_template(
        "rust/project/service/tests/smoke.rs",
        &RustCrateTemplate {
            name: String::new(),
            description: String::new(),
            crate_module: crate_module.to_string(),
            has_binary: false,
            uses_framework: false,
            framework_rs_path: framework_rs_path(),
        },
    )
}

fn worker_lib_rs(name: &str) -> String {
    render_rust_template(
        "rust/project/worker/src/lib.rs",
        &rust_crate_template_data(name, "", true),
    )
}

fn worker_main_rs(name: &str) -> String {
    render_rust_template(
        "rust/project/worker/src/main.rs",
        &rust_crate_template_data(name, "", true),
    )
}

fn worker_config_rs(name: &str) -> String {
    render_rust_template(
        "rust/project/worker/src/config/mod.rs",
        &rust_crate_template_data(name, "", true),
    )
}

fn worker_error_rs() -> String {
    render_static_rust_template("rust/project/worker/src/error/mod.rs")
}

fn worker_jobs_rs() -> String {
    render_static_rust_template("rust/project/worker/src/jobs/mod.rs")
}

fn worker_smoke_test_rs(crate_module: &str) -> String {
    render_rust_template(
        "rust/project/worker/tests/smoke.rs",
        &RustCrateTemplate {
            name: String::new(),
            description: String::new(),
            crate_module: crate_module.to_string(),
            has_binary: false,
            uses_framework: false,
            framework_rs_path: framework_rs_path(),
        },
    )
}

fn library_lib_rs(name: &str) -> String {
    render_rust_template(
        "rust/project/library/src/lib.rs",
        &rust_crate_template_data(name, "", false),
    )
}

fn library_error_rs() -> String {
    render_static_rust_template("rust/project/library/src/error/mod.rs")
}

fn library_models_rs() -> String {
    render_static_rust_template("rust/project/library/src/models/mod.rs")
}

fn library_service_rs() -> String {
    render_static_rust_template("rust/project/library/src/service/mod.rs")
}

fn library_smoke_test_rs(crate_module: &str) -> String {
    render_rust_template(
        "rust/project/library/tests/smoke.rs",
        &RustCrateTemplate {
            name: String::new(),
            description: String::new(),
            crate_module: crate_module.to_string(),
            has_binary: false,
            uses_framework: false,
            framework_rs_path: framework_rs_path(),
        },
    )
}

fn required_name(matches: &ArgMatches, label: &str) -> Result<String> {
    let name = matches
        .get_one::<String>("name")
        .with_context(|| format!("Missing {label} name"))?;
    if !validate_name(name) {
        bail!("{label} name cannot be empty or include numbers or spaces");
    }
    Ok(name.clone())
}

fn option(matches: &ArgMatches, id: &str) -> Option<String> {
    matches.try_get_one::<String>(id).ok().flatten().cloned()
}

fn render_rust_template<T: Content>(template_path: &str, data: &T) -> String {
    let template_file = TEMPLATES_DIR
        .get_file(template_path)
        .unwrap_or_else(|| panic!("embedded Rust template {template_path} is missing"));
    let template = Template::new(
        template_file
            .contents_utf8()
            .expect("embedded Rust templates must be UTF-8"),
    )
    .expect("embedded Rust template should compile");

    template.render(data)
}

fn render_static_rust_template(template_path: &str) -> String {
    render_rust_template(template_path, &EmptyRustTemplate {})
}

fn root_cargo_toml(license: &str, author: &str) -> String {
    let authors = if author.trim().is_empty() {
        "[]".to_string()
    } else {
        format!("[\"{}\"]", escape_toml_string(author))
    };
    render_rust_template(
        "rust/application/Cargo.toml.template",
        &RustRootCargoTemplate {
            license: escape_toml_string(license),
            authors,
        },
    )
}

fn project_cargo_toml(name: &str, description: &str, has_binary: bool) -> String {
    render_rust_template(
        "rust/project/Cargo.toml.template",
        &rust_crate_template_data(name, description, has_binary),
    )
}

fn application_manifest(
    name: &str,
    description: &str,
    author: &str,
    license: &str,
    language: &str,
    sdk_targets: &[String],
) -> String {
    render_rust_template(
        "rust/application/manifest.toml",
        &RustApplicationManifestTemplate {
            id: Uuid::new_v4().to_string(),
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            name: escape_toml_string(name),
            description: escape_toml_string(description),
            author: escape_toml_string(author),
            license: escape_toml_string(license),
            language: language.to_string(),
            sdk_targets: sdk_targets_toml_array(sdk_targets),
        },
    )
}

fn mixed_typescript_sdk_templates(root_path: &Path, name: &str) -> Vec<RenderedTemplate> {
    let package_name = name.to_case(Case::Kebab);
    vec![
        RenderedTemplate {
            path: root_path.join("package.json"),
            content: serde_json::to_string_pretty(&json!({
                "name": package_name,
                "private": true,
                "type": "module",
                "packageManager": "pnpm@10.0.0",
                "scripts": {
                    "rust:build": "cargo build --workspace",
                    "rust:test": "cargo test --workspace",
                    "rust:fmt": "cargo fmt --all",
                    "rust:lint": "cargo clippy --workspace --all-targets",
                    "sdk:generate": "forklaunch sdk generate --target typescript --from contract",
                    "sdk:check": "forklaunch sdk generate --target typescript --from contract --check",
                    "sdk:typecheck": "pnpm --filter @forklaunch-generated/sdk build",
                    "sdk:watch": "forklaunch sdk watch --target typescript --from contract"
                }
            }))
            .unwrap(),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("pnpm-workspace.yaml"),
            content: "packages:\n  - \"sdks/typescript\"\n".to_string(),
            context: None,
        },
        RenderedTemplate {
            path: root_path
                .join("sdks")
                .join("typescript")
                .join("package.json"),
            content: serde_json::to_string_pretty(&json!({
                "name": "@forklaunch-generated/sdk",
                "version": "0.0.0",
                "private": true,
                "type": "module",
                "exports": {
                    ".": "./src/index.ts"
                },
                "scripts": {
                    "build": "tsc --noEmit"
                },
                "devDependencies": {
                    "typescript": "^5.0.0"
                }
            }))
            .unwrap(),
            context: None,
        },
        RenderedTemplate {
            path: root_path.join("sdks").join("typescript").join("tsconfig.json"),
            content: TYPESCRIPT_SDK_TSCONFIG.to_owned(),
            context: None,
        },
        RenderedTemplate {
            path: root_path
                .join("sdks")
                .join("typescript")
                .join("src")
                .join("index.ts"),
            content: "export type ForkLaunchSdk = Record<string, never>;\n\nexport const sdk = {} as ForkLaunchSdk;\n".to_string(),
            context: None,
        },
    ]
}

fn rust_docker_compose_yaml(app_name: &str) -> Result<String> {
    let mut compose = DockerCompose::default();
    ensure_rust_docker_network(&mut compose, app_name);
    yaml_to_string(&compose).context("Failed to render Rust docker-compose.yaml")
}

fn ensure_rust_docker_compose_project(
    app_root: &Path,
    kind: RustProjectKind,
    name: &str,
    dryrun: bool,
) -> Result<()> {
    if dryrun || matches!(kind, RustProjectKind::Library) {
        return Ok(());
    }

    let compose_path = app_root.join("docker-compose.yaml");
    let mut compose = read_or_create_rust_docker_compose(&compose_path)?;
    let app_name = rust_app_name(app_root)?;
    ensure_rust_docker_network(&mut compose, &app_name);

    let port = if matches!(kind, RustProjectKind::Service) {
        Some(next_rust_docker_port(&compose))
    } else {
        None
    };
    compose
        .services
        .entry(name.to_string())
        .or_insert_with(|| rust_docker_service(&app_name, name, kind, port));

    write(&compose_path, yaml_to_string(&compose)?)
        .with_context(|| format!("Failed to update {}", compose_path.display()))
}

fn remove_rust_docker_compose_project(app_root: &Path, name: &str) -> Result<()> {
    let compose_path = app_root.join("docker-compose.yaml");
    if !compose_path.exists() {
        return Ok(());
    }
    let mut compose = read_or_create_rust_docker_compose(&compose_path)?;
    compose.services.shift_remove(name);
    write(&compose_path, yaml_to_string(&compose)?)
        .with_context(|| format!("Failed to update {}", compose_path.display()))
}

fn read_or_create_rust_docker_compose(compose_path: &Path) -> Result<DockerCompose> {
    if compose_path.exists() {
        let content = read_to_string(compose_path)
            .with_context(|| format!("Failed to read {}", compose_path.display()))?;
        yaml_from_str::<DockerCompose>(&content)
            .with_context(|| format!("Failed to parse {}", compose_path.display()))
    } else {
        Ok(DockerCompose::default())
    }
}

fn ensure_rust_docker_network(compose: &mut DockerCompose, app_name: &str) {
    let network_name = format!("{app_name}-network");
    compose
        .networks
        .entry(network_name.clone())
        .or_insert_with(|| DockerNetwork {
            name: network_name,
            driver: "bridge".to_string(),
            ..Default::default()
        });
}

fn rust_docker_service(
    app_name: &str,
    name: &str,
    kind: RustProjectKind,
    port: Option<i32>,
) -> DockerService {
    let mut environment = IndexMap::new();
    environment.insert("HOST".to_string(), "0.0.0.0".to_string());
    environment.insert("PROTOCOL".to_string(), "http".to_string());
    environment.insert("VERSION".to_string(), "v1".to_string());
    environment.insert("DOCS_PATH".to_string(), "/docs".to_string());
    environment.insert("RUST_LOG".to_string(), "info".to_string());
    if let Some(port) = port {
        environment.insert("PORT".to_string(), port.to_string());
    }
    if matches!(kind, RustProjectKind::Worker) {
        environment.insert("QUEUE_NAME".to_string(), format!("{app_name}-{name}-dev"));
    }

    DockerService {
        hostname: Some(name.to_string()),
        container_name: Some(format!("{app_name}-{name}-rust")),
        image: Some(format!("{app_name}-{name}-rust:latest")),
        restart: Some(Restart::UnlessStopped),
        build: Some(DockerBuild {
            context: ".".to_string(),
            dockerfile: "./Dockerfile".to_string(),
        }),
        environment: Some(environment),
        ports: port.map(|port| vec![format!("{port}:{port}")]),
        networks: Some(vec![format!("{app_name}-network")]),
        working_dir: Some("/app".to_string()),
        command: Some(DockerCommand::Multiple(vec![format!(
            "./target/release/{name}"
        )])),
        healthcheck: port.map(|port| Healthcheck {
            test: HealthTest::List(vec![
                "CMD".to_string(),
                "curl".to_string(),
                "-f".to_string(),
                format!("http://localhost:{port}/health"),
            ]),
            interval: "30s".to_string(),
            timeout: "10s".to_string(),
            retries: 3,
            start_period: "40s".to_string(),
            additional_properties: Default::default(),
        }),
        ..Default::default()
    }
}

fn next_rust_docker_port(compose: &DockerCompose) -> i32 {
    compose
        .services
        .values()
        .flat_map(|service| service.ports.as_deref().unwrap_or_default())
        .filter_map(|port| port.split(':').next()?.parse::<i32>().ok())
        .filter(|port| (8000..9000).contains(port))
        .max()
        .unwrap_or(7999)
        + 1
}

fn rust_app_name(app_root: &Path) -> Result<String> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let manifest = read_manifest_value(&manifest_path)?;
    Ok(manifest
        .get("app_name")
        .and_then(Value::as_str)
        .unwrap_or("forklaunch")
        .to_case(Case::Kebab))
}

fn ensure_rust_package_scripts(
    app_root: &Path,
    kind: RustProjectKind,
    name: &str,
    dryrun: bool,
) -> Result<()> {
    if dryrun {
        return Ok(());
    }

    let package_path = app_root.join("package.json");
    if !package_path.exists() {
        return Ok(());
    }

    let mut package = read_package_json(&package_path)?;
    let package_object = package
        .as_object_mut()
        .context("Root package.json must be a JSON object")?;
    let scripts = package_object
        .entry("scripts".to_string())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("Root package.json scripts must be a JSON object")?;

    scripts.insert(
        "rust:build".to_string(),
        JsonValue::String("cargo build --workspace".to_string()),
    );
    scripts.insert(
        "rust:test".to_string(),
        JsonValue::String("cargo test --workspace".to_string()),
    );
    scripts.insert(
        "rust:fmt".to_string(),
        JsonValue::String("cargo fmt --all".to_string()),
    );
    scripts.insert(
        "rust:lint".to_string(),
        JsonValue::String("cargo clippy --workspace --all-targets".to_string()),
    );
    scripts.insert(
        format!("build:{name}"),
        JsonValue::String(format!("cargo build -p {name}")),
    );
    scripts.insert(
        format!("test:{name}"),
        JsonValue::String(format!("cargo test -p {name}")),
    );
    if matches!(kind, RustProjectKind::Service | RustProjectKind::Worker) {
        scripts.insert(
            format!("dev:{name}"),
            JsonValue::String(format!("cargo run -p {name}")),
        );
    }

    write(&package_path, serde_json::to_string_pretty(&package)?)
        .with_context(|| format!("Failed to update {}", package_path.display()))
}

fn remove_rust_package_scripts(app_root: &Path, name: &str) -> Result<()> {
    let package_path = app_root.join("package.json");
    if !package_path.exists() {
        return Ok(());
    }

    let mut package = read_package_json(&package_path)?;
    if let Some(scripts) = package
        .get_mut("scripts")
        .and_then(JsonValue::as_object_mut)
    {
        for script in [
            format!("dev:{name}"),
            format!("build:{name}"),
            format!("test:{name}"),
        ] {
            scripts.remove(&script);
        }
    }

    write(&package_path, serde_json::to_string_pretty(&package)?)
        .with_context(|| format!("Failed to update {}", package_path.display()))
}

fn read_package_json(package_path: &Path) -> Result<JsonValue> {
    serde_json::from_str::<JsonValue>(
        &read_to_string(package_path)
            .with_context(|| format!("Failed to read {}", package_path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", package_path.display()))
}

fn find_manifest_root(start: &Path) -> Result<PathBuf> {
    let start = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir()
            .context("Failed to read current directory")?
            .join(start)
    };
    let mut cursor = if start.is_file() {
        start
            .parent()
            .context("Failed to resolve parent directory")?
            .to_path_buf()
    } else {
        start
    };

    loop {
        if cursor.join(".forklaunch").join("manifest.toml").exists() {
            return Ok(cursor);
        }
        if !cursor.pop() {
            bail!("Could not find .forklaunch/manifest.toml for Rust project generation");
        }
    }
}

fn edit_cargo_members(root: &Path, update: impl FnOnce(&mut toml_edit::Array)) -> Result<()> {
    let path = root.join("Cargo.toml");
    let mut cargo: toml_edit::DocumentMut = read_to_string(&path)?.parse()?;
    let members = cargo["workspace"]["members"]
        .as_array_mut()
        .context("Expected workspace.members array")?;
    update(members);
    write(path, cargo.to_string())?;
    Ok(())
}

fn add_cargo_member(app_root: &Path, member: &str, dryrun: bool) -> Result<()> {
    if dryrun {
        return Ok(());
    }
    edit_cargo_members(app_root, |members| {
        if !members.iter().any(|value| value.as_str() == Some(member)) {
            members.push(member);
        }
    })
}

fn add_project_to_manifest(
    app_root: &Path,
    kind: RustProjectKind,
    name: &str,
    description: &str,
    matches: &ArgMatches,
    dryrun: bool,
) -> Result<()> {
    if dryrun {
        return Ok(());
    }

    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let mut manifest = read_manifest_value(&manifest_path)?;
    let sdk_targets = sdk_targets_from_manifest(&manifest);
    let root = manifest
        .as_table_mut()
        .context("ForkLaunch manifest root must be a TOML table")?;
    let projects = root
        .entry("projects")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .context("ForkLaunch manifest projects must be an array")?;

    if !projects.iter().any(|project| {
        project
            .get("name")
            .and_then(Value::as_str)
            .map(|project_name| project_name == name)
            .unwrap_or(false)
    }) {
        projects.push(project_manifest_entry(
            kind,
            name,
            description,
            matches,
            &sdk_targets,
        ));
    }

    write_manifest_value(&manifest_path, &manifest)
}

fn add_router_to_manifest(
    app_root: &Path,
    service_name: &str,
    router_name: &str,
    dryrun: bool,
) -> Result<()> {
    if dryrun {
        return Ok(());
    }

    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let mut manifest = read_manifest_value(&manifest_path)?;
    let projects = manifest
        .as_table_mut()
        .context("ForkLaunch manifest root must be a TOML table")?
        .entry("projects")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .context("ForkLaunch manifest projects must be an array")?;

    let Some(project) = projects.iter_mut().find(|project| {
        project
            .get("name")
            .and_then(Value::as_str)
            .map(|project_name| project_name == service_name)
            .unwrap_or(false)
    }) else {
        bail!("Could not find project {service_name} in ForkLaunch manifest");
    };

    let project_table = project
        .as_table_mut()
        .context("ForkLaunch project entry must be a TOML table")?;
    let routers = project_table
        .entry("routers")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .context("ForkLaunch project routers must be an array")?;
    if !routers
        .iter()
        .any(|router| router.as_str() == Some(router_name))
    {
        routers.push(Value::String(router_name.to_string()));
    }

    write_manifest_value(&manifest_path, &manifest)
}

fn project_manifest_entry(
    kind: RustProjectKind,
    name: &str,
    description: &str,
    matches: &ArgMatches,
    sdk_targets: &[String],
) -> Value {
    let mut table = Map::new();
    let crate_path = format!("crates/{name}");
    table.insert(
        "type".to_string(),
        Value::String(kind.manifest_type().to_string()),
    );
    table.insert("name".to_string(), Value::String(name.to_string()));
    table.insert(
        "description".to_string(),
        Value::String(description.to_string()),
    );
    table.insert("routers".to_string(), Value::Array(Vec::new()));

    if let Some(resources) = resources_for_project(kind, matches) {
        table.insert("resources".to_string(), Value::Table(resources));
    }

    let mut metadata = Map::new();
    metadata.insert("language".to_string(), Value::String("rust".to_string()));
    metadata.insert("crateName".to_string(), Value::String(name.to_string()));
    metadata.insert("cratePath".to_string(), Value::String(crate_path));
    metadata.insert(
        "contractArtifactPath".to_string(),
        Value::String(format!(".forklaunch/contracts/{name}/contract.json")),
    );
    metadata.insert(
        "openapiPath".to_string(),
        Value::String(format!(".forklaunch/openapi/{name}/openapi.json")),
    );
    metadata.insert(
        "asyncapiPath".to_string(),
        Value::String(format!(".forklaunch/asyncapi/{name}/asyncapi.json")),
    );
    metadata.insert(
        "sdkTargets".to_string(),
        Value::Array(sdk_targets.iter().cloned().map(Value::String).collect()),
    );
    if matches!(kind, RustProjectKind::Worker)
        && let Some(worker_type) = option(matches, "type")
    {
        metadata.insert("workerType".to_string(), Value::String(worker_type));
    }
    table.insert("metadata".to_string(), Value::Table(metadata));

    Value::Table(table)
}

fn resources_for_project(
    kind: RustProjectKind,
    matches: &ArgMatches,
) -> Option<Map<String, Value>> {
    let mut resources = Map::new();
    match kind {
        RustProjectKind::Service => {
            let database = option(matches, "database").unwrap_or_else(|| "postgresql".to_string());
            resources.insert("database".to_string(), Value::String(database));
        }
        RustProjectKind::Worker => {
            let worker_type = option(matches, "type").unwrap_or_else(|| "database".to_string());
            match worker_type.as_str() {
                "database" => {
                    let database =
                        option(matches, "database").unwrap_or_else(|| "postgresql".to_string());
                    resources.insert("database".to_string(), Value::String(database));
                }
                "redis" | "bullmq" => {
                    resources.insert("cache".to_string(), Value::String("redis".to_string()));
                }
                "kafka" => {
                    resources.insert("queue".to_string(), Value::String("kafka".to_string()));
                }
                _ => {}
            }
        }
        RustProjectKind::Library => {}
    }

    if resources.is_empty() {
        None
    } else {
        Some(resources)
    }
}

fn project_table<'a>(manifest: &'a Value, name: &str) -> Option<&'a Map<String, Value>> {
    manifest
        .get("projects")
        .and_then(Value::as_array)?
        .iter()
        .find(|project| {
            project
                .get("name")
                .and_then(Value::as_str)
                .map(|project_name| project_name == name)
                .unwrap_or(false)
        })?
        .as_table()
}

fn project_table_mut<'a>(
    manifest: &'a mut Value,
    name: &str,
) -> Option<&'a mut Map<String, Value>> {
    manifest
        .get_mut("projects")
        .and_then(Value::as_array_mut)?
        .iter_mut()
        .find(|project| {
            project
                .get("name")
                .and_then(Value::as_str)
                .map(|project_name| project_name == name)
                .unwrap_or(false)
        })?
        .as_table_mut()
}

fn crate_path_for_project(manifest: &Value, name: &str) -> Option<String> {
    project_table(manifest, name)
        .and_then(|project| project.get("metadata"))
        .and_then(Value::as_table)
        .and_then(|metadata| metadata.get("cratePath"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn ensure_rust_local_library_dependencies(app_root: &Path, dryrun: bool) -> Result<()> {
    if dryrun {
        return Ok(());
    }

    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    if !manifest_path.exists() {
        return Ok(());
    }

    let manifest = read_manifest_value(&manifest_path)?;
    let projects = rust_workspace_projects(&manifest);
    let libraries = projects
        .iter()
        .filter(|project| project.kind == RustProjectKind::Library)
        .cloned()
        .collect::<Vec<_>>();

    for project in projects.iter().filter(|project| {
        matches!(
            project.kind,
            RustProjectKind::Service | RustProjectKind::Worker
        )
    }) {
        update_rust_project_local_library_dependencies(app_root, project, &libraries)?;
    }

    Ok(())
}

fn rust_workspace_projects(manifest: &Value) -> Vec<RustWorkspaceProject> {
    manifest
        .get("projects")
        .and_then(Value::as_array)
        .map(|projects| {
            projects
                .iter()
                .filter_map(|project| {
                    let name = project.get("name").and_then(Value::as_str)?.to_string();
                    let kind = project
                        .get("type")
                        .and_then(Value::as_str)
                        .and_then(kind_from_manifest_type)?;
                    let metadata = project.get("metadata").and_then(Value::as_table)?;
                    if metadata.get("language").and_then(Value::as_str) != Some("rust") {
                        return None;
                    }
                    let crate_path = metadata
                        .get("cratePath")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("crates/{name}"));

                    Some(RustWorkspaceProject {
                        name,
                        kind,
                        crate_path,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn update_rust_project_local_library_dependencies(
    app_root: &Path,
    project: &RustWorkspaceProject,
    libraries: &[RustWorkspaceProject],
) -> Result<()> {
    let cargo_path = app_root.join(&project.crate_path).join("Cargo.toml");
    if !cargo_path.exists() {
        return Ok(());
    }

    let content = read_to_string(&cargo_path)
        .with_context(|| format!("Failed to read {}", cargo_path.display()))?;
    let mut cargo = toml::from_str::<Value>(&content)
        .with_context(|| format!("Failed to parse {}", cargo_path.display()))?;
    let root = cargo
        .as_table_mut()
        .context("Cargo.toml must be a TOML table")?;
    let dependencies = root
        .entry("dependencies")
        .or_insert_with(|| Value::Table(Map::new()))
        .as_table_mut()
        .context("Cargo.toml dependencies must be a TOML table")?;

    let library_names = libraries
        .iter()
        .map(|library| library.name.as_str())
        .collect::<HashSet<_>>();
    let library_paths = libraries
        .iter()
        .map(|library| normalize_path(app_root.join(&library.crate_path)))
        .collect::<HashSet<_>>();
    let crates_root = normalize_path(app_root.join("crates"));
    let project_dir = app_root.join(&project.crate_path);

    dependencies.retain(|dependency_name, dependency| {
        if library_names.contains(dependency_name) {
            return true;
        }

        let Some(path) = dependency_path(dependency) else {
            return true;
        };
        let resolved_path = normalize_path(project_dir.join(path));
        if resolved_path.starts_with(&crates_root) {
            return library_paths.contains(&resolved_path);
        }
        true
    });

    for library in libraries {
        if library.name == project.name {
            continue;
        }
        let mut dependency = Map::new();
        dependency.insert(
            "path".to_string(),
            Value::String(relative_cargo_path(
                &project.crate_path,
                &library.crate_path,
            )),
        );
        dependencies.insert(library.name.clone(), Value::Table(dependency));
    }

    write(&cargo_path, toml::to_string_pretty(&cargo)?)
        .with_context(|| format!("Failed to update {}", cargo_path.display()))
}

fn dependency_path(dependency: &Value) -> Option<&str> {
    dependency
        .as_table()
        .and_then(|table| table.get("path"))
        .and_then(Value::as_str)
}

fn relative_cargo_path(from_crate_path: &str, to_crate_path: &str) -> String {
    let from = normal_relative_components(from_crate_path);
    let to = normal_relative_components(to_crate_path);
    let common = from
        .iter()
        .zip(to.iter())
        .take_while(|(left, right)| left == right)
        .count();

    let mut path_parts = Vec::new();
    for _ in common..from.len() {
        path_parts.push("..".to_string());
    }
    path_parts.extend(to[common..].iter().cloned());

    if path_parts.is_empty() {
        ".".to_string()
    } else {
        path_parts.join("/")
    }
}

fn normal_relative_components(path: &str) -> Vec<String> {
    Path::new(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str().map(str::to_string),
            _ => None,
        })
        .collect()
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn kind_from_manifest_type(manifest_type: &str) -> Option<RustProjectKind> {
    match manifest_type {
        "Service" => Some(RustProjectKind::Service),
        "Worker" => Some(RustProjectKind::Worker),
        "Library" => Some(RustProjectKind::Library),
        _ => None,
    }
}

fn detect_rust_project_kind(app_root: &Path, name: &str) -> Result<RustProjectKind> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let manifest = read_manifest_value(&manifest_path)?;
    if let Some(kind) = project_table(&manifest, name)
        .and_then(|project| project.get("type"))
        .and_then(Value::as_str)
        .and_then(kind_from_manifest_type)
    {
        return Ok(kind);
    }

    let crate_dir = app_root.join("crates").join(name);
    if crate_dir.join("src").join("jobs").join("mod.rs").exists() {
        return Ok(RustProjectKind::Worker);
    }
    if crate_dir.join("src").join("routes").join("mod.rs").exists() {
        return Ok(RustProjectKind::Service);
    }
    Ok(RustProjectKind::Library)
}

fn remove_project_from_manifest_value(
    manifest: &mut Value,
    name: &str,
    kind: Option<RustProjectKind>,
) -> Result<()> {
    let projects = manifest
        .as_table_mut()
        .context("ForkLaunch manifest root must be a TOML table")?
        .entry("projects")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .context("ForkLaunch manifest projects must be an array")?;
    let before_len = projects.len();
    projects.retain(|project| {
        let same_name = project
            .get("name")
            .and_then(Value::as_str)
            .map(|project_name| project_name == name)
            .unwrap_or(false);
        let same_kind = kind
            .map(|kind| {
                project
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|project_type| project_type == kind.manifest_type())
                    .unwrap_or(false)
            })
            .unwrap_or(true);
        !(same_name && same_kind)
    });
    if projects.len() == before_len {
        bail!("Could not find Rust project {name} in manifest");
    }
    Ok(())
}

fn cleanup_missing_rust_projects(app_root: &Path) -> Result<()> {
    let manifest_path = app_root.join(".forklaunch").join("manifest.toml");
    let mut manifest = read_manifest_value(&manifest_path)?;
    let missing_projects = manifest
        .get("projects")
        .and_then(Value::as_array)
        .map(|projects| {
            projects
                .iter()
                .filter_map(|project| {
                    let name = project.get("name").and_then(Value::as_str)?;
                    let metadata = project.get("metadata").and_then(Value::as_table)?;
                    if metadata.get("language").and_then(Value::as_str) != Some("rust") {
                        return None;
                    }
                    let crate_path = metadata
                        .get("cratePath")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("crates/{name}"));
                    if app_root.join(&crate_path).join("Cargo.toml").exists() {
                        None
                    } else {
                        Some((name.to_string(), crate_path))
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for (name, crate_path) in &missing_projects {
        remove_project_from_manifest_value(&mut manifest, name, None)?;
        remove_cargo_member(app_root, crate_path)?;
    }

    if !missing_projects.is_empty() {
        write_manifest_value(&manifest_path, &manifest)?;
    }

    Ok(())
}

fn cargo_description(cargo_path: &Path) -> Result<Option<String>> {
    let content = read_to_string(cargo_path)
        .with_context(|| format!("Failed to read {}", cargo_path.display()))?;
    let value = toml::from_str::<Value>(&content)
        .with_context(|| format!("Failed to parse {}", cargo_path.display()))?;
    Ok(value
        .get("package")
        .and_then(Value::as_table)
        .and_then(|package| package.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn remove_cargo_member(app_root: &Path, member: &str) -> Result<()> {
    if !app_root.join("Cargo.toml").exists() {
        return Ok(());
    }
    edit_cargo_members(app_root, |members| {
        members.retain(|value| value.as_str() != Some(member))
    })
}

fn replace_cargo_member(app_root: &Path, current_member: &str, target_member: &str) -> Result<()> {
    edit_cargo_members(app_root, |members| {
        for index in 0..members.len() {
            if members.get(index).and_then(toml_edit::Value::as_str) == Some(current_member) {
                members.replace(index, target_member);
            }
        }
        if !members
            .iter()
            .any(|value| value.as_str() == Some(target_member))
        {
            members.push(target_member);
        }
    })
}

fn update_package_name(cargo_path: &Path, name: &str) -> Result<()> {
    if !cargo_path.exists() {
        return Ok(());
    }
    let contents = read_to_string(cargo_path)
        .with_context(|| format!("Failed to read {}", cargo_path.display()))?;
    let mut in_named_section = false;
    let mut updated = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" || trimmed == "[[bin]]" {
            in_named_section = true;
            updated.push(line.to_string());
            continue;
        }
        if in_named_section && trimmed.starts_with('[') {
            in_named_section = false;
        }
        if in_named_section && line.trim_start().starts_with("name = ") {
            updated.push(format!("name = \"{}\"", escape_toml_string(name)));
        } else {
            updated.push(line.to_string());
        }
    }
    write(cargo_path, updated.join("\n") + "\n")
        .with_context(|| format!("Failed to update {}", cargo_path.display()))
}

fn update_rust_root_cargo_metadata(
    app_root: &Path,
    author: Option<&str>,
    license: Option<&str>,
) -> Result<()> {
    if author.is_none() && license.is_none() {
        return Ok(());
    }

    let cargo_path = app_root.join("Cargo.toml");
    let mut cargo = toml::from_str::<Value>(
        &read_to_string(&cargo_path)
            .with_context(|| format!("Failed to read {}", cargo_path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", cargo_path.display()))?;
    let workspace = cargo
        .as_table_mut()
        .context("Root Cargo.toml must be a TOML table")?
        .entry("workspace")
        .or_insert_with(|| Value::Table(Map::new()))
        .as_table_mut()
        .context("Root Cargo.toml workspace must be a TOML table")?;
    let workspace_package = workspace
        .entry("package")
        .or_insert_with(|| Value::Table(Map::new()))
        .as_table_mut()
        .context("Root Cargo.toml workspace.package must be a TOML table")?;

    if let Some(author) = author {
        workspace_package.insert(
            "authors".to_string(),
            Value::Array(if author.trim().is_empty() {
                Vec::new()
            } else {
                vec![Value::String(author.to_string())]
            }),
        );
    }
    if let Some(license) = license {
        workspace_package.insert("license".to_string(), Value::String(license.to_string()));
    }

    write(&cargo_path, toml::to_string_pretty(&cargo)?)
        .with_context(|| format!("Failed to update {}", cargo_path.display()))
}

fn update_rust_readme(app_root: &Path, target_name: &str, description: Option<&str>) -> Result<()> {
    let readme_path = app_root.join("README.md");
    if !readme_path.exists() {
        return Ok(());
    }
    let existing = read_to_string(&readme_path)
        .with_context(|| format!("Failed to read {}", readme_path.display()))?;
    let current_description = existing
        .lines()
        .skip_while(|line| line.starts_with('#') || line.trim().is_empty())
        .next()
        .unwrap_or_default();
    write(
        &readme_path,
        format!(
            "# {target_name}\n\n{}\n",
            description.unwrap_or(current_description)
        ),
    )
    .with_context(|| format!("Failed to update {}", readme_path.display()))
}

fn ensure_service_shape(crate_dir: &Path, name: &str) -> Result<()> {
    let crate_module = name.to_case(Case::Snake);
    create_dir_all(crate_dir.join("src").join("routes"))?;
    create_dir_all(crate_dir.join("src").join("domain"))?;
    create_dir_all(crate_dir.join("src").join("persistence"))?;
    create_dir_all(crate_dir.join("src").join("config"))?;
    create_dir_all(crate_dir.join("src").join("error"))?;
    create_dir_all(crate_dir.join("tests"))?;
    write(crate_dir.join("src").join("lib.rs"), service_lib_rs(name))?;
    write(crate_dir.join("src").join("main.rs"), service_main_rs(name))?;
    write(
        crate_dir.join("src").join("routes").join("mod.rs"),
        routes_mod_rs(&rust_route_modules(&crate_dir.join("src").join("routes"))?),
    )?;
    write(
        crate_dir.join("src").join("routes").join("health.rs"),
        health_route_rs(),
    )?;
    write(
        crate_dir.join("src").join("domain").join("mod.rs"),
        service_domain_rs(),
    )?;
    write(
        crate_dir.join("src").join("error").join("mod.rs"),
        service_error_rs(),
    )?;
    write(
        crate_dir.join("src").join("persistence").join("mod.rs"),
        service_persistence_rs(),
    )?;
    write(
        crate_dir.join("src").join("config").join("mod.rs"),
        service_config_rs(name),
    )?;
    write(
        crate_dir.join("tests").join("smoke.rs"),
        service_smoke_test_rs(&crate_module),
    )?;
    Ok(())
}

fn ensure_worker_shape(crate_dir: &Path, name: &str) -> Result<()> {
    let crate_module = name.to_case(Case::Snake);
    create_dir_all(crate_dir.join("src").join("jobs"))?;
    create_dir_all(crate_dir.join("src").join("config"))?;
    create_dir_all(crate_dir.join("src").join("error"))?;
    create_dir_all(crate_dir.join("tests"))?;
    write(crate_dir.join("src").join("lib.rs"), worker_lib_rs(name))?;
    write(crate_dir.join("src").join("main.rs"), worker_main_rs(name))?;
    write(
        crate_dir.join("src").join("jobs").join("mod.rs"),
        worker_jobs_rs(),
    )?;
    write(
        crate_dir.join("src").join("error").join("mod.rs"),
        worker_error_rs(),
    )?;
    write(
        crate_dir.join("src").join("config").join("mod.rs"),
        worker_config_rs(name),
    )?;
    write(
        crate_dir.join("tests").join("smoke.rs"),
        worker_smoke_test_rs(&crate_module),
    )?;
    Ok(())
}

fn ensure_library_shape(crate_dir: &Path, name: &str) -> Result<()> {
    let crate_module = name.to_case(Case::Snake);
    create_dir_all(crate_dir.join("src").join("error"))?;
    create_dir_all(crate_dir.join("src").join("models"))?;
    create_dir_all(crate_dir.join("src").join("service"))?;
    create_dir_all(crate_dir.join("tests"))?;
    write(crate_dir.join("src").join("lib.rs"), library_lib_rs(name))?;
    write(
        crate_dir.join("src").join("error").join("mod.rs"),
        library_error_rs(),
    )?;
    write(
        crate_dir.join("src").join("models").join("mod.rs"),
        library_models_rs(),
    )?;
    write(
        crate_dir.join("src").join("service").join("mod.rs"),
        library_service_rs(),
    )?;
    write(
        crate_dir.join("tests").join("smoke.rs"),
        library_smoke_test_rs(&crate_module),
    )?;
    Ok(())
}

fn rewrite_routes_mod(routes_dir: &Path, dryrun: bool, stdout: &mut StandardStream) -> Result<()> {
    let modules = rust_route_modules(routes_dir)?;

    write_text_file(
        &routes_dir.join("mod.rs"),
        &routes_mod_rs(&modules),
        dryrun,
        stdout,
    )
}

fn rust_route_modules(routes_dir: &Path) -> Result<Vec<String>> {
    let mut modules = Vec::new();
    if routes_dir.exists() {
        for entry in std::fs::read_dir(routes_dir)
            .with_context(|| format!("Failed to read {}", routes_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let Some(module) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if module != "mod" {
                modules.push(module.to_string());
            }
        }
    }
    if !modules.iter().any(|module| module == "health") {
        modules.push("health".to_string());
    }

    Ok(modules)
}

fn replace_router_in_manifest_value(
    manifest: &mut Value,
    project_name: &str,
    existing_name: &str,
    new_name: &str,
) -> Result<()> {
    let project = project_table_mut(manifest, project_name)
        .with_context(|| format!("Could not find Rust project {project_name} in manifest"))?;
    let routers = project
        .entry("routers")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .context("Project routers must be an array")?;
    let mut replaced = false;
    for router in routers.iter_mut() {
        if router.as_str() == Some(existing_name) {
            *router = Value::String(new_name.to_string());
            replaced = true;
        }
    }
    if !replaced
        && !routers
            .iter()
            .any(|router| router.as_str() == Some(new_name))
    {
        routers.push(Value::String(new_name.to_string()));
    }
    Ok(())
}

fn remove_router_from_manifest_value(
    manifest: &mut Value,
    project_name: &str,
    router_name: &str,
) -> Result<()> {
    let project = project_table_mut(manifest, project_name)
        .with_context(|| format!("Could not find Rust project {project_name} in manifest"))?;
    let routers = project
        .entry("routers")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .context("Project routers must be an array")?;
    routers.retain(|router| router.as_str() != Some(router_name));
    Ok(())
}

fn read_manifest_value(path: &Path) -> Result<Value> {
    let content =
        read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str::<Value>(&content).with_context(|| format!("Failed to parse {}", path.display()))
}

fn write_manifest_value(path: &Path, value: &Value) -> Result<()> {
    write(path, toml::to_string_pretty(value)?)
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn write_rust_rendered_templates(
    templates: &[RenderedTemplate],
    dryrun: bool,
    stdout: &mut StandardStream,
) -> Result<()> {
    let mut writable_templates = Vec::new();
    for template in templates {
        if template.path.exists() && is_user_editable_rust_file(&template.path) {
            if dryrun {
                writeln!(stdout, "Would preserve {}", template.path.display())?;
            } else {
                writeln!(stdout, "Preserved {}", template.path.display())?;
            }
            continue;
        }
        writable_templates.push(template.clone());
    }

    write_rendered_templates(&writable_templates, dryrun, stdout)
}

fn is_user_editable_rust_file(path: &Path) -> bool {
    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        return false;
    }

    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();

    if components.iter().any(|component| *component == "tests") {
        return true;
    }

    let Some(src_index) = components.iter().position(|component| *component == "src") else {
        return false;
    };
    let Some(section) = components.get(src_index + 1).copied() else {
        return false;
    };

    match section {
        "lib.rs" | "main.rs" | "config" | "domain" | "persistence" | "error" | "jobs"
        | "models" | "service" => true,
        "routes" => true,
        _ => false,
    }
}

fn write_text_file(
    path: &Path,
    content: &str,
    dryrun: bool,
    stdout: &mut StandardStream,
) -> Result<()> {
    if dryrun {
        writeln!(stdout, "Would write {}", path.display())?;
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory for {}", path.display()))?;
    }
    write(path, content).with_context(|| format!("Failed to write {}", path.display()))
}

fn write_text_file_preserving_existing(
    path: &Path,
    content: &str,
    dryrun: bool,
    stdout: &mut StandardStream,
) -> Result<()> {
    if path.exists() && is_user_editable_rust_file(path) {
        if dryrun {
            writeln!(stdout, "Would preserve {}", path.display())?;
        } else {
            writeln!(stdout, "Preserved {}", path.display())?;
        }
        return Ok(());
    }

    write_text_file(path, content, dryrun, stdout)
}

fn escape_toml_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn escape_rust_string(value: &str) -> String {
    escape_toml_string(value)
}

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn cargo_members_support_compact_arrays_and_preserve_unrelated_metadata() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("Cargo.toml");
        write(&path, "# user comment\n[workspace]\nmembers = [\"crates/a\"]\n[workspace.metadata]\nexample = \"crates/a\"\n").unwrap();
        add_cargo_member(root.path(), "crates/a", false).unwrap();
        add_cargo_member(root.path(), "crates/b", false).unwrap();
        replace_cargo_member(root.path(), "crates/a", "crates/c").unwrap();
        remove_cargo_member(root.path(), "crates/b").unwrap();
        let source = read_to_string(path).unwrap();
        let cargo: Value = toml::from_str(&source).unwrap();
        assert_eq!(
            cargo["workspace"]["members"].as_array().unwrap(),
            &[Value::String("crates/c".to_owned())]
        );
        assert_eq!(
            cargo["workspace"]["metadata"]["example"].as_str(),
            Some("crates/a")
        );
        assert!(source.contains("# user comment"));
    }
}
