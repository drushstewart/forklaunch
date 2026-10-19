//! Portable compiler used by the CLI for raw socket artifacts.
use anyhow::{Context, Result, bail};
use forklaunch_core::asyncapi::{AsyncApiInfo, generate_raw_socket_asyncapi_json};
use forklaunch_core::codegen::websocket::SocketSdkPlan;
use forklaunch_core::rust_sdk::{RustSdkOptions, generate_rust_websocket_sdk_source};
use forklaunch_core::typescript::{
    TypeScriptWebSocketSdkOptions, generate_typescript_websocket_sdk_source,
};
use forklaunch_core::ws::SocketContractArtifact;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read as _, Write as _},
};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().context("socket contract artifact path")?;
    let target = args.next().context("target: typescript, rust or all")?;
    let source = args.next().unwrap_or_else(|| "socket-contract".into());
    if args.next().is_some()
        || !matches!(source.as_str(), "socket-contract" | "asyncapi")
        || !matches!(target.as_str(), "typescript" | "rust" | "all")
    {
        bail!(
            "Expected artifact path, target (typescript/rust/all), and optional source (socket-contract/asyncapi)"
        );
    }
    let mut bytes = Vec::new();
    std::fs::File::open(input)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 * 1024 {
        bail!("Socket artifact exceeds 16 MiB");
    }
    let document = (source == "asyncapi")
        .then(|| serde_json::from_slice::<Value>(&bytes))
        .transpose()?;
    let artifact: SocketContractArtifact = if let Some(document) = &document {
        forklaunch_core::asyncapi::import_server_websocket_asyncapi(document)?
    } else {
        serde_json::from_slice(&bytes)?
    };
    let mut output = compile(&artifact, &target)?;
    if let Some(document) = document {
        output["asyncapi"] = document;
    }
    let mut bytes = LimitedOutput(Vec::new());
    serde_json::to_writer(&mut bytes, &output)?;
    std::io::stdout().write_all(&bytes.0)?;
    Ok(())
}

fn compile(artifact: &SocketContractArtifact, target: &str) -> Result<Value> {
    if artifact.format_version != 1
        || artifact.title.is_empty()
        || artifact.version.is_empty()
        || artifact.sockets.is_empty()
        || artifact.sockets.len() > 64
    {
        bail!("Socket artifacts require formatVersion 1, title, version and 1 through 64 sockets");
    }
    let mut typescript = BTreeMap::new();
    let mut rust = BTreeMap::new();
    let mut document = json!({"asyncapi":"3.0.0", "info":{"title":artifact.title,"version":artifact.version},"channels":{},"operations":{}});
    for (name, contract) in &artifact.sockets {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            bail!(
                "Socket keys must contain only lowercase ASCII letters, digits and underscores (1 through 128 bytes)"
            );
        }
        let plan = SocketSdkPlan::from_contract(contract)
            .with_context(|| format!("Invalid socket {name}"))?;
        let module = format!("socket_{name}");
        if matches!(target, "typescript" | "all") {
            typescript.insert(
                module.clone(),
                generate_typescript_websocket_sdk_source(
                    &plan,
                    &TypeScriptWebSocketSdkOptions::new("SocketClient"),
                )?,
            );
        }
        if matches!(target, "rust" | "all") {
            let options = RustSdkOptions::new("socket-sdk").with_client_struct("SocketClient");
            rust.insert(module, generate_rust_websocket_sdk_source(&plan, &options)?);
        }
        let socket_document: Value = serde_json::from_str(&generate_raw_socket_asyncapi_json(
            &AsyncApiInfo::new(&artifact.title, &artifact.version),
            contract,
        )?)?;
        for section in ["channels", "operations"] {
            for (key, value) in socket_document[section].as_object().into_iter().flatten() {
                if document[section]
                    .get(key)
                    .is_some_and(|existing| existing != value)
                {
                    bail!("Conflicting AsyncAPI {section} entry {key}");
                }
                document[section][key] = value.clone();
            }
        }
    }
    Ok(json!({"typescript":typescript,"rust":rust,"asyncapi":document}))
}

struct LimitedOutput(Vec<u8>);
impl std::io::Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > 64 * 1024 * 1024 {
            return Err(std::io::Error::other(
                "Socket compiler output exceeds 64 MiB",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_core::ws::{SocketContract, SocketSchemaEntry};
    #[test]
    fn multi_socket_outputs_share_checked_generators_and_detect_conflicts() {
        let artifact = SocketContractArtifact::new("Fixture", "1")
            .with_socket(
                "chat",
                SocketContract::new("/chat")
                    .with_client_message("chat", SocketSchemaEntry::new(json!({"type":"string"}))),
            )
            .with_socket(
                "tree",
                SocketContract::new("/tree")
                    .with_server_message("tree", SocketSchemaEntry::new(json!({"type":"integer"}))),
            );
        let output = compile(&artifact, "all").unwrap();
        assert!(
            output["typescript"]["socket_chat"]
                .as_str()
                .unwrap()
                .contains("export class SocketClient")
        );
        assert!(
            output["rust"]["socket_tree"]
                .as_str()
                .unwrap()
                .contains("pub struct SocketClient")
        );
        assert_eq!(
            output["asyncapi"]["operations"].as_object().unwrap().len(),
            2
        );
        assert!(
            compile(&artifact, "rust").unwrap()["typescript"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        let mut invalid = artifact.clone();
        invalid.format_version = 2;
        assert!(compile(&invalid, "all").is_err());
        invalid = artifact.clone();
        invalid
            .sockets
            .insert("../bad".into(), SocketContract::new("/bad"));
        assert!(compile(&invalid, "all").is_err());
        invalid = artifact;
        invalid.sockets.insert(
            "conflict".into(),
            SocketContract::new("/other")
                .with_client_message("chat", SocketSchemaEntry::new(json!({"type":"boolean"}))),
        );
        assert!(compile(&invalid, "all").is_err());
    }
}
