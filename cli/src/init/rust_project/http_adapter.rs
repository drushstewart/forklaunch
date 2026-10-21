//! Module HTTP layers are application source, as TypeScript modules copy their
//! controllers and routes. The blueprint keeps each `*-axum` crate as the tested,
//! unpublished origin; generation copies it under `api/http` and depends on the
//! published implementation crates it uses.
use super::*;

/// The `*-axum` crates' manifests and sources, one directory per module.
static MODULE_HTTP: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/templates/rust/module-http");

/// The module's embedded HTTP layer, its blueprint crate path (relative to
/// `blueprint-rs/crates`), and its crate name.
fn adapter(kind: modules::BuiltinModule) -> (&'static str, &'static str, &'static str) {
    match kind.template() {
        "cac" => ("cac", "implementations/cac/cac-axum", "forklaunch-cac-axum"),
        "ecommerce" => ("ecommerce", "implementations/ecommerce/ecommerce-axum", "forklaunch-ecommerce-axum"),
        _ => ("billing", "implementations/billing/billing-axum", "forklaunch-billing-axum"),
    }
}

const MODULE_PATH: &str = "crate::api::http::";

/// Re-express a dependency path of the embedded crate in the module template's
/// convention, which later framework/blueprint path rewriting understands.
fn rebase(source: &str, path: &str) -> Option<String> {
    let virtual_root = Path::new("/workspace");
    let target = normalize(&virtual_root.join("blueprint-rs/crates").join(source).join(path));
    for (workspace, generated) in [("blueprint-rs/crates", ".forklaunch/blueprint-rs/crates"), ("framework-rs/crates", ".forklaunch/framework-rs/crates")] {
        if let Ok(rest) = target.strip_prefix(virtual_root.join(workspace)) {
            return Some(format!("../../{generated}/{}", rest.to_string_lossy().replace('\\', "/")));
        }
    }
    None
}

/// Resolve `..` without touching the file system; dependencies may not exist yet.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    normalized
}

fn merge_tables(into: &mut toml_edit::Table, from: &toml_edit::Table, source: &str) -> Result<()> {
    for (name, item) in from.iter() {
        let mut item = item.clone();
        if let Some(spec) = item.as_table_like_mut() {
            if let Some(path) = spec.get("path").and_then(|path| path.as_str()).map(str::to_owned) {
                let rebased = rebase(source, &path).with_context(|| format!("Unsupported adapter dependency path: {path}"))?;
                spec.insert("path", toml_edit::value(rebased));
            }
        }
        match into.get_mut(name).and_then(|existing| existing.as_table_like_mut()) {
            Some(existing) => {
                // Keep the module's own spec; add features the adapter needs.
                let wanted: Vec<String> = item.as_table_like().and_then(|spec| spec.get("features"))
                    .and_then(|features| features.as_array()).into_iter().flatten()
                    .filter_map(|feature| feature.as_str().map(str::to_owned)).collect();
                if !wanted.is_empty() {
                    if existing.get("features").is_none() {
                        existing.insert("features", toml_edit::value(toml_edit::Array::new()));
                    }
                    let features = existing.get_mut("features").and_then(|features| features.as_array_mut())
                        .context("Dependency features must be an array")?;
                    for feature in wanted {
                        if !features.iter().any(|value| value.as_str() == Some(&feature)) {
                            features.push(feature);
                        }
                    }
                }
            }
            None => {
                into.insert(name, item);
            }
        }
    }
    Ok(())
}

/// Copy the module's HTTP layer into `api/http` and fold its manifest into the
/// module's own: dependencies, optional features, and the features the module
/// template selected become the module's defaults.
pub(super) fn embed(dir: &Path, kind: modules::BuiltinModule, templates: &mut Vec<RenderedTemplate>) -> Result<()> {
    let (module, source, crate_name) = adapter(kind);
    let embedded = MODULE_HTTP.get_dir(module)
        .with_context(|| format!("Embedded module HTTP layer {module} is missing"))?;
    let namespace = crate_name.replace('-', "_");
    let manifest: toml_edit::DocumentMut = embedded
        .get_file(Path::new(module).join("Cargo.toml.template"))
        .and_then(|file| file.contents_utf8())
        .context("Module HTTP layer manifest is missing")?
        .parse()?;
    let cargo_file = templates.iter_mut().find(|file| file.path == dir.join("Cargo.toml"))
        .context("Missing module manifest")?;
    let mut cargo: toml_edit::DocumentMut = cargo_file.content.parse()?;
    let selected: Vec<String> = cargo.get("dependencies").and_then(|deps| deps.get(crate_name))
        .and_then(|spec| spec.get("features")).and_then(|features| features.as_array()).into_iter().flatten()
        .filter_map(|feature| feature.as_str().map(str::to_owned)).collect();
    for scope in ["dependencies", "build-dependencies"] {
        if let Some(table) = cargo.get_mut(scope).and_then(|table| table.as_table_like_mut()) {
            table.remove(crate_name);
        }
    }
    for scope in ["dependencies", "dev-dependencies"] {
        if let Some(from) = manifest.get(scope).and_then(|table| table.as_table()) {
            if cargo.get(scope).is_none() {
                cargo[scope] = toml_edit::table();
            }
            let into = cargo[scope].as_table_mut().context("Expected dependency table")?;
            merge_tables(into, from, source)?;
        }
    }
    if let Some(features) = manifest.get("features").and_then(|table| table.as_table()) {
        if cargo.get("features").is_none() {
            cargo["features"] = toml_edit::table();
        }
        let into = cargo["features"].as_table_mut().context("Expected features table")?;
        for (name, item) in features.iter().filter(|(name, _)| *name != "default") {
            into.entry(name).or_insert(item.clone());
        }
        let mut defaults = toml_edit::Array::new();
        for feature in &selected {
            defaults.push(feature.as_str());
        }
        into.insert("default", toml_edit::value(defaults));
    }
    cargo_file.content = cargo.to_string();
    // Module entry points referred to the adapter crate by name.
    for file in templates.iter_mut() {
        if file.path.extension().is_some_and(|extension| extension == "rs") {
            file.content = file.content.replace(&format!("{namespace}::"), MODULE_PATH);
        }
    }
    if let Some(api) = templates.iter_mut().find(|file| file.path == dir.join("api/mod.rs")) {
        if !api.content.contains("pub mod http;") {
            api.content.push_str("pub mod http;\n");
        }
    }
    copy_sources(embedded, &Path::new(module).join("src"), &dir.join("api/http"), templates);
    Ok(())
}

fn copy_sources(directory: &Dir<'_>, root: &Path, destination: &Path, templates: &mut Vec<RenderedTemplate>) {
    for file in directory.files() {
        let Ok(relative) = file.path().strip_prefix(root) else { continue };
        let relative = if relative == Path::new("lib.rs") { PathBuf::from("mod.rs") } else { relative.to_path_buf() };
        let content = match file.contents_utf8() {
            Some(text) if file.path().extension().is_some_and(|extension| extension == "rs") => {
                // A crate root's inner lint attributes and docs are valid module headers.
                text.replace("crate::", MODULE_PATH)
            }
            Some(text) => text.to_owned(),
            None => continue,
        };
        templates.push(RenderedTemplate { path: destination.join(relative), content, context: None });
    }
    for child in directory.dirs() {
        if child.path().starts_with(root) || root.starts_with(child.path()) {
            copy_sources(child, root, destination, templates);
        }
    }
}
