//! Executes production auth HTTP handlers against an owned fixture store/clock.
#[path = "http_parity/fault_store.rs"]
mod fault_store;
#[path = "../src/ecmascript/json.rs"]
mod literal_json;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use chrono::{DateTime, Utc};
use forklaunch_iam_better_auth::{
    accounts::CredentialService,
    http::AuthHttp,
    store::{AuthCursor, AuthPage, AuthStore, AuthTransaction, Model, memory::MemoryAuthStore},
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};
use tower::ServiceExt;

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() {
    let store = Arc::new(MemoryAuthStore::default());
    let clock = Arc::new(AtomicI64::new(0));
    let server_clock = clock.clone();
    let plugin_calls = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let router = AuthHttp::new(
        store.clone(),
        std::env::var("FORKLAUNCH_AUTH_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:3000".into()),
        b"better-auth-protocol-fixture-secret-32-characters".to_vec(),
        vec![],
    )
    .unwrap()
    .with_base_path(
        std::env::var("FORKLAUNCH_AUTH_BASE_PATH").unwrap_or_else(|_| "/api/auth".into()),
    )
    .unwrap()
    .with_clock(move || {
        DateTime::<Utc>::from_timestamp_millis(server_clock.load(Ordering::SeqCst)).unwrap()
    });
    let router = if let Ok(config) = std::env::var("FORKLAUNCH_AUTH_RATE_LIMIT") {
        use forklaunch_iam_better_auth::http::{
            AuthRateLimitDecision, AuthRateLimitOptions, AuthRateLimitRule,
        };
        let config: serde_json::Value = serde_json::from_str(&config).unwrap();
        let rule = |value: &serde_json::Value| {
            AuthRateLimitRule::new(
                u32::try_from(value["window"].as_u64().unwrap()).unwrap(),
                u32::try_from(value["max"].as_u64().unwrap()).unwrap(),
            )
            .unwrap()
        };
        let mut options = AuthRateLimitOptions::default().with_default(rule(&config));
        if let Some(plugins) = config["pluginRules"].as_array() {
            for (index, plugin) in plugins.iter().enumerate() {
                let prefix = plugin["prefix"].as_str().unwrap().to_owned();
                let calls = plugin_calls.clone();
                options = options.with_plugin_rule(rule(plugin), move |path| {
                    calls
                        .lock()
                        .unwrap()
                        .push(json!({ "index": index, "path": path }));
                    path.starts_with(&prefix)
                });
            }
        }
        for (path, value) in config["customRules"].as_object().unwrap() {
            if value["callback"] == true {
                options = options
                    .with_dynamic_path(path, |request, baseline| async move {
                        tokio::task::yield_now().await;
                        Ok(
                            match request
                                .headers
                                .get("x-rate-mode")
                                .and_then(|value| value.to_str().ok())
                            {
                                Some("disabled") => AuthRateLimitDecision::Disable,
                                Some("inherit") => AuthRateLimitDecision::UseDefault,
                                _ => AuthRateLimitDecision::Limit(AuthRateLimitRule::new(
                                    baseline.window_seconds() + 2,
                                    baseline.max_requests().saturating_sub(1).max(1),
                                )?),
                            },
                        )
                    })
                    .unwrap();
                continue;
            }
            options = options
                .with_path_pattern(
                    path,
                    if value == &serde_json::Value::Bool(false) {
                        None
                    } else {
                        Some(rule(value))
                    },
                )
                .unwrap();
        }
        router.with_rate_limit_options(options)
    } else {
        router
    };
    let router = if let Ok(endpoint) = std::env::var("FORKLAUNCH_GOOGLE_CERTIFICATES_URL") {
        let provider = forklaunch_iam_better_auth::oauth::google_client::GoogleClient::new(
            "fixture-google-id".into(),
            "fixture-google-secret".into(),
        )
        .unwrap()
        .with_certificate_endpoint(&endpoint)
        .unwrap();
        let provider = if let Ok(endpoint) = std::env::var("FORKLAUNCH_GOOGLE_TOKEN_URL") {
            provider.with_token_endpoint(&endpoint).unwrap()
        } else {
            provider
        };
        router.with_google(provider)
    } else {
        router
    };
    #[cfg(feature = "application-http")]
    let router = if std::env::var_os("FORKLAUNCH_AUTH_POPUP_HELPERS").is_some() {
        router.router_with_redirects()
    } else {
        router.router()
    };
    #[cfg(not(feature = "application-http"))]
    let router = router.router();
    for line in io::stdin().lock().lines() {
        let input: Value = literal_json::parse(&line.unwrap()).unwrap();
        clock.store(input["now"].as_i64().unwrap(), Ordering::SeqCst);
        let result = match input["op"].as_str().unwrap() {
            "ratePluginCalls" => json!(*plugin_calls.lock().unwrap()),
            "seed" | "replace-seed" => seed(&store, &input).await,
            "records" => records(&store, &input).await,
            "seed-keys" => {
                let mut tx = store.begin(true).await.unwrap();
                for row in input["keys"].as_array().unwrap() {
                    tx.insert(Model::Jwks, row.clone()).await.unwrap();
                }
                tx.commit().await.unwrap();
                json!({"ok":true})
            }
            "compliance-export" | "compliance-erase" => {
                if let Some(model) = input["failComplianceModel"].as_str() {
                    let model = match model {
                        "Account" => Model::Account,
                        "Jwks" => Model::Jwks,
                        _ => panic!("invalid fixture model"),
                    };
                    compliance_operation(
                        Arc::new(fault_store::FaultStore {
                            store: store.clone(),
                            read: Some(model),
                            delete: None,
                        }),
                        &input,
                    )
                    .await
                } else {
                    compliance_operation(store.clone(), &input).await
                }
            }
            "retention-cutoff" => {
                let now =
                    DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap();
                json!(
                    forklaunch_iam_better_auth::retention::account_cutoff(now)
                        .unwrap()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                )
            }
            "retention" => {
                let now =
                    DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap();
                let options = forklaunch_iam_better_auth::retention::RetentionOptions {
                    dry_run: input["dryRun"].as_bool().unwrap_or(false),
                    batch_size: input["batchSize"]
                        .as_u64()
                        .map_or(1000, |value| usize::try_from(value).unwrap()),
                    entities: input
                        .get("entities")
                        .map(|value| serde_json::from_value(value.clone()).unwrap()),
                };
                serde_json::to_value(
                    forklaunch_iam_better_auth::retention::RetentionService::new(store.clone())
                        .enforce(now, &options)
                        .await,
                )
                .unwrap()
            }
            "surface" => {
                let service =
                    forklaunch_iam_better_auth::surfacing::SurfacingService::new(store.clone());
                let now =
                    DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap();
                let user = input["userId"].as_str().unwrap();
                json!({"role": service.surface_role(user, now).await.unwrap(), "permissions": service.surface_permissions(user, now).await.unwrap()})
            }
            "set" => set_password(store.clone(), &input).await,
            "addMember" => add_member(store.clone(), &input).await,
            "request" => {
                let mut request = Request::builder()
                    .method(input["method"].as_str().unwrap())
                    .uri(input["path"].as_str().unwrap())
                    .header("host", "localhost:3000");
                for (name, value) in input["headers"].as_object().unwrap() {
                    request = request.header(name, value.as_str().unwrap());
                }
                let request = request
                    .body(Body::from(input["body"].as_str().unwrap_or("").to_owned()))
                    .unwrap();
                let response = router.clone().oneshot(request).await.unwrap();
                let status = response.status().as_u16();
                let headers: serde_json::Map<String, Value> = response
                    .headers()
                    .keys()
                    .map(|name| {
                        (
                            name.to_string(),
                            json!(
                                response
                                    .headers()
                                    .get_all(name)
                                    .iter()
                                    .map(|value| value
                                        .as_bytes()
                                        .iter()
                                        .copied()
                                        .map(char::from)
                                        .collect::<String>())
                                    .collect::<Vec<_>>()
                            ),
                        )
                    })
                    .collect();
                let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
                let body: Value = literal_json::parse(std::str::from_utf8(&bytes).unwrap_or(""))
                    .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
                json!({"status":status,"headers":headers,"body":body})
            }
            operation => panic!("unknown operation {operation}"),
        };
        println!("{result}");
    }
}

