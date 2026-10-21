//! Shared guidance for ordinary Rust init commands.
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
