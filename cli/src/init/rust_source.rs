//! Shared source filtering and guidance for ordinary Rust init commands.
use std::{fs, path::Path};
use anyhow::{Context, Result};

pub(crate) fn write_rust_guidance(root: &Path) -> Result<()> {
    let path = root.join(".agents/skills/rust-framework/SKILL.md");
    fs::create_dir_all(path.parent().context("Skill path has no parent")?)?;
    // Preserve local guidance when adding modules or rerunning setup.
    if !path.exists() {
        fs::write(
            path,
            include_str!("../../assets/forklaunch-skills/rust-framework/SKILL.md"),
        )?;
    }
    Ok(())
}

/// Source snapshots and receipts exclude local state, including nested paths
/// reached through source-catalog directory symlinks. Environment examples are
/// declarative templates; ordinary local environment generation is unchanged.
pub(super) fn is_source_path(path: &Path) -> bool {
    path.components().all(|part| {
        let std::path::Component::Normal(name) = part else {
            return false;
        };
        let name = name.to_string_lossy();
        !matches!(
            name.as_ref(),
            "target"
                | "node_modules"
                | "dist"
                | ".git"
                | ".DS_Store"
                | "__pycache__"
                | "artifacts"
                | "secrets"
        ) && !name.ends_with(".pyc")
            && !name.ends_with(".pyo")
            && !name.ends_with(".local")
            && (!name.starts_with(".env") || name.ends_with(".example"))
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_source_filter_excludes_private_and_runtime_files() {
        for path in ["src/main.rs", ".env.example", "src/api/.env.database.example"] {
            assert!(is_source_path(Path::new(path)), "{path}");
        }
        for path in ["../outside", ".env", "src/api/.env.local", "nested/node_modules/pkg/index.js", "nested/target/debug/app", "secrets/key", "nested/__pycache__/test.pyc"] {
            assert!(!is_source_path(Path::new(path)), "{path}");
        }
    }
}
