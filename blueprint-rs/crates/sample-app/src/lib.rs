#![forbid(unsafe_code)]

use forklaunch_blueprint_core::{BlueprintModule, PageRequest};

#[must_use]
pub fn module() -> BlueprintModule {
    BlueprintModule::new("sample-app", "Sample Rust blueprint application")
        .with_tag("kind", "app")
        .with_tag("language", "rust")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleAppConfig {
    pub service_page: PageRequest,
    pub enable_sdk_generation: bool,
}

impl SampleAppConfig {
    #[must_use]
    pub fn local() -> Self {
        Self {
            service_page: PageRequest::first_page(50),
            enable_sdk_generation: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_app_module_has_rust_metadata() {
        let module = module();

        assert_eq!(module.tags.get("kind").map(String::as_str), Some("app"));
        assert_eq!(SampleAppConfig::local().service_page.limit, 50);
    }
}
