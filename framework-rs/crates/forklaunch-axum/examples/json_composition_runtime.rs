//! Bounded reusable JSON evaluator integrated with HTTP and mounted MCP transport.
#[path = "json_composition/contract.rs"]
mod contract;
use axum::{Extension, Json};
use forklaunch_axum::{
    AuthErrorFormat, CorrelationIdFormat, ForkLaunchRouter, JsonRequestParsers,
    JsonTransformFailure, ParsedJsonBody, RequestValidators, RouterConfig,
};
use forklaunch_contract::{
    BodyContract, BodyKind, ResponseContract, RouteContract, SchemaRef, ValidationMode,
};
use forklaunch_core::ServiceInfo;
use forklaunch_http::{BodyParserHooks, ErrorKind, ErrorResponse, FrameworkRequest};
use forklaunch_validation::{typescript_coerce, typescript_request_validation_error_response};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn composition_schema(fallback: &Value) -> typescript_coerce::ZodJsonSchema<'_> {
    use typescript_coerce::{ZodJsonField as Field, ZodJsonSchema as Schema};
    let mut fields = vec![
        Field {
            name: "amount",
            schema: Schema::Number(vec![]),
        },
        Field {
            name: "label",
            schema: Schema::Default {
                schema: Box::new(Schema::String(vec![])),
                value: fallback,
            },
        },
        Field {
            name: "kind",
            schema: Schema::Optional(Box::new(Schema::Union(vec![
                Schema::Literal(typescript_coerce::ZodJsonLiteral::String("create")),
                Schema::Literal(typescript_coerce::ZodJsonLiteral::String("update")),
            ]))),
        },
        Field {
            name: "choice",
            schema: Schema::Optional(Box::new(Schema::Union(vec![
                Schema::String(vec![
                    typescript_coerce::ZodStringCheck {
                        rule: typescript_coerce::ZodStringRule::Step(
                            typescript_coerce::ZodStringStep::Trim,
                        ),
                        message: None,
                    },
                    typescript_coerce::ZodStringCheck {
                        rule: typescript_coerce::ZodStringRule::Step(
                            typescript_coerce::ZodStringStep::Min(3),
                        ),
                        message: None,
                    },
                ]),
                Schema::Number(vec![typescript_coerce::ZodNumberCheck {
                    step: typescript_coerce::ZodNumberStep::Int,
                    message: None,
                }]),
                Schema::Object(vec![Field {
                    name: "nested",
                    schema: Schema::Union(vec![
                        Schema::String(vec![]),
                        Schema::Array {
                            element: Box::new(Schema::Boolean),
                            checks: vec![],
                        },
                    ]),
                }]),
            ]))),
        },
        Field {
            name: "labels",
            schema: Schema::Optional(Box::new(Schema::Record {
                key: Box::new(Schema::String(vec![typescript_coerce::ZodStringCheck {
                    rule: typescript_coerce::ZodStringRule::Step(
                        typescript_coerce::ZodStringStep::Trim,
                    ),
                    message: None,
                }])),
                value: Box::new(Schema::Number(vec![])),
            })),
        },
        Field {
            name: "settings",
            schema: Schema::Optional(Box::new(Schema::StrictObject(vec![Field {
                name: "enabled",
                schema: Schema::Boolean,
            }]))),
        },
        Field {
            name: "items",
            schema: Schema::Optional(Box::new(Schema::Array {
                element: Box::new(Schema::Object(vec![Field {
                    name: "enabled",
                    schema: Schema::Boolean,
                }])),
                checks: vec![],
            })),
        },
    ];
    fields.extend(absence_fields());
    fields.extend(object_policy_fields());
    #[cfg(feature = "zod-uri")]
    fields.push(Field {
        name: "uri",
        schema: Schema::Optional(Box::new(Schema::String(vec![
            typescript_coerce::ZodStringCheck {
                rule: typescript_coerce::ZodStringRule::Format(
                    typescript_coerce::ZodStringFormat::Uri,
                ),
                message: None,
            },
        ]))),
    });
    Schema::Object(fields)
}
fn absence_fields<'a>() -> Vec<typescript_coerce::ZodJsonField<'a>> {
    use typescript_coerce::{ZodJsonField as Field, ZodJsonSchema as Schema};
    vec![
        Field {
            name: "cleared",
            schema: Schema::Optional(Box::new(Schema::Null)),
        },
        Field {
            name: "omitted",
            schema: Schema::Undefined,
        },
        Field {
            name: "voided",
            schema: Schema::Void,
        },
        Field {
            name: "forbidden",
            schema: Schema::Optional(Box::new(Schema::Never)),
        },
        Field {
            name: "nothing",
            schema: Schema::Union(vec![Schema::Void, Schema::Null, Schema::Undefined]),
        },
    ]
}

fn object_policy_fields<'a>() -> Vec<typescript_coerce::ZodJsonField<'a>> {
    use typescript_coerce::{
        ZodJsonField as Field, ZodJsonSchema as Schema, ZodJsonUnknownKeys as Policy,
    };
    vec![
        Field {
            name: "bag",
            schema: Schema::Optional(Box::new(Schema::ObjectWithPolicy {
                fields: vec![Field {
                    name: "label",
                    schema: Schema::String(vec![]),
                }],
                unknown: Policy::Passthrough,
                catchall: None,
            })),
        },
        Field {
            name: "scores",
            schema: Schema::Optional(Box::new(Schema::ObjectWithPolicy {
                fields: vec![Field {
                    name: "label",
                    schema: Schema::String(vec![]),
                }],
                unknown: Policy::Strict,
                catchall: Some(Box::new(Schema::Number(vec![]))),
            })),
        },
    ]
}

