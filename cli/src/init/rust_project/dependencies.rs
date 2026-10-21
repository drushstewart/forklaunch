//! Explicit dependency source selection.
use super::*;

use crate::core::forklaunch_state;

const RELEASE_VERSION: &str = "0.1.3";
/// Blueprint crates released against `RELEASE_VERSION` of the framework.
const BLUEPRINT_RELEASE_VERSION: &str = "0.1.1";

/// The `[framework]` selection for a new application: published crates by
/// default, or an external framework-rs checkout referenced in place.
pub(crate) fn configuration(matches: &ArgMatches) -> Result<toml::Table> {
    let selected_version = matches
        .try_get_one::<String>("framework-version")
        .ok()
        .flatten();
    let path = matches
        .try_get_one::<String>("framework-path")
        .ok()
        .flatten();
    if std::env::var("FORKLAUNCH_FRAMEWORK_SNAPSHOT").is_ok_and(|value| value == "1") {
        eprintln!(
            "warning: FORKLAUNCH_FRAMEWORK_SNAPSHOT is no longer supported; framework crates come from the registry (use --framework-path for a local checkout)."
        );
    }
    let version = selected_version.map(String::as_str)
        .or_else(|| if path.is_none() { Some(RELEASE_VERSION) } else { None });
    let mut config = toml::Table::new();
    if let Some(version) = version {
        // Accept exact releases only; arbitrary TOML or broad ranges cannot leak
        // into generated dependencies or silently select a different release.
        anyhow::ensure!(
            valid_version(version),
            "--framework-version requires an exact numeric MAJOR.MINOR.PATCH version"
        );
        config.insert("version".into(), version.into());
    }
    // Module and worker crates come from the registry too; nothing is copied.
    // A local framework checkout uses its sibling blueprint-rs when present.
    config.insert("blueprint_version".into(), BLUEPRINT_RELEASE_VERSION.into());
    if let Some(path) = path {
        let path =
            std::fs::canonicalize(path).context("Framework override directory does not exist")?;
        anyhow::ensure!(
            path.join("crates/axum/Cargo.toml").is_file(),
            "--framework-path must identify the framework-rs workspace"
        );
        config.insert("path".into(), path.to_string_lossy().into_owned().into());
    }
    Ok(config)
}

/// The registry selection written when an application is migrated off a
/// vendored snapshot or generated without explicit options.
fn default_configuration() -> toml::Table {
    let mut config = toml::Table::new();
    config.insert("version".into(), RELEASE_VERSION.into());
    config.insert("blueprint_version".into(), BLUEPRINT_RELEASE_VERSION.into());
    config
}

/// Record the selection and rewrite already generated manifests to it.
pub(crate) fn configure(root: &Path, matches: &ArgMatches) -> Result<()> {
    let config = configuration(matches)?;
    forklaunch_state::write_framework(root, &config)?;
    rewrite_tree(root, root)?;
    remove_vendor_excludes(root)
}

/// Applications generated before `[framework]` existed used a vendored
/// snapshot. Switch them to the published crates when the snapshot is exactly
/// what the CLI vendored; keep a modified one and warn once.
pub(super) fn migrate_vendored_framework(root: &Path, dryrun: bool) -> Result<()> {
    if dryrun || configured(root) {
        return Ok(());
    }
    let Some(snapshot) = ["vendor/framework", ".forklaunch/framework-rs"]
        .into_iter()
        .map(|path| root.join(path))
        .find(|path| path.join("Cargo.toml").is_file())
    else {
        // Nothing vendored: a new application without an explicit selection.
        forklaunch_state::write_framework(root, &default_configuration())?;
        rewrite_tree(root, root)?;
        return remove_vendor_excludes(root);
    };
    if !super::vendored_framework_unmodified(&snapshot)? {
        eprintln!(
            "warning: {} differs from the framework sources ForkLaunch vendored, so this application keeps using it. Move your changes upstream, delete it, and rerun to use the published framework crates.",
            snapshot.display()
        );
        return Ok(());
    }
    forklaunch_state::write_framework(root, &default_configuration())?;
    rewrite_tree(root, root)?;
    remove_dir_all(&snapshot)?;
    let vendor = root.join("vendor");
    if std::fs::read_dir(&vendor).is_ok_and(|mut entries| entries.next().is_none()) {
        std::fs::remove_dir(vendor)?;
    }
    let dockerfile = root.join("infra/Dockerfile");
    if dockerfile.is_file() {
        let source = read_to_string(&dockerfile)?;
        write(dockerfile, source.replace("find src vendor/framework", "find src"))?;
    }
    eprintln!("Switched {} from its vendored framework snapshot to the published framework crates.", root.display());
    remove_vendor_excludes(root)
}

