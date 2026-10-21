//! Make cargo aware that the generated templates are an input.
//!
//! `src/templates` is baked into the binary with `include_dir!`, and several of
//! its module directories are symlinks into `blueprint/` so a module's template
//! and its reference implementation cannot drift. Cargo does not follow those
//! symlinks when deciding whether a rebuild is needed, so editing a blueprint
//! file leaves `cargo build` reporting success while the binary still carries
//! the previous copy of that module.
//!
//! The failure is quiet and expensive: `forklaunch init` keeps scaffolding the
//! old module, the generated project compiles cleanly, and nothing anywhere
//! says the template is stale. It cost a full verification round — a scaffold
//! was built, run, and paid through before the missing changes were noticed.
//!
//! Walking the tree and emitting `rerun-if-changed` for every file, resolving
//! symlinks as we go, makes the dependency explicit.

use std::path::Path;

fn main() {
    verify_rust_inventory();
    if let Ok(projects) = std::fs::read_dir("src/templates/project") {
        for project in projects.flatten() {
            let name = project.file_name();
            let owner = match name.to_str() {
                Some("billing-stripe") => "billing-base",
                Some("messaging-twilio") => "messaging-base",
                Some(name) => name,
                None => continue,
            };
            let source = Path::new("../blueprint").join(owner).join(".gitignore");
            let embedded = project.path().join(".gitignore");
            if source.is_file() && embedded.exists() {
                verify_embedded_source(&source, &embedded);
            }
        }
    }
    emit_rerun_for_tree(Path::new("src/templates"));
    // The blueprint is where the symlinked template directories actually live.
    emit_rerun_for_tree(Path::new("../blueprint"));
}

// Module HTTP layers are individual links so Cargo.toml can be renamed for
// cargo package. A newly added source file otherwise goes silently unembedded.
// Published CLI packages contain real files and have no adjacent source trees.
fn verify_rust_inventory() {
    let catalog = Path::new("src/templates/rust/module-http");
    verify_source_catalog(catalog);
    let blueprint = Path::new("../blueprint-rs/crates/implementations");
    if blueprint.is_dir() {
        for module in ["iam", "billing", "cac", "ecommerce"] {
            let source = blueprint.join(module).join(format!("{module}-axum"));
            verify_embedded_source(
                &source.join("Cargo.toml"),
                &catalog.join(module).join("Cargo.toml.template"),
            );
            verify_source_tree(&source.join("src"), &catalog.join(module).join("src"));
        }
    }
}

fn verify_source_catalog(directory: &Path) {
    if !directory.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(directory).expect("Read embedded source catalog") {
        let path = entry.expect("Read embedded source entry").path();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(
            !ignored_rust_source(&name)
                && !name.ends_with(".pyc")
                && !name.ends_with(".pyo")
                && !name.ends_with(".local")
                && (!name.starts_with(".env") || name.ends_with(".example")),
            "Non-source state in embedded catalog: {}",
            path.display()
        );
        if path.is_dir() {
            assert!(
                !path.is_symlink(),
                "Directory links may embed ignored state: {}",
                path.display()
            );
            verify_source_catalog(&path);
        }
    }
}

fn ignored_rust_source(name: &str) -> bool {
    matches!(
        name,
        "target"
            | "node_modules"
            | ".git"
            | ".DS_Store"
            | "__pycache__"
            | "dist"
            | "artifacts"
            | "secrets"
    )
}

fn embedded_name(name: &str) -> String {
    if matches!(name, "Cargo.toml" | "Cargo.lock") {
        format!("{name}.template")
    } else {
        name.to_owned()
    }
}

fn verify_source_tree(source: &Path, destination: &Path) {
    println!("cargo:rerun-if-changed={}", source.display());
    assert!(
        !destination.is_symlink(),
        "Embedded source directory must use per-file links: {}. Run scripts/sync_rust_embedded_sources.py",
        destination.display()
    );
    for entry in std::fs::read_dir(source).expect("Read Rust source inventory") {
        let path = entry.expect("Read Rust source entry").path();
        let name = path.file_name().unwrap().to_string_lossy();
        if ignored_rust_source(&name)
            || (name.starts_with(".env") && !name.ends_with(".example"))
            || name.ends_with(".local")
            || name.ends_with(".pyc")
            || name.ends_with(".pyo")
        {
            continue;
        }
        if path.is_dir() {
            verify_source_tree(&path, &destination.join(name.as_ref()));
        } else if path.is_file() {
            verify_embedded_source(&path, &destination.join(embedded_name(&name)));
        }
    }
}

fn verify_embedded_source(source: &Path, embedded: &Path) {
    let matches = if source
        .file_name()
        .is_some_and(|name| name == ".gitignore" || name == ".gitattributes")
    {
        // Git refuses symlink ignore/attribute files; use exact source copies.
        !embedded.is_symlink()
            && std::fs::read(source)
                .ok()
                .zip(std::fs::read(embedded).ok())
                .is_some_and(|(source, embedded)| source == embedded)
    } else {
        embedded.canonicalize().ok() == source.canonicalize().ok()
    };
    assert!(
        matches,
        "Rust template inventory is stale at {}. Run `python3 scripts/sync_rust_embedded_sources.py` from the repository root before rebuilding the CLI.",
        embedded.display()
    );
}

fn emit_rerun_for_tree(dir: &Path) {
    // canonicalize resolves the symlink, so the paths handed to cargo are the
    // real files rather than the links pointing at them.
    let Ok(resolved) = dir.canonicalize() else {
        return;
    };
    println!("cargo:rerun-if-changed={}", resolved.display());

    let Ok(entries) = std::fs::read_dir(&resolved) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // node_modules and build output dwarf the template tree and never
        // affect what gets baked in; walking them would make every build slow
        // for no benefit.
        if path.file_name().is_some_and(|name| {
            matches!(
                name.to_str(),
                Some("node_modules") | Some("dist") | Some("lib") | Some(".git") | Some("target")
            )
        }) {
            continue;
        }
        if path.is_dir() {
            emit_rerun_for_tree(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