async fn add_member(store: Arc<MemoryAuthStore>, input: &Value) -> Value {
    use forklaunch_iam_better_auth::organizations::{AddMemberInput, OrganizationService};
    match OrganizationService::new(store)
        .add_member(
            input["token"].as_str(),
            input["transient"].as_bool().unwrap_or(false),
            serde_json::from_value::<AddMemberInput>(input["body"].clone()).unwrap(),
            DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap(),
        )
        .await
    {
        Ok(body) => json!({"body":body}),
        Err(error) => json!({"error":{"code":error.code,"message":error.message}}),
    }
}

async fn set_password(store: Arc<MemoryAuthStore>, input: &Value) -> Value {
    match CredentialService::new(store.clone())
        .set_password(
            input["token"].as_str().unwrap(),
            input["password"].as_str().unwrap().to_owned(),
            input["transient"].as_bool().unwrap_or(false),
            DateTime::<Utc>::from_timestamp_millis(input["now"].as_i64().unwrap()).unwrap(),
        )
        .await
    {
        Ok(body) => json!({"body":body}),
        Err(error) => json!({"error":{"code":error.code,"message":error.message}}),
    }
}

const MODELS: &[(&str, Model)] = &[
    ("users", Model::User),
    ("sessions", Model::Session),
    ("accounts", Model::Account),
    ("verifications", Model::Verification),
    ("organizations", Model::Organization),
    ("members", Model::Member),
    ("teams", Model::Team),
    ("teamMembers", Model::TeamMember),
    ("invitations", Model::Invitation),
    ("organizationRoles", Model::OrganizationRole),
];
async fn seed(store: &MemoryAuthStore, input: &Value) -> Value {
    let mut tx = store.begin(true).await.unwrap();
    if input["op"] == "replace-seed" {
        for (_, model) in MODELS {
            for row in tx.list(*model).await.unwrap() {
                tx.delete(*model, row["id"].as_str().unwrap())
                    .await
                    .unwrap();
            }
        }
    }
    for (field, model) in MODELS {
        for row in input[field].as_array().into_iter().flatten() {
            tx.insert(*model, row.clone()).await.unwrap();
        }
    }
    tx.commit().await.unwrap();
    json!({"ok":true})
}
async fn records(store: &MemoryAuthStore, input: &Value) -> Value {
    let mut tx = store.begin(false).await.unwrap();
    let mut result = json!({});
    for (field, model) in MODELS {
        if matches!(*model, Model::User | Model::Session | Model::Account)
            || *model == Model::Verification && input["withVerifications"] == true
            || matches!(
                *model,
                Model::Organization
                    | Model::Member
                    | Model::Team
                    | Model::TeamMember
                    | Model::Invitation
                    | Model::OrganizationRole
            ) && input["withOrganizations"] == true
        {
            result[field] = json!(tx.list(*model).await.unwrap());
        }
    }
    tx.commit().await.unwrap();
    result
}

async fn compliance_operation<S: AuthStore>(store: Arc<S>, input: &Value) -> Value {
    let mut service = forklaunch_iam_better_auth::compliance::ComplianceService::new(store);
    if let Some(label) = input["tenantLabel"].as_str() {
        service = service.with_tenant_label(label.to_owned());
    }
    if input["op"] == "compliance-export" {
        serde_json::to_value(
            service
                .export(input["userId"].as_str().unwrap())
                .await
                .unwrap(),
        )
        .unwrap()
    } else {
        serde_json::to_value(
            service
                .erase(input["userId"].as_str().unwrap())
                .await
                .unwrap(),
        )
        .unwrap()
    }
}