/// Workspace excludes for vendored copies are obsolete once they are gone.
pub(super) fn remove_vendor_excludes(root: &Path) -> Result<()> {
    let path = root.join("Cargo.toml");
    let Ok(source) = read_to_string(&path) else { return Ok(()) };
    let mut cargo: toml_edit::DocumentMut = source.parse()?;
    let Some(workspace) = cargo.get_mut("workspace").and_then(toml_edit::Item::as_table_like_mut) else {
        return Ok(());
    };
    if let Some(excludes) = workspace.get_mut("exclude").and_then(toml_edit::Item::as_array_mut) {
        excludes.retain(|value| {
            !value.as_str().is_some_and(|entry| {
                matches!(entry, "vendor/framework" | ".forklaunch/framework-rs" | ".forklaunch/blueprint-rs")
                    && !root.join(entry).exists()
            })
        });
        if excludes.is_empty() {
            workspace.remove("exclude");
        }
    }
    let updated = cargo.to_string();
    if updated != source {
        write(path, updated)?;
    }
    Ok(())
}

/// Host-local dependencies need an explicitly application-owned container integration.
pub(super) fn preflight_mixed_image(root: &Path) -> Result<()> {
    let Some(config) = forklaunch_state::framework(root) else { return Ok(()); };
    if config.get("path").and_then(Value::as_str).is_none() { return Ok(()); }
    let path = root.join("Dockerfile.rust");
    let template = render_static_rust_template("rust/application/Dockerfile");
    let marked = crate::core::watermark::apply_watermark(&RenderedTemplate { path: path.clone(), content: template.clone(), context: None })?;
    let custom = read_to_string(&path).is_ok_and(|source| source.trim_end() != template.trim_end() && source.trim_end() != marked.trim_end());
    anyhow::ensure!(custom && forklaunch_state::file_stamped(&path).is_none(),
        "Local --framework-path dependencies are a host-development override outside the generated Docker build context. Before adding Rust to this workspace, provide an application-owned Dockerfile.rust integrating that local source, or use the published default for the managed image. No project files were generated.");
    Ok(())
}

/// Registry and path modes need no local sources; earlier parser patches are removed.
pub(super) fn prepare(root: &Path, dryrun: bool) -> Result<()> {
    if dryrun { return Ok(()); }
    remove_literal_json_patch(root)?;
    rewrite_tree(root, root)?;
    remove_vendor_excludes(root)
}

fn valid_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

pub(super) fn rewrite_tree(root: &Path, directory: &Path) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if ["vendor", "target", "node_modules", ".git"]
                .iter()
                .any(|name| path.file_name().is_some_and(|file| file == *name))
            {
                continue;
            }
            if path == root.join(".forklaunch/framework-rs") {
                continue;
            }
            rewrite_tree(root, &path)?;
        } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            let source = read_to_string(&path)?;
            if let Some(updated) = rewrite_in_place(root, &source)? {
                write(&path, updated)?;
            }
        }
    }
    Ok(())
}

pub(super) fn configured(root: &Path) -> bool {
    forklaunch_state::has_framework(root)
}

pub(crate) fn render_for_path(path: &Path, source: &str) -> Result<String> {
    if path.file_name().is_none_or(|name| name != "Cargo.toml") {
        return Ok(source.into());
    }
    let root = path.ancestors().find(|root| configured(root));
    match root {
        Some(root) => render(root, source),
        None => Ok(source.into()),
    }
}

fn blueprint_suffix(path: &str) -> Option<&str> {
    [".forklaunch/blueprint-rs/crates/", "blueprint-rs/crates/"]
        .into_iter()
        .find_map(|prefix| path.split_once(prefix).map(|(_, suffix)| suffix))
}

/// Whether blueprint path dependencies resolve outside the application, so no
/// `.forklaunch/blueprint-rs` copy is needed: published crates, or the
/// blueprint workspace beside a host framework checkout.
pub(super) fn external_blueprint(root: &Path) -> bool {
    let Some(config) = forklaunch_state::framework(root) else { return false };
    config.get("blueprint_version").and_then(Value::as_str).is_some()
        || config.get("path").and_then(Value::as_str).is_some_and(|path| local_blueprint(path).is_dir())
}

fn local_blueprint(framework: &str) -> PathBuf {
    Path::new(framework).parent().unwrap_or(Path::new(framework)).join("blueprint-rs/crates")
}

