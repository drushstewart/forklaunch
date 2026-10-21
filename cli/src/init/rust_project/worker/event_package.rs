//! Declare worker event dependencies before the first workspace installation.
use super::{Path, RenderedTemplate, Result};

// These renderers are crate-private in the published framework. Reuse the exact
// embedded source instead of maintaining a second set of package dependency pins.
#[allow(dead_code)]
#[path = "../../../templates/rust/framework/crates/core/src/build/service_package.rs"]
mod canonical;

pub(super) fn templates(directory: &Path, name: &str) -> Result<Vec<RenderedTemplate>> {
    let mut templates = Vec::new();
    for (file, content) in [
        ("package.json", canonical::event_package(name)?),
        ("tsconfig.json", canonical::EVENT_TSCONFIG.to_owned()),
    ] {
        let path = directory.join("events/typescript").join(file);
        // Reinitialization may fill missing files, but must not reset application
        // dependency pins or compiler settings. Planning itself never writes.
        match std::fs::symlink_metadata(&path) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        templates.push(RenderedTemplate {
            path,
            content,
            context: None,
        });
    }
    Ok(templates)
}

#[cfg(test)]
mod tests;
