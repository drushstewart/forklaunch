//! Add the native workspace to an existing Node/Bun application without replacing
//! its TypeScript runtime, tooling, package workspace or Docker build.
use toml_edit::{Array, DocumentMut, Item, Table, value};

use super::*;
use crate::core::application_output::output_path;

fn table<'a>(parent: &'a mut Table, name: &str) -> Result<&'a mut Table> {
    parent
        .entry(name)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .with_context(|| format!("Cargo {name} must be a table"))
}

pub(super) fn dockerfile(root: &Path) -> Result<&'static str> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    Ok(
        if matches!(
            manifest.get("runtime").and_then(Value::as_str),
            Some("node" | "bun")
        ) {
            "./Dockerfile.rust"
        } else {
            "./Dockerfile"
        },
    )
}

pub(super) fn ensure_workspace(root: &Path, dryrun: bool) -> Result<()> {
    let manifest_path = output_path(root, Path::new(".forklaunch/manifest.toml"))?;
    let mut manifest = read_manifest_value(&manifest_path)?;
    if !matches!(
        manifest.get("runtime").and_then(Value::as_str),
        Some("node" | "bun")
    ) {
        return Ok(());
    }
    // Project manifest views accept both spellings; emit only one canonical key.
    let application = manifest
        .as_table_mut()
        .context("Application manifest must be a table")?;
    for (canonical, alias) in [
        ("applicationLanguage", "application_language"),
        ("cargoWorkspace", "cargo_workspace"),
        ("sdkTargets", "sdk_targets"),
    ] {
        if let Some(aliased) = application.remove(alias) {
            if let Some(existing) = application.get(canonical) {
                if existing != &aliased {
                    bail!("Conflicting application metadata: {canonical} and {alias}");
                }
            } else {
                application.insert(canonical.into(), aliased);
            }
        }
    }
    if manifest
        .get("cargoWorkspace")
        .and_then(Value::as_str)
        .is_some_and(|path| path != "Cargo.toml")
    {
        bail!("Native generation requires cargoWorkspace = \"Cargo.toml\" at the application root");
    }
    let cargo_path = output_path(root, Path::new("Cargo.toml"))?;
    let mut cargo: DocumentMut = if cargo_path.exists() {
        read_to_string(&cargo_path)?
            .parse()
            .context("Failed to parse existing Cargo workspace")?
    } else {
        DocumentMut::new()
    };
    let workspace = table(cargo.as_table_mut(), "workspace")?;
    workspace.entry("resolver").or_insert(value("3"));
    if !workspace
        .entry("members")
        .or_insert(value(Array::new()))
        .is_array()
    {
        bail!("Cargo workspace.members must be an array");
    }
    let exclude = workspace
        .entry("exclude")
        .or_insert(value(Array::new()))
        .as_array_mut()
        .context("Cargo workspace.exclude must be an array")?;
    for path in [".forklaunch/framework-rs", ".forklaunch/blueprint-rs"] {
        if !exclude.iter().any(|entry| entry.as_str() == Some(path)) {
            exclude.push(path);
        }
    }
    let package = table(workspace, "package")?;
    package.entry("version").or_insert(value("0.1.0"));
    let edition = package.entry("edition").or_insert(value("2024"));
    if edition.as_str() != Some("2024") {
        bail!("Generated native crates require Cargo workspace.package.edition = \"2024\"");
    }
    package.entry("license").or_insert(value(
        manifest
            .get("license")
            .and_then(Value::as_str)
            .unwrap_or("MIT"),
    ));
    let mut authors = Array::new();
    if let Some(author) = manifest
        .get("author")
        .and_then(Value::as_str)
        .filter(|author| !author.is_empty())
    {
        authors.push(author);
    }
    package.entry("authors").or_insert(value(authors));
    let mut outputs = Vec::new();
    for (destination, template) in [
        ("Dockerfile.rust", "rust/application/Dockerfile"),
        (
            "Dockerfile.rust.dockerignore",
            "rust/application/dockerignore",
        ),
        ("rustfmt.toml", "rust/application/rustfmt.toml"),
        ("clippy.toml", "rust/application/clippy.toml"),
    ] {
        let destination = output_path(root, Path::new(destination))?;
        if !destination.exists() {
            outputs.push((destination, render_static_rust_template(template)));
        } else if !destination.is_file() {
            bail!(
                "Native workspace output must be a file: {}",
                destination.display()
            );
        }
    }
    let application = manifest
        .as_table_mut()
        .context("Application manifest must be a table")?;
    application.insert("language".into(), Value::String("mixed".into()));
    application.insert("applicationLanguage".into(), Value::String("mixed".into()));
    application
        .entry("cargoWorkspace")
        .or_insert(Value::String("Cargo.toml".into()));
    application.entry("sdkTargets").or_insert(Value::Array(
        default_sdk_targets()
            .into_iter()
            .map(Value::String)
            .collect(),
    ));
    if dryrun {
        return Ok(());
    }
    write(cargo_path, cargo.to_string())?;
    for (destination, source) in outputs {
        write(destination, source)?;
    }
    write_manifest_value(&manifest_path, &manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        create_dir_all(root.path().join(".forklaunch")).unwrap();
        write(root.path().join(".forklaunch/manifest.toml"), "runtime = \"node\"\nmodules_path = \"modules\"\nauthor = \"Example\"\nlicense = \"MIT\"\n").unwrap();
        root
    }

    #[test]
    fn adds_native_workspace_without_changing_typescript_tooling() {
        let root = fixture();
        write(root.path().join("Dockerfile"), "FROM node:22\n").unwrap();
        let original = read_to_string(root.path().join(".forklaunch/manifest.toml")).unwrap();
        ensure_workspace(root.path(), true).unwrap();
        assert!(!root.path().join("Cargo.toml").exists());
        assert!(!root.path().join("Dockerfile.rust").exists());
        assert_eq!(
            read_to_string(root.path().join(".forklaunch/manifest.toml")).unwrap(),
            original
        );
        ensure_workspace(root.path(), false).unwrap();
        assert_eq!(
            read_to_string(root.path().join("Dockerfile")).unwrap(),
            "FROM node:22\n"
        );
        let manifest = read_manifest_value(&root.path().join(".forklaunch/manifest.toml")).unwrap();
        assert_eq!(manifest["runtime"].as_str(), Some("node"));
        assert_eq!(manifest["modules_path"].as_str(), Some("modules"));
        assert_eq!(manifest["language"].as_str(), Some("mixed"));
        let cargo: Value =
            toml::from_str(&read_to_string(root.path().join("Cargo.toml")).unwrap()).unwrap();
        assert_eq!(
            cargo["workspace"]["package"]["authors"][0].as_str(),
            Some("Example")
        );
        let source = read_to_string(root.path().join("Cargo.toml")).unwrap();
        ensure_workspace(root.path(), false).unwrap();
        assert_eq!(
            read_to_string(root.path().join("Cargo.toml")).unwrap(),
            source
        );
        assert_eq!(dockerfile(root.path()).unwrap(), "./Dockerfile.rust");
    }

    #[test]
    fn preserves_existing_cargo_members_comments_and_custom_native_files() {
        let root = fixture();
        write(root.path().join("Cargo.toml"), "# user-owned workspace\n[workspace]\nmembers = [\"tools\"]\n[workspace.package]\nversion = \"2.0.0\"\nedition = \"2024\"\n[workspace.metadata]\nowner = \"custom\"\n").unwrap();
        write(
            root.path().join("Dockerfile.rust"),
            "# custom native image\n",
        )
        .unwrap();
        ensure_workspace(root.path(), false).unwrap();
        let source = read_to_string(root.path().join("Cargo.toml")).unwrap();
        assert!(source.starts_with("# user-owned workspace"));
        let cargo: Value = toml::from_str(&source).unwrap();
        assert_eq!(cargo["workspace"]["members"][0].as_str(), Some("tools"));
        assert_eq!(
            cargo["workspace"]["package"]["version"].as_str(),
            Some("2.0.0")
        );
        assert_eq!(
            cargo["workspace"]["metadata"]["owner"].as_str(),
            Some("custom")
        );
        assert_eq!(
            read_to_string(root.path().join("Dockerfile.rust")).unwrap(),
            "# custom native image\n"
        );
    }

    #[test]
    fn rejects_incompatible_cargo_before_mutating_application() {
        let root = fixture();
        let original = read_to_string(root.path().join(".forklaunch/manifest.toml")).unwrap();
        let source = "[workspace]\nmembers = []\n[workspace.package]\nedition = \"2021\"\n";
        write(root.path().join("Cargo.toml"), source).unwrap();
        assert!(
            ensure_workspace(root.path(), false)
                .unwrap_err()
                .to_string()
                .contains("edition")
        );
        assert_eq!(
            read_to_string(root.path().join("Cargo.toml")).unwrap(),
            source
        );
        assert_eq!(
            read_to_string(root.path().join(".forklaunch/manifest.toml")).unwrap(),
            original
        );
        assert!(!root.path().join("Dockerfile.rust").exists());
    }
    #[test]
    fn preserves_aliased_sdk_settings_without_duplicate_manifest_fields() {
        let root = fixture();
        let path = root.path().join(".forklaunch/manifest.toml");
        let original = read_to_string(&path).unwrap();
        write(&path, format!("{original}sdk_targets = [\"rust\"]\ncargo_workspace = \"Cargo.toml\"\napplication_language = \"typescript\"\n")).unwrap();
        ensure_workspace(root.path(), false).unwrap();
        let manifest = read_manifest_value(&path).unwrap();
        assert_eq!(
            manifest["sdkTargets"],
            Value::Array(vec![Value::String("rust".into())])
        );
        assert_eq!(manifest["applicationLanguage"].as_str(), Some("mixed"));
        assert!(manifest.get("sdk_targets").is_none());
        assert!(manifest.get("cargo_workspace").is_none());
        assert!(manifest.get("application_language").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_escaping_output_before_creating_a_native_workspace() {
        let root = fixture();
        let external = tempfile::tempdir().unwrap();
        let path = root.path().join(".forklaunch/manifest.toml");
        let original = read_to_string(&path).unwrap();
        write(external.path().join("Dockerfile"), "external file").unwrap();
        std::os::unix::fs::symlink(
            external.path().join("Dockerfile"),
            root.path().join("Dockerfile.rust"),
        )
        .unwrap();
        assert!(
            ensure_workspace(root.path(), false)
                .unwrap_err()
                .to_string()
                .contains("inside")
        );
        assert!(!root.path().join("Cargo.toml").exists());
        assert_eq!(read_to_string(&path).unwrap(), original);
        assert_eq!(
            read_to_string(external.path().join("Dockerfile")).unwrap(),
            "external file"
        );
    }
}
