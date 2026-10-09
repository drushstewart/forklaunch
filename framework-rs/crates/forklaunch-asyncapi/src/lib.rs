#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use forklaunch_common::{JsonField, json_array, json_object};
use forklaunch_contract::SchemaRef;
use forklaunch_ws::{MessageDirection as WsDirection, WebSocketContract};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsyncApiInfo {
    pub title: String,
    pub version: String,
}

impl AsyncApiInfo {
    #[must_use]
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageProtocol {
    Kafka,
    Redis,
    WebSocket,
    HttpWebhook,
}

impl MessageProtocol {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Kafka => "kafka",
            Self::Redis => "redis",
            Self::WebSocket => "ws",
            Self::HttpWebhook => "http",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    Publish,
    Subscribe,
}

impl MessageDirection {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Subscribe => "subscribe",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageContract {
    pub name: String,
    pub title: String,
    pub payload: SchemaRef,
    pub headers: BTreeMap<String, SchemaRef>,
}

impl MessageContract {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        title: impl Into<String>,
        payload: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            title: title.into(),
            payload: SchemaRef::named(payload),
            headers: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, schema: impl Into<String>) -> Self {
        self.headers.insert(name.into(), SchemaRef::named(schema));
        self
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let headers = self
            .headers
            .iter()
            .map(|(name, schema)| {
                format!(
                    "{{\"name\":{},\"schema\":{}}}",
                    json_string(name),
                    schema.to_json()
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "{{\"name\":{},\"title\":{},\"payload\":{},\"headers\":[{}]}}",
            json_string(&self.name),
            json_string(&self.title),
            self.payload.to_json(),
            headers
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelContract {
    pub name: String,
    pub address: String,
    pub protocol: MessageProtocol,
    pub direction: MessageDirection,
    pub message: MessageContract,
}

impl ChannelContract {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        address: impl Into<String>,
        protocol: MessageProtocol,
        direction: MessageDirection,
        message: MessageContract,
    ) -> Self {
        Self {
            name: name.into(),
            address: address.into(),
            protocol,
            direction,
            message,
        }
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"name\":{},\"address\":{},\"protocol\":{},\"direction\":{},\"message\":{}}}",
            json_string(&self.name),
            json_string(&self.address),
            json_string(self.protocol.as_str()),
            json_string(self.direction.as_str()),
            self.message.to_json()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsyncApiDocument {
    pub info: AsyncApiInfo,
    pub channels: Vec<ChannelContract>,
}

impl AsyncApiDocument {
    #[must_use]
    pub fn new(info: AsyncApiInfo) -> Self {
        Self {
            info,
            channels: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_channel(mut self, channel: ChannelContract) -> Self {
        self.channels.push(channel);
        self
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let channels = self
            .channels
            .iter()
            .map(ChannelContract::to_json)
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "{{\"asyncapi\":\"3.0.0\",\"info\":{{\"title\":{},\"version\":{}}},\"channels\":[{}]}}",
            json_string(&self.info.title),
            json_string(&self.info.version),
            channels
        )
    }
}

fn json_string(input: &str) -> String {
    format!("\"{}\"", input.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Generates a spec-compliant `AsyncAPI` 3.0 document from a WebSocket contract.
///
/// Each contract maps to a single channel whose events become channel messages;
/// every event becomes an operation whose `action` reflects its direction
/// (`clientToServer` → `receive`, `serverToClient` → `send`). Output is
/// deterministic (sorted keys) so it is suitable for snapshot review.
#[must_use]
pub fn generate_asyncapi_json(info: &AsyncApiInfo, contract: &WebSocketContract) -> String {
    let channel = channel_name_from_path(&contract.path);

    let channel_messages = json_object(contract.events.iter().map(|event| {
        JsonField::new(
            &event.name,
            json_object([JsonField::new(
                "$ref",
                json_string(&format!("#/components/messages/{}", event.schema_name)),
            )]),
        )
    }));

    let channels = json_object([JsonField::new(
        &channel,
        json_object([
            JsonField::new("address", json_string(&contract.path)),
            JsonField::new("messages", channel_messages),
        ]),
    )]);

    let operations = json_object(contract.events.iter().map(|event| {
        let action = match event.direction {
            WsDirection::ClientToServer => "receive",
            WsDirection::ServerToClient => "send",
        };
        JsonField::new(
            &event.name,
            json_object([
                JsonField::new("action", json_string(action)),
                JsonField::new(
                    "channel",
                    json_object([JsonField::new(
                        "$ref",
                        json_string(&format!("#/channels/{channel}")),
                    )]),
                ),
                JsonField::new(
                    "messages",
                    json_array([json_object([JsonField::new(
                        "$ref",
                        json_string(&format!("#/channels/{channel}/messages/{}", event.name)),
                    )])]),
                ),
            ]),
        )
    }));

    let mut messages = BTreeMap::<String, String>::new();
    let mut schema_names = BTreeSet::<String>::new();
    for event in &contract.events {
        schema_names.insert(event.schema_name.clone());
        messages
            .entry(event.schema_name.clone())
            .or_insert_with(|| {
                json_object([
                    JsonField::new("name", json_string(&event.schema_name)),
                    JsonField::new(
                        "payload",
                        json_object([JsonField::new(
                            "$ref",
                            json_string(&format!("#/components/schemas/{}", event.schema_name)),
                        )]),
                    ),
                ])
            });
    }

    let components = json_object([
        JsonField::new(
            "messages",
            json_object(
                messages
                    .into_iter()
                    .map(|(name, message)| JsonField::new(name, message)),
            ),
        ),
        JsonField::new(
            "schemas",
            json_object(schema_names.into_iter().map(|name| {
                JsonField::new(
                    name,
                    json_object([JsonField::new("type", json_string("object"))]),
                )
            })),
        ),
    ]);

    json_object([
        JsonField::new("asyncapi", json_string("3.0.0")),
        JsonField::new("channels", channels),
        JsonField::new("components", components),
        JsonField::new(
            "info",
            json_object([
                JsonField::new("title", json_string(&info.title)),
                JsonField::new("version", json_string(&info.version)),
            ]),
        ),
        JsonField::new("operations", operations),
    ])
}

fn channel_name_from_path(path: &str) -> String {
    let trimmed = path.trim_matches('/').replace('/', ".");
    if trimmed.is_empty() {
        "root".to_owned()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asyncapi_document_serializes_channels() {
        let document = AsyncApiDocument::new(AsyncApiInfo::new("Orders", "1.0.0")).with_channel(
            ChannelContract::new(
                "orders-created",
                "orders.created",
                MessageProtocol::Kafka,
                MessageDirection::Subscribe,
                MessageContract::new("OrderCreated", "Order Created", "OrderCreatedPayload")
                    .with_header("correlationId", "String"),
            ),
        );

        let encoded = document.to_json();

        assert!(encoded.contains("\"asyncapi\":\"3.0.0\""));
        assert!(encoded.contains("\"orders.created\""));
        assert!(encoded.contains("\"OrderCreatedPayload\""));
    }

    #[test]
    fn generates_asyncapi_30_from_websocket_contract() {
        use forklaunch_ws::{EventContract, MessageDirection, WebSocketContract};

        let contract = WebSocketContract::new("realtime")
            .with_event(EventContract::new(
                "subscribe",
                MessageDirection::ClientToServer,
                "SubscribeRequest",
            ))
            .with_event(EventContract::new(
                "user.updated",
                MessageDirection::ServerToClient,
                "UserUpdatedEvent",
            ));

        let document = generate_asyncapi_json(&AsyncApiInfo::new("Realtime", "1.0.0"), &contract);

        assert!(document.contains("\"asyncapi\":\"3.0.0\""));
        // client-to-server event is received by the server
        assert!(document.contains("\"subscribe\":{\"action\":\"receive\""));
        // server-to-client event is sent by the server
        assert!(document.contains("\"user.updated\":{\"action\":\"send\""));
        assert!(document.contains("\"#/components/messages/SubscribeRequest\""));
        assert!(document.contains("\"channels\":{\"realtime\":{\"address\":\"/realtime\""));
    }

    #[test]
    fn asyncapi_30_output_is_deterministic() {
        use forklaunch_ws::{EventContract, MessageDirection, WebSocketContract};

        let contract = WebSocketContract::new("chat").with_event(EventContract::new(
            "message",
            MessageDirection::ClientToServer,
            "ChatMessage",
        ));
        let info = AsyncApiInfo::new("Chat", "2.0.0");

        assert_eq!(
            generate_asyncapi_json(&info, &contract),
            generate_asyncapi_json(&info, &contract)
        );
    }
}
