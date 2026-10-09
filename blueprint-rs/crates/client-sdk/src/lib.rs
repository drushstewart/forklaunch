#![forbid(unsafe_code)]

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueprintSdkTarget {
    pub service_name: String,
    pub target_language: String,
}

impl BlueprintSdkTarget {
    #[must_use]
    pub fn typescript(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            target_language: "typescript".to_owned(),
        }
    }

    #[must_use]
    pub fn rust(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            target_language: "rust".to_owned(),
        }
    }

    #[must_use]
    pub fn package_name(&self) -> String {
        match self.target_language.as_str() {
            "rust" => format!("{}-sdk", self.service_name),
            "typescript" => format!("@forklaunch/{}/sdk", self.service_name),
            _ => format!("{}-{}", self.service_name, self.target_language),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlueprintSdkPlan {
    pub targets: Vec<BlueprintSdkTarget>,
}

impl BlueprintSdkPlan {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_target(mut self, target: BlueprintSdkTarget) -> Self {
        self.targets.push(target);
        self
    }

    #[must_use]
    pub fn package_names(&self) -> Vec<String> {
        self.targets
            .iter()
            .map(BlueprintSdkTarget::package_name)
            .collect()
    }
}

/// Generates TypeScript and Rust SDK source from blueprint service contracts.
///
/// Both generators are driven entirely by the published contract registries, so
/// any contract change deterministically changes the generated SDKs — the basis
/// for reviewable SDK diffs and cross-language conformance.
pub mod generate {
    use forklaunch_blueprint_core::contract::ContractRegistry;
    use forklaunch_sdk_codegen::{SdkGenerationPlan, SdkTarget};
    use forklaunch_sdk_rust::{RustSdkOptions, generate_rust_sdk_source};
    use forklaunch_sdk_typescript::{TypeScriptSdkOptions, generate_typescript_sdk_source};

    /// Generates a browser-safe TypeScript fetch client for a contract registry.
    #[must_use]
    pub fn typescript_sdk(registry: &ContractRegistry, client_name: &str) -> String {
        let plan = SdkGenerationPlan::from_registry(SdkTarget::TypeScript, registry);
        generate_typescript_sdk_source(&plan, &TypeScriptSdkOptions::new(client_name))
    }

    /// Generates a Rust request-builder SDK for a contract registry.
    #[must_use]
    pub fn rust_sdk(registry: &ContractRegistry, crate_name: &str) -> String {
        let plan = SdkGenerationPlan::from_registry(SdkTarget::Rust, registry);
        generate_rust_sdk_source(&plan, &RustSdkOptions::new(crate_name))
    }

    /// Generates the IAM TypeScript SDK from the IAM contracts.
    #[must_use]
    pub fn iam_typescript_sdk() -> String {
        typescript_sdk(
            &forklaunch_iam_interface::contracts::registry(),
            "IamClient",
        )
    }

    /// Generates the IAM Rust SDK from the IAM contracts.
    #[must_use]
    pub fn iam_rust_sdk() -> String {
        rust_sdk(&forklaunch_iam_interface::contracts::registry(), "iam-sdk")
    }

    /// Generates the billing TypeScript SDK from the billing contracts.
    #[must_use]
    pub fn billing_typescript_sdk() -> String {
        typescript_sdk(
            &forklaunch_billing_interface::contracts::registry(),
            "BillingClient",
        )
    }

    /// Generates the billing Rust SDK from the billing contracts.
    #[must_use]
    pub fn billing_rust_sdk() -> String {
        rust_sdk(
            &forklaunch_billing_interface::contracts::registry(),
            "billing-sdk",
        )
    }

    /// Generates the monitoring TypeScript SDK from the monitoring contracts.
    #[must_use]
    pub fn monitoring_typescript_sdk() -> String {
        typescript_sdk(
            &forklaunch_monitoring_interface::contracts::registry(),
            "MonitoringClient",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_plan_derives_package_names_for_frontend_and_rust_targets() {
        let plan = BlueprintSdkPlan::new()
            .with_target(BlueprintSdkTarget::typescript("accounts"))
            .with_target(BlueprintSdkTarget::rust("accounts"));

        assert_eq!(
            plan.package_names(),
            vec!["@forklaunch/accounts/sdk", "accounts-sdk"]
        );
    }

    #[test]
    fn generates_iam_typescript_and_rust_sdks_from_contracts() {
        let typescript = generate::iam_typescript_sdk();
        assert!(typescript.contains("export class IamClient"));
        assert!(typescript.contains("authenticate("));
        assert!(typescript.contains("getUser("));
        assert!(typescript.contains("listUsers("));
        assert!(typescript.contains("method: 'POST'"));

        let rust = generate::iam_rust_sdk();
        assert!(rust.contains("pub struct ForkLaunchClient"));
        assert!(rust.contains("pub fn get_user_request"));
        assert!(rust.contains("pub fn authenticate_request"));
    }

    #[test]
    fn generates_billing_and_monitoring_sdks_from_contracts() {
        let billing = generate::billing_typescript_sdk();
        assert!(billing.contains("export class BillingClient"));
        assert!(billing.contains("listCustomers("));
        assert!(billing.contains("createCheckoutSession("));

        let billing_rust = generate::billing_rust_sdk();
        assert!(billing_rust.contains("pub fn receive_webhook_request"));

        let monitoring = generate::monitoring_typescript_sdk();
        assert!(monitoring.contains("liveness("));
        assert!(monitoring.contains("readiness("));
    }

    #[test]
    fn sdk_generation_is_deterministic() {
        // identical contracts must produce byte-identical SDKs for reviewable diffs
        assert_eq!(
            generate::iam_typescript_sdk(),
            generate::iam_typescript_sdk()
        );
        assert_eq!(generate::iam_rust_sdk(), generate::iam_rust_sdk());
    }
}