fn framework_suffix(path: &str) -> Option<&str> {
    [
        "vendor/framework/",
        ".forklaunch/framework-rs/",
        "framework-rs/",
    ]
    .into_iter()
    .find_map(|prefix| path.split_once(prefix).map(|(_, suffix)| suffix))
}

fn render(root: &Path, source: &str) -> Result<String> {
    let config = forklaunch_state::framework(root).context("Framework dependency selection missing")?;
    let mut document: Value = toml::from_str(source)?;
    rewrite_value(&mut document, &config);
    Ok(toml::to_string_pretty(&document)?)
}

/// Rewrite an existing manifest's framework dependencies, keeping its comments
/// and formatting. `None` when nothing refers to framework sources.
fn rewrite_in_place(root: &Path, source: &str) -> Result<Option<String>> {
    let config = forklaunch_state::framework(root).context("Framework dependency selection missing")?;
    // An unparseable manifest is the application's to fix; Cargo reports it.
    let Ok(mut document) = source.parse::<toml_edit::DocumentMut>() else { return Ok(None) };
    let mut changed = false;
    rewrite_item(document.as_item_mut(), &config, &mut changed);
    Ok(changed.then(|| document.to_string()))
}

fn rewrite_item(item: &mut toml_edit::Item, config: &Value, changed: &mut bool) {
    let Some(table) = item.as_table_like_mut() else { return };
    if let Some(path) = table.get("path").and_then(toml_edit::Item::as_str).map(str::to_owned) {
        let mut parsed = toml::Table::new();
        parsed.insert("path".into(), path.clone().into());
        let mut value = Value::Table(parsed);
        rewrite_value(&mut value, config);
        match (value.get("path").and_then(Value::as_str), value.get("version").and_then(Value::as_str)) {
            (Some(next), _) if next != path => {
                table.insert("path", toml_edit::value(next));
                *changed = true;
            }
            (None, Some(version)) => {
                table.remove("path");
                table.insert("version", toml_edit::value(version));
                *changed = true;
            }
            _ => {}
        }
    }
    for (_, child) in table.iter_mut() {
        rewrite_item(child, config, changed);
    }
}

