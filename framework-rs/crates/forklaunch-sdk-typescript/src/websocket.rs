//! Checked Node WebSocket clients with typed raw-schema payloads.
use std::{collections::BTreeSet, fmt::Write as _};

use anyhow::{Result, bail};
use forklaunch_sdk_codegen::websocket::SocketSdkPlan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScriptWebSocketSdkOptions {
    pub client_name: String,
}
impl TypeScriptWebSocketSdkOptions {
    #[must_use]
    pub fn new(client_name: impl Into<String>) -> Self {
        Self {
            client_name: client_name.into(),
        }
    }
}

/// Generate a Node `ws` client with AJV validation, bounded queues and owned iterators.
/// Consumers need ws 8.22, AJV 8, ajv-formats 3 and Node/ws TypeScript declarations.
/// Client class names must begin with an ASCII uppercase letter.
///
/// # Errors
/// Rejects invalid identifiers, conflicting names, unsupported dialects and schema serialization failures.
pub fn generate_typescript_websocket_sdk_source(
    plan: &SocketSdkPlan,
    options: &TypeScriptWebSocketSdkOptions,
) -> Result<String> {
    let mut names = client_names(&options.client_name)?;
    let payloads = [
        &plan.incoming,
        &plan.outgoing,
        &plan.ping,
        &plan.pong,
        &plan.close,
    ];
    for payload in payloads {
        if let Some(dialect) = payload
            .schema
            .get("$schema")
            .and_then(serde_json::Value::as_str)
            && !dialect.contains("/draft-06/")
            && !dialect.contains("/draft-07/")
            && !dialect.contains("/draft/2019-09/")
            && !dialect.contains("/draft/2020-12/")
        {
            bail!("Unsupported TypeScript WebSocket JSON Schema dialect: {dialect}");
        }
    }
    let aliases = [
        "SocketIncoming",
        "SocketOutgoing",
        "SocketPing",
        "SocketPong",
        "SocketClose",
    ];
    let mut source = runtime_source();
    for (alias, payload) in aliases.into_iter().zip(payloads) {
        if !names.insert(alias.into()) {
            bail!("Conflicting TypeScript socket payload name {alias}");
        }
        for (name, definition) in &payload.models.typescript_definitions {
            if !names.insert(name.clone()) {
                bail!("Conflicting TypeScript socket definition {name}");
            }
            let _ = writeln!(source, "export type {name} = {definition};");
        }
        let _ = writeln!(
            source,
            "export type {alias} = {};",
            payload.models.typescript_type
        );
    }
    let schemas = serde_json::json!({"incoming":plan.incoming.schema,"outgoing":plan.outgoing.schema,"ping":plan.ping.schema,"pong":plan.pong.schema,"close":plan.close.schema});
    let encoded = forklaunch_common::json_string(&schemas.to_string());
    let _ = writeln!(
        source,
        "const generatedSocketSchemas = JSON.parse({encoded}) as SocketSchemaSet;"
    );
    let _ = writeln!(
        source,
        "export const SOCKET_PATH = {};",
        forklaunch_common::json_string(&plan.path)
    );
    let _ = writeln!(
        source,
        "export class {} extends SocketRuntime<SocketIncoming, SocketOutgoing, SocketPing, SocketPong, SocketClose> {{",
        options.client_name
    );
    source.push_str("private constructor(url: string, options: SocketClientOptions) { super(url, generatedSocketSchemas, options); }\n");
    let _ = writeln!(
        source,
        "static async connect(url: string, options: SocketClientOptions = {{}}): Promise<{}> {{",
        options.client_name
    );
    let _ = writeln!(
        source,
        "const client = new {}(url, options); await client.waitForOpen(); return client;\n}}\n}}",
        options.client_name
    );
    Ok(source)
}

fn runtime_source() -> String {
    let mut source = String::from(include_str!("websocket-runtime.ts"));
    // Official Draft 6 meta-schema, embedded to keep both module formats self-contained.
    let schema = forklaunch_common::json_string(include_str!("draft6-schema.json"));
    let _ = writeln!(
        source,
        "const socketDraft6MetaSchema = JSON.parse({schema}) as Record<string, unknown>;"
    );
    source
}