fn transform(
    request: &FrameworkRequest,
    root_default: bool,
) -> Result<Option<Value>, JsonTransformFailure> {
    use typescript_coerce::{ZodJsonOutcome, ZodJsonSchema as Schema};
    let fallback = json!("fallback");
    let schema = composition_schema(&fallback);
    let root_value = json!({"amount":7,"label":"fallback"});
    let schema = if root_default {
        Schema::Default {
            schema: Box::new(schema),
            value: &root_value,
        }
    } else {
        schema
    };
    let value = request
        .parsed_json
        .as_ref()
        .and_then(|parsed| parsed.value.as_deref());
    // This fixture registers no reviver or preceding transformation. Only at
    // that explicit boundary may decompressed bytes reconstruct original order.
    // The parser's framed-empty synthetic object has no own-key ambiguity.
    let result = if let Some(raw) = request
        .parsed_json
        .as_ref()
        .and_then(|parsed| parsed.raw_body.as_deref())
        .filter(|raw| value.is_some() && !raw.is_empty())
    {
        let document = typescript_coerce::OrderedJsonDocument::parse(raw)
            .map_err(|_| JsonTransformFailure::Operational)?;
        typescript_coerce::zod_json_ordered_at_path(&document, &schema, &["body"])
    } else {
        typescript_coerce::zod_json_at_path(value, &schema, &["body"])
    };
    match result {
        Ok(ZodJsonOutcome::Valid(value)) => Ok(value),
        Ok(ZodJsonOutcome::Invalid(errors)) => Err(JsonTransformFailure::Validation(
            typescript_request_validation_error_response(
                Some(&errors),
                Some(&request.context.correlation_id),
            ),
        )),
        Err(_) => Err(JsonTransformFailure::Operational),
    }
}
fn report(parsed: ParsedJsonBody) -> Value {
    let mut result = json!({"hasBody":parsed.value.is_some()});
    if let Some(value) = parsed.value {
        result["body"] = value.as_ref().clone();
    }
    if let Some(raw) = parsed.raw_body {
        result["raw"] = json!(String::from_utf8_lossy(&raw));
    }
    result
}
fn rejection() -> ResponseContract {
    ResponseContract::new(
        400,
        Some(BodyContract {
            kind: BodyKind::Text,
            schema: SchemaRef::named("String"),
            content_type: Some("text/plain".into()),
        }),
    )
}
#[tokio::main]
async fn main() {
    let calls = Arc::new(AtomicUsize::new(0));
    let transforms = Arc::new(AtomicUsize::new(0));
    let authentications = Arc::new(AtomicUsize::new(0));
    let mut parsers = JsonRequestParsers::default();
    let mut validators = RequestValidators::default();
    let mut router = ForkLaunchRouter::new(ServiceInfo::new("transform", "1"));
    for (name, mode) in [
        ("none", ValidationMode::None),
        ("warning", ValidationMode::Warning),
        ("error", ValidationMode::Error),
    ] {
        for profile in ["public", "protected", "default", "remove", "unsupported"] {
            let route = contract::route(name, mode, profile);
            parsers.insert(&route, BodyParserHooks::default()).unwrap();
            let transform_calls = transforms.clone();
            validators
                .insert_json_transform_checked(&route, &parsers, rejection(), move |request| {
                    transform_calls.fetch_add(1, Ordering::SeqCst);
                    match profile {
                        "remove" => Ok(None),
                        "unsupported" => {
                            let schema = typescript_coerce::ZodJsonSchema::Object(vec![
                                typescript_coerce::ZodJsonField {
                                    name: "__proto__",
                                    schema: typescript_coerce::ZodJsonSchema::Unknown,
                                },
                            ]);
                            assert!(typescript_coerce::zod_json(None, &schema).is_err());
                            Err(JsonTransformFailure::Operational)
                        }
                        _ => transform(request, profile == "default"),
                    }
                })
                .unwrap();
            let count = calls.clone();
            router = router.post(
                route,
                move |Extension(parsed): Extension<ParsedJsonBody>| async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Json(report(parsed))
                },
            );
        }
    }
    let auth_count = authentications.clone();
    let app = router
        .get(RouteContract::get("Counters", "Counters", "/counters").with_response(200,None), move || {
            let value=json!({"transforms":transforms.load(Ordering::SeqCst),"auth":authentications.load(Ordering::SeqCst)});
            async move {Json(value)}
        })
        .get(
            RouteContract::get("Calls", "Calls", "/calls").with_response(200, None),
            move || async move { Json(json!({"handlerCalls":calls.load(Ordering::SeqCst)})) },
        )
        .with_config(
            RouterConfig {
                request_validators: validators,
                json_request_parsers: parsers,
                auth_error_format: AuthErrorFormat::TypeScriptText,
                correlation_id_format: CorrelationIdFormat::TypeScript,
                ..RouterConfig::default()
            }
            .with_response_authenticator(move |_, _| {
                auth_count.fetch_add(1, Ordering::SeqCst);
                async {
                Err(ErrorResponse::new(
                    ErrorKind::Unauthorized,
                    "Authorization token required.",
                ))
                }
            }),
        );
    #[cfg(feature = "mcp")]
    let app = app
        .into_router_with_mcp(forklaunch_mcp::server::McpServerOptions::new(
            "composition",
            "1",
        ))
        .unwrap();
    #[cfg(not(feature = "mcp"))]
    let app = app.into_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("READY http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