fn rewrite_value(value: &mut Value, config: &Value) {
    if let Some(table) = value.as_table_mut() {
        // All framework path dependencies are explicit inline/table specs.
        // Features, aliases, optional flags and default-features are preserved.
        if let Some(path) = table.get("path").and_then(Value::as_str).map(str::to_owned) {
            if let Some(suffix) = blueprint_suffix(&path) {
                let local = config.get("path").and_then(Value::as_str).map(local_blueprint).filter(|path| path.is_dir());
                if let Some(local) = local {
                    table.insert("path".into(), local.join(suffix).to_string_lossy().into_owned().into());
                } else if let Some(version) = config.get("blueprint_version").and_then(Value::as_str) {
                    table.remove("path");
                    table.insert("version".into(), format!("={version}").into());
                }
            } else if let Some(suffix) = framework_suffix(&path) {
                if let Some(local) = config.get("path").and_then(Value::as_str) {
                    table.insert(
                        "path".into(),
                        Path::new(local).join(suffix).to_string_lossy().into_owned().into(),
                    );
                } else if let Some(version) = config.get("version").and_then(Value::as_str) {
                    table.remove("path");
                    table.insert("version".into(), format!("={version}").into());
                }
            }
        }
        for (_, value) in table.iter_mut() {
            rewrite_value(value, config);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_mode_preserves_features_and_aliases() {
        let root = tempfile::tempdir().unwrap();
        create_dir_all(root.path().join(".forklaunch")).unwrap();
        write(root.path().join(".forklaunch/manifest.toml"), "app_name = 'demo'\n\n[framework]\nversion = '0.1.0'\n").unwrap();
        let source = "[dependencies]\nframework = { package='forklaunch-axum',path='../../vendor/framework/crates/axum', default-features=false,features=['native'] }\nmodels = { path='../models' }\n";
        let output: Value = toml::from_str(&render(root.path(), source).unwrap()).unwrap();
        assert_eq!(
            output["dependencies"]["framework"]["version"].as_str(),
            Some("=0.1.0")
        );
        assert_eq!(
            output["dependencies"]["framework"]["features"][0].as_str(),
            Some("native")
        );
        assert_eq!(
            output["dependencies"]["framework"]["default-features"].as_bool(),
            Some(false)
        );
        assert_eq!(
            output["dependencies"]["models"]["path"].as_str(),
            Some("../models")
        );
        assert!(output.get("patch").is_none());
        assert!(output["dependencies"]["framework"].get("path").is_none());
    }
    #[test]
    fn existing_manifests_keep_comments_and_untouched_files_are_not_rewritten() {
        let root = tempfile::tempdir().unwrap();
        create_dir_all(root.path().join(".forklaunch")).unwrap();
        write(root.path().join(".forklaunch/manifest.toml"), "[framework]\nversion = '0.1.2'\nblueprint_version = '0.1.0'\n").unwrap();
        let source = "# owned\n[dependencies]\n# framework\ncore = { package = 'forklaunch-core', path = '../../vendor/framework/crates/core', features = ['build'] }\nworker = { path = '../../.forklaunch/blueprint-rs/crates/interfaces/worker-interface' }\nlocal = { path = '../local' }\n";
        let updated = rewrite_in_place(root.path(), source).unwrap().unwrap();
        assert!(updated.starts_with("# owned\n") && updated.contains("# framework\n"), "{updated}");
        let value: Value = toml::from_str(&updated).unwrap();
        assert_eq!(value["dependencies"]["core"]["version"].as_str(), Some("=0.1.2"));
        assert_eq!(value["dependencies"]["core"]["features"][0].as_str(), Some("build"));
        assert_eq!(value["dependencies"]["worker"]["version"].as_str(), Some("=0.1.0"));
        assert_eq!(value["dependencies"]["local"]["path"].as_str(), Some("../local"));
        assert!(rewrite_in_place(root.path(), "# mine\n[dependencies]\nlocal = { path = '../local' }\n").unwrap().is_none());
    }

    #[test]
    fn vendored_snapshot_migrates_only_when_unmodified() {
        let root = tempfile::tempdir().unwrap();
        create_dir_all(root.path().join(".forklaunch")).unwrap();
        write(root.path().join(".forklaunch/manifest.toml"), "app_name = 'demo'\n").unwrap();
        write(root.path().join("Cargo.toml"), "[workspace]\nmembers = ['api']\nexclude = ['vendor/framework', 'other']\n").unwrap();
        create_dir_all(root.path().join("api")).unwrap();
        write(root.path().join("api/Cargo.toml"), "[package]\nname = 'api'\n[dependencies]\ncore = { path = '../vendor/framework/crates/core' }\n").unwrap();
        // A snapshot that differs from the embedded sources is kept.
        create_dir_all(root.path().join("vendor/framework/crates/core")).unwrap();
        write(root.path().join("vendor/framework/Cargo.toml"), "[workspace]\n").unwrap();
        write(root.path().join("vendor/framework/crates/core/lib.rs"), "// edited\n").unwrap();
        migrate_vendored_framework(root.path(), false).unwrap();
        assert!(!configured(root.path()));
        assert!(root.path().join("vendor/framework/crates/core/lib.rs").is_file());
        // An exact copy is replaced by the published crates.
        remove_dir_all(root.path().join("vendor/framework/crates")).unwrap();
        super::super::write_embedded_sources(&super::super::FRAMEWORK_CRATES, &root.path().join("vendor/framework/crates")).unwrap();
        assert!(super::super::vendored_framework_unmodified(&root.path().join("vendor/framework")).unwrap());
        migrate_vendored_framework(root.path(), false).unwrap();
        assert!(configured(root.path()));
        assert!(!root.path().join("vendor").exists());
        let manifest: Value = toml::from_str(&read_to_string(root.path().join(".forklaunch/manifest.toml")).unwrap()).unwrap();
        assert_eq!(manifest["framework"]["version"].as_str(), Some(RELEASE_VERSION));
        let api: Value = toml::from_str(&read_to_string(root.path().join("api/Cargo.toml")).unwrap()).unwrap();
        assert_eq!(api["dependencies"]["core"]["version"].as_str(), Some(format!("={RELEASE_VERSION}").as_str()));
        let cargo: Value = toml::from_str(&read_to_string(root.path().join("Cargo.toml")).unwrap()).unwrap();
        assert_eq!(cargo["workspace"]["exclude"].as_array().unwrap().len(), 1, "unrelated excludes stay");
    }

    #[test]
    fn local_override_is_explicit_and_exact_versions_are_required() {
        assert!(valid_version("0.1.0"));
        for invalid in ["*", "^0.1", "0.1", "01.2.3", "0.1.0\n"] {
            assert!(!valid_version(invalid));
        }
        let mut value: Value = toml::from_str("[dependencies]\nworker = {package='forklaunch-worker-interface',path='../../.forklaunch/blueprint-rs/crates/interfaces/worker-interface',features=['apalis']}\n").unwrap();
        let config: Value = toml::from_str("path='/tmp/framework-rs'").unwrap();
        rewrite_value(&mut value, &config);
        assert_eq!(
            value["dependencies"]["worker"]["path"].as_str(),
            Some("../../.forklaunch/blueprint-rs/crates/interfaces/worker-interface")
        );
        let mut framework: Value = toml::from_str("[dependencies]\ncore = {package='forklaunch-core',path='../../vendor/framework/crates/core',features=['typed-schema']}\n").unwrap();
        rewrite_value(&mut framework, &config);
        assert_eq!(framework["dependencies"]["core"]["path"].as_str(), Some("/tmp/framework-rs/crates/core"));
    }

    fn workspace_toml(relative: &str) -> toml::Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        toml::from_str(&read_to_string(&path).unwrap()).unwrap()
    }

    fn member_manifests(workspace: &str) -> Vec<toml::Value> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(workspace);
        let members = workspace_toml(&format!("{workspace}/Cargo.toml"))["workspace"]["members"]
            .as_array()
            .unwrap()
            .clone();
        let mut manifests = Vec::new();
        for member in members {
            let pattern = root.join(member.as_str().unwrap()).join("Cargo.toml");
            for path in glob::glob(pattern.to_str().unwrap()).unwrap() {
                manifests.push(toml::from_str(&read_to_string(path.unwrap()).unwrap()).unwrap());
            }
        }
        manifests
    }

    /// Generated applications default to the release this checkout builds, and
    /// the blueprint pins exactly that framework release.
    #[test]
    fn default_versions_follow_the_framework_and_blueprint_workspaces() {
        let framework = workspace_toml("../framework-rs/Cargo.toml");
        assert_eq!(framework["workspace"]["package"]["version"].as_str(), Some(RELEASE_VERSION));
        let blueprint = workspace_toml("../blueprint-rs/Cargo.toml");
        assert_eq!(blueprint["workspace"]["package"]["version"].as_str(), Some(BLUEPRINT_RELEASE_VERSION));
        let pinned = format!("={RELEASE_VERSION}");
        for manifest in member_manifests("../blueprint-rs") {
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                for (name, dependency) in manifest.get(section).and_then(toml::Value::as_table).into_iter().flatten() {
                    let package = dependency.get("package").and_then(toml::Value::as_str).unwrap_or(name);
                    let path = dependency.get("path").and_then(toml::Value::as_str).unwrap_or_default();
                    if path.contains("framework-rs/") {
                        assert_eq!(dependency["version"].as_str(), Some(pinned.as_str()), "{package}");
                    }
                }
            }
        }
    }

    /// Third-party crates the Rust templates name directly must use the same
    /// requirement as the framework, so generated code and framework types unify.
    #[test]
    fn template_dependencies_match_the_framework_requirements() {
        let mut framework: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> = Default::default();
        for manifest in member_manifests("../framework-rs") {
            for section in ["dependencies", "dev-dependencies"] {
                for (name, dependency) in manifest.get(section).and_then(toml::Value::as_table).into_iter().flatten() {
                    let requirement = dependency.as_str().or_else(|| dependency.get("version")?.as_str());
                    if let Some(requirement) = requirement && !name.starts_with("forklaunch-") {
                        framework.entry(name.clone()).or_default().insert(requirement.trim_start_matches('=').to_owned());
                    }
                }
            }
        }
        let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/templates/rust");
        let pattern = templates.join("*/Cargo.toml.template");
        let mut checked = 0;
        for path in glob::glob(pattern.to_str().unwrap()).unwrap() {
            let path = path.unwrap();
            for line in read_to_string(&path).unwrap().lines() {
                let Some((name, rest)) = line.split_once('=') else { continue };
                let name = name.trim();
                let Some(versions) = framework.get(name).filter(|versions| versions.len() == 1) else { continue };
                let requirement = rest
                    .split('"')
                    .nth(1)
                    .filter(|_| rest.trim_start().starts_with('"') || rest.contains("version"))
                    .unwrap_or_default();
                let expected = versions.iter().next().unwrap();
                // Compare the declared compatibility range (major, or major.minor below 1.0).
                let range = |version: &str| {
                    let parts: Vec<&str> = version.split('.').collect();
                    if parts[0] == "0" { parts.iter().take(2).copied().collect::<Vec<_>>().join(".") } else { parts[0].to_owned() }
                };
                assert_eq!(range(requirement), range(expected), "{} in {}", name, path.display());
                checked += 1;
            }
        }
        assert!(checked > 0, "No template dependencies were compared");
    }
}