fn client_names(client_name: &str) -> Result<BTreeSet<String>> {
    let mut chars = client_name.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_uppercase())
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        bail!("Unsupported TypeScript socket client identifier");
    }
    let reserved = [
        "WebSocket",
        "SocketRuntime",
        "SocketCodec",
        "SocketEvent",
        "SocketClientOptions",
        "SocketReadOptions",
        "SocketWriteOptions",
        "SocketUpgradeResponse",
        "SocketSchemaSet",
        "SocketWaiter",
        "ClientOptions",
        "RawData",
        "IncomingHttpHeaders",
        "Ajv",
        "Ajv2020",
        "Ajv2019",
        "AnySchema",
        "ValidateFunction",
        "Record",
        "Buffer",
        "Error",
        "Promise",
        "Set",
        "Object",
        "Number",
        "String",
        "BigInt",
        "Array",
        "Math",
        "JSON",
        "Symbol",
        "AbortSignal",
        "AsyncIterator",
        "AsyncIterable",
        "ReturnType",
        "Pick",
        "Omit",
        "SOCKET_PATH",
    ];
    let mut names: BTreeSet<_> = reserved.into_iter().map(str::to_owned).collect();
    if !names.insert(client_name.to_owned()) {
        bail!("Conflicting TypeScript socket client name");
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_ws::{SocketContract, SocketSchemaEntry};
    use serde_json::json;

    #[test]
    fn generated_typescript_socket_client_fixture() {
        let mut contract = SocketContract::new("/chat")
            .with_client_message("chat", SocketSchemaEntry::new(json!({"type":"object","required":["message"],"properties":{"message":{"type":"string","minLength":1,"maxLength":32}},"additionalProperties":false})))
            .with_server_message("chat", SocketSchemaEntry::new(json!({"type":"object","required":["message"],"properties":{"message":{"type":"string","minLength":1,"maxLength":32}},"additionalProperties":false})));
        contract.ping = Some(SocketSchemaEntry::new(
            json!({"type":"object","required":["ts"],"properties":{"ts":{"type":"integer","minimum":1}},"additionalProperties":false}),
        ));
        contract.pong = contract.ping.clone();
        contract.close_reason.insert("reason".into(), SocketSchemaEntry::new(json!({"type":"object","required":["reason"],"properties":{"reason":{"type":"string"}},"additionalProperties":false})));
        let plan = SocketSdkPlan::from_contract(&contract).unwrap();
        let source = generate_typescript_websocket_sdk_source(
            &plan,
            &TypeScriptWebSocketSdkOptions::new("ForkLaunchSocketClient"),
        )
        .unwrap();
        if let Ok(output) = std::env::var("FORKLAUNCH_TEST_TS_WS_SDK_OUTPUT") {
            std::fs::write(&output, source).unwrap();
            contract.server_messages.clear();
            contract.server_messages.insert("reply".into(), SocketSchemaEntry::new(json!({"type":"object","required":["accepted"],"properties":{"accepted":{"type":"string"}},"additionalProperties":false})));
            let native = generate_typescript_websocket_sdk_source(
                &SocketSdkPlan::from_contract(&contract).unwrap(),
                &TypeScriptWebSocketSdkOptions::new("ForkLaunchSocketClient"),
            )
            .unwrap();
            std::fs::write(format!("{output}.native"), native).unwrap();
            let recursive = SocketContract::new("/tree")
                .with_server_message("tree", SocketSchemaEntry::new(json!({"$ref":"#/$defs/Node", "$defs":{"Node":{"type":"object","required":["value"],"properties":{"value":{"type":"integer"},"next":{"$ref":"#/$defs/Node"}},"additionalProperties":false}}})))
                .with_server_message("text", SocketSchemaEntry::new(json!({"type":"string"})))
                .with_client_message("accepted", SocketSchemaEntry::new(json!({"type":"boolean"})));
            for (suffix, contract) in [
                ("recursive", recursive),
                ("empty", SocketContract::new("/empty")),
                ("draft6", SocketContract::new("/draft-six")
                    .with_server_message("values", SocketSchemaEntry::new(json!({"$schema":"http://json-schema.org/draft-06/schema#","type":"array","items":{"type":"integer"},"contains":{"minimum":2}})))
                    .with_client_message("request", SocketSchemaEntry::new(json!({"$schema":"http://json-schema.org/draft-06/schema#","type":"string","minLength":1})))),
            ] {
                let source = generate_typescript_websocket_sdk_source(
                    &SocketSdkPlan::from_contract(&contract).unwrap(),
                    &TypeScriptWebSocketSdkOptions::new("ForkLaunchSocketClient"),
                )
                .unwrap();
                std::fs::write(format!("{output}.{suffix}"), source).unwrap();
            }
        }
        for name in ["SocketIncoming", "WebSocket", "Number", "x; throw Error()"] {
            assert!(
                generate_typescript_websocket_sdk_source(
                    &plan,
                    &TypeScriptWebSocketSdkOptions::new(name)
                )
                .is_err()
            );
        }
    }
    #[test]
    fn rejects_unsupported_dialects_before_generating_clients() {
        let contract = SocketContract::new("/legacy").with_server_message(
            "legacy",
            SocketSchemaEntry::new(
                json!({"$schema":"http://json-schema.org/draft-04/schema#","type":"string"}),
            ),
        );
        let plan = SocketSdkPlan::from_contract(&contract).unwrap();
        let error = generate_typescript_websocket_sdk_source(
            &plan,
            &TypeScriptWebSocketSdkOptions::new("LegacyClient"),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Unsupported TypeScript WebSocket JSON Schema dialect")
        );
    }
    #[test]
    fn generates_declared_draft_six_clients() {
        let contract = SocketContract::new("/draft-six").with_server_message(
            "values", SocketSchemaEntry::new(json!({"$schema":"http://json-schema.org/draft-06/schema#","type":"array","items":{"type":"integer"},"contains":{"minimum":2}})),
        );
        let plan = SocketSdkPlan::from_contract(&contract).unwrap();
        let source = generate_typescript_websocket_sdk_source(
            &plan,
            &TypeScriptWebSocketSdkOptions::new("DraftSixClient"),
        )
        .unwrap();
        assert!(source.contains("const socketDraft6MetaSchema = JSON.parse("));
        assert!(source.contains("export type SocketIncoming = Array<number>"));
    }
}
