//! Checked raw socket payload plans shared by native client generators.
use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use forklaunch_ws::{SocketContract, SocketSchemaEntry, codec::PayloadCodec};
use serde_json::{Value, json};

use crate::{SchemaModels, compile_schema};

/// One socket payload direction, including its original runtime constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketPayloadPlan {
    pub schema: Value,
    pub models: SchemaModels,
}

/// Directions are from the client's perspective; protocol controls stay shared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketSdkPlan {
    pub path: String,
    pub incoming: SocketPayloadPlan,
    pub outgoing: SocketPayloadPlan,
    pub ping: SocketPayloadPlan,
    pub pong: SocketPayloadPlan,
    pub close: SocketPayloadPlan,
}

impl SocketSdkPlan {
    /// Compile standalone schemas into bounded models and checked runtime unions.
    /// Error/context metadata describe framework events, not wire messages.
    ///
    /// # Errors
    /// Rejects invalid paths, schemas, references, mixed union dialects and limits.
    pub fn from_contract(contract: &SocketContract) -> Result<Self> {
        if !contract.path.starts_with('/')
            || contract.path.contains(['?', '#'])
            || contract.path.chars().any(char::is_control)
        {
            bail!(
                "Socket SDK path must be an absolute path without query/fragment/control characters"
            );
        }
        Ok(Self {
            path: contract.path.clone(),
            incoming: collection("SocketIncoming", &contract.server_messages)?,
            outgoing: collection("SocketOutgoing", &contract.client_messages)?,
            ping: payload(
                "SocketPing",
                contract.ping.as_ref().map(|entry| &entry.shape),
            )?,
            pong: payload(
                "SocketPong",
                contract.pong.as_ref().map(|entry| &entry.shape),
            )?,
            close: collection("SocketClose", &contract.close_reason)?,
        })
    }
}

fn payload(name: &str, schema: Option<&Value>) -> Result<SocketPayloadPlan> {
    let empty = json!({});
    let schema = schema.unwrap_or(&empty);
    // Validate constraints as well as the type graph before emitting source.
    PayloadCodec::<Value>::json_schema(schema).context("Invalid socket payload schema")?;
    let models = compile_schema(name, schema)?;
    Ok(SocketPayloadPlan {
        schema: schema.clone(),
        models,
    })
}

