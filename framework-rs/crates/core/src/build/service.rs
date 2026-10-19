//! Shared service build layout for ordinary CLI projects.
use crate::build::{rust_http_module, typescript_source};
use anyhow::{bail, Context, Result};
use forklaunch_core::contract::ContractRegistry;
use std::path::{Path, PathBuf};

pub struct ServiceSdkPaths {
    pub typescript: PathBuf,
    pub rust: PathBuf,
}

/// Find the owning application even after a service is relocated deeper in src.
/// # Errors
/// Rejects build scripts outside a generated application.
pub fn application_root(manifest_dir: impl AsRef<Path>) -> Result<PathBuf> {
    manifest_dir
        .as_ref()
        .ancestors()
        .find(|path| path.join(".forklaunch/manifest.toml").is_file())
        .map(Path::to_path_buf)
        .context("Service must belong to a ForkLaunch application")
}

/// Emit both language SDKs, contract and `OpenAPI` from the same service registry.
/// Environment output overrides match the CLI's ordinary generated services.
/// Returned paths let optional socket/event assets share the same layout.
/// # Errors
/// Rejects invalid service names, schemas, application roots and filesystem failures.
pub fn service_sdks(
    registry: &ContractRegistry,
    service: &str,
    client_name: &str,
) -> Result<ServiceSdkPaths> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    service_sdks_at(&manifest_dir, registry, service, client_name)
}

/// Emit service-owned artifacts from either its server or its SDK build script.
/// The explicit directory avoids changing process-wide Cargo environment values.
/// # Errors
/// Rejects invalid declarations, output paths and filesystem failures.
pub fn service_sdks_at(
    manifest_dir: &Path,
    registry: &ContractRegistry,
    service: &str,
    client_name: &str,
) -> Result<ServiceSdkPaths> {
    if service.is_empty()
        || service.len() > 64
        || !service
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        bail!("Invalid service SDK output name");
    }
    println!("cargo:rerun-if-env-changed=FORKLAUNCH_TYPESCRIPT_SDK_DIR");
    println!("cargo:rerun-if-env-changed=FORKLAUNCH_RUST_SDK_FILE");
    let root = application_root(manifest_dir)?;
    let local = manifest_dir
        .join("sdk/rust/.forklaunch-sdk-local")
        .is_file();
    let client_name = if local { "Client" } else { client_name };
    let paths = ServiceSdkPaths {
        typescript: std::env::var_os("FORKLAUNCH_TYPESCRIPT_SDK_DIR").map_or_else(
            || {
                if local {
                    manifest_dir.join("sdk/typescript")
                } else {
                    root.join("src/client-sdk/typescript").join(service)
                }
            },
            PathBuf::from,
        ),
        rust: std::env::var_os("FORKLAUNCH_RUST_SDK_FILE").map_or_else(
            || {
                if local {
                    manifest_dir.join("sdk/rust/generated.rs")
                } else {
                    root.join("src/client-sdk/rust")
                        .join(format!("{service}.rs"))
                }
            },
            PathBuf::from,
        ),
    };
    if local {
        let mut artifact: serde_json::Value =
            serde_json::from_str(&registry.clone().into_artifact().to_json())?;
        artifact["rustClientName"] = client_name.into();
        artifact["rustOutputLayout"] = "service-module".into();
        artifact["sdkLayout"] = "project-local-v1".into();
        artifact["typescriptRuntimeLayout"] = "local".into();
        let contract = serde_json::to_string(&artifact)?;
        let sources = super::http_artifact_sources(&contract, service)?;
        for (name, source) in sources.typescript_files {
            super::write_output(&paths.typescript.join(name), &source)?;
        }
        super::write_output(&paths.typescript.join("contract.json"), &contract)?;
        super::write_output(
            &paths.typescript.join("openapi.json"),
            &crate::openapi::generate_portable_openapi_json(registry)?,
        )?;
    } else {
        typescript_source(registry, service, &paths.typescript, Some(client_name))?;
    }
    if !local && std::env::var_os("FORKLAUNCH_TYPESCRIPT_SDK_DIR").is_none() {
        crate::build::write_output(
            &paths.typescript.join(".forklaunch-generated"),
            "forklaunch.sdk.http.v1\n",
        )?;
        let package = root.join("src/client-sdk/typescript");
        crate::build::service_package::refresh(&package)?;
        for file in ["index.ts", "package.json", ".forklaunch-sdk-package.json"] {
            println!("cargo:rerun-if-changed={}", package.join(file).display());
        }
    }

    if !local && std::env::var_os("FORKLAUNCH_TYPESCRIPT_SDK_DIR").is_some() {
        crate::build::service_package::http(&paths.typescript)?;
    }

    rust_http_module(
        registry,
        &format!("{service}-sdk"),
        client_name,
        &paths.rust,
    )?;
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_and_service_emit_identical_local_artifacts() {
        let root = std::env::temp_dir().join(format!("forklaunch-local-artifacts-{}", std::process::id()));
        let service = root.join("src/api");
        std::fs::create_dir_all(root.join(".forklaunch")).unwrap();
        std::fs::write(root.join(".forklaunch/manifest.toml"), "app_name='fixture'").unwrap();
        std::fs::create_dir_all(service.join("sdk/rust")).unwrap();
        std::fs::write(service.join("sdk/rust/.forklaunch-sdk-local"), "forklaunch.sdk.local.v1\n").unwrap();
        let registry = ContractRegistry::new();
        let first = service_sdks_at(&service, &registry, "api", "Client").unwrap();
        let contract = std::fs::read(first.typescript.join("contract.json")).unwrap();
        let rust = std::fs::read(&first.rust).unwrap();
        let second = service_sdks_at(&service, &registry, "api", "ForkLaunchClient").unwrap();
        assert_eq!(contract, std::fs::read(second.typescript.join("contract.json")).unwrap());
        assert_eq!(rust, std::fs::read(second.rust).unwrap());
        assert!(String::from_utf8(contract).unwrap().contains("\"rustClientName\":\"Client\""));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn relocated_service_finds_owning_application() {
        let root =
            std::env::temp_dir().join(format!("forklaunch-build-layout-{}", std::process::id()));
        let service = root.join("src/team/notes");
        std::fs::create_dir_all(&service).unwrap();
        assert!(application_root(&service).is_err());
        std::fs::create_dir_all(root.join(".forklaunch")).unwrap();
        std::fs::write(
            root.join(".forklaunch/manifest.toml"),
            "application = 'fixture'\n",
        )
        .unwrap();
        assert_eq!(application_root(&service).unwrap(), root);
        std::fs::remove_dir_all(root).unwrap();
    }
}