fn collection(
    name: &str,
    entries: &BTreeMap<String, SocketSchemaEntry>,
) -> Result<SocketPayloadPlan> {
    if entries.keys().any(String::is_empty) {
        bail!("Socket SDK message names must not be empty");
    }
    if entries.len() <= 1 {
        return payload(name, entries.values().next().map(|entry| &entry.shape));
    }
    if entries.len() > 1024 {
        bail!("Socket message union exceeds 1024 alternatives");
    }
    // Bound the complete union before any schema cloning or compilation.
    let mut remaining = 100_000_usize;
    let mut pending: Vec<_> = entries.values().map(|entry| (&entry.shape, 0)).collect();
    while let Some((value, depth)) = pending.pop() {
        if depth > 60 || remaining == 0 {
            bail!("Socket message union exceeds schema depth/node limits");
        }
        remaining -= 1;
        let children = match value {
            Value::Object(values) => values.len(),
            Value::Array(values) => values.len(),
            _ => 0,
        };
        if children > remaining.saturating_sub(pending.len()) {
            bail!("Socket message union exceeds schema node limits");
        }
        match value {
            Value::Object(values) => {
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            Value::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            _ => {}
        }
        if pending.len() > remaining {
            bail!("Socket message union exceeds schema node limits");
        }
    }
    let mut bytes = UnionBytes(32 * 1024 * 1024);
    for entry in entries.values() {
        serde_json::to_writer(&mut bytes, &entry.shape)
            .context("Socket message union exceeds 32 MiB")?;
    }
    let mut schemas = Vec::with_capacity(entries.len());
    let mut dialect = None;
    for (index, entry) in entries.values().enumerate() {
        // Validate the standalone graph before rebasing its document-local refs.
        let checked = payload(&format!("{name}Variant{index}"), Some(&entry.shape))?;
        let current = checked
            .schema
            .get("$schema")
            .and_then(Value::as_str)
            .unwrap_or("http://json-schema.org/draft-07/schema#");
        if dialect.as_deref().is_some_and(|dialect| dialect != current) {
            bail!("Socket message unions must use one JSON Schema dialect");
        }
        dialect = Some(current.to_owned());
        let relocated = forklaunch_contract::relocate_schema_json(
            &checked.schema.to_string(),
            &["anyOf", &index.to_string()],
        );
        schemas.push(serde_json::from_str::<Value>(&relocated)?);
    }
    let schema = json!({"$schema": dialect, "anyOf": schemas});
    payload(name, Some(&schema))
}

struct UnionBytes(usize);
impl std::io::Write for UnionBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("socket union byte limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_directions_rebase_standalone_recursive_alternatives_and_keep_literal_data() {
        let tree = json!({"$ref":"#/$defs/Node", "$defs":{"Node":{
            "type":"object", "required":["value"], "properties":{
                "value":{"type":"integer"}, "next":{"$ref":"#/$defs/Node"}},
            "examples":[{"$ref":"literal"}]}}});
        let contract = SocketContract::new("/chat")
            .with_server_message("tree", SocketSchemaEntry::new(tree))
            .with_server_message(
                "text",
                SocketSchemaEntry::new(json!({"type":"string","minLength":1})),
            )
            .with_client_message(
                "count",
                SocketSchemaEntry::new(json!({"type":"integer","minimum":0})),
            );
        let plan = SocketSdkPlan::from_contract(&contract).unwrap();
        let incoming = PayloadCodec::<Value>::json_schema(&plan.incoming.schema).unwrap();
        assert!(
            incoming
                .decode(br#"{"value":1,"next":{"value":2}}"#)
                .is_ok()
        );
        assert!(
            incoming
                .decode(br#"{"value":1,"next":{"value":"bad"}}"#)
                .is_err()
        );
        assert!(incoming.decode(b"hello").is_ok());
        assert!(incoming.decode(b"123").is_err());
        assert_eq!(
            plan.incoming.schema["anyOf"][1]["$ref"],
            "#/anyOf/1/$defs/Node"
        );
        assert_eq!(
            plan.incoming.schema["anyOf"][1]["$defs"]["Node"]["examples"][0]["$ref"],
            "literal"
        );
        assert_eq!(plan.outgoing.models.rust_type, "SocketOutgoing");
        assert_eq!(plan.outgoing.models.rust_aliases[0].target_type, "i64");
        assert!(!plan.incoming.models.rust_models.is_empty());
    }

    #[test]
    fn draft_four_models_ignore_later_keywords_and_reject_legacy_resource_ids() {
        let schema = json!({"$schema":"http://json-schema.org/draft-04/schema#","type":"object","required":["value"],"properties":{"value":{"type":"string","const":"ignored"}},"const":{"literal":"annotation"}});
        let contract = SocketContract::new("/legacy")
            .with_server_message("legacy", SocketSchemaEntry::new(schema.clone()));
        let plan = SocketSdkPlan::from_contract(&contract).unwrap();
        assert!(
            plan.incoming
                .models
                .typescript_type
                .contains("value: string")
        );
        assert_eq!(plan.incoming.schema, schema);
        for schema in [
            json!({"$schema":"http://json-schema.org/draft-04/schema#","id":"https://example.test/root","type":"string"}),
            json!({"$schema":"http://json-schema.org/draft-04/schema#","type":"object","properties":{"value":{"id":"nested","type":"string"}}}),
        ] {
            let contract = SocketContract::new("/legacy")
                .with_server_message("legacy", SocketSchemaEntry::new(schema));
            assert!(SocketSdkPlan::from_contract(&contract).is_err());
        }
    }

    #[test]
    fn malformed_constraints_scopes_and_mixed_dialects_fail_before_generation() {
        for schema in [
            json!({"type":"string","minLength":-1}),
            json!({"$ref":"#/missing"}),
            json!({"$id":"https://example.test/schema","type":"string"}),
        ] {
            let contract = SocketContract::new("/chat")
                .with_client_message("input", SocketSchemaEntry::new(schema));
            assert!(SocketSdkPlan::from_contract(&contract).is_err());
        }
        let contract = SocketContract::new("/chat")
            .with_client_message("draft7", SocketSchemaEntry::new(json!({"type":"string"})))
            .with_client_message("draft2020", SocketSchemaEntry::new(json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"integer"})));
        assert!(SocketSdkPlan::from_contract(&contract).is_err());
    }
}
