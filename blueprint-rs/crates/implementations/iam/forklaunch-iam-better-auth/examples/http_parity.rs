//! Executes production auth HTTP handlers against an owned fixture store/clock.
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use chrono::{DateTime, Utc};
use forklaunch_iam_better_auth::{
    accounts::CredentialService,
    http::AuthHttp,
    store::{AuthStore, AuthTransaction, Model, memory::MemoryAuthStore},
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
async fn main() {
    let store = Arc::new(MemoryAuthStore::default());
    let clock = Arc::new(AtomicI64::new(0));
    let server_clock = clock.clone();
    let router = AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        b"better-auth-protocol-fixture-secret-32-characters".to_vec(),
        vec![],
    )
    .unwrap()
    .with_clock(move || {
        DateTime::<Utc>::from_timestamp_millis(server_clock.load(Ordering::SeqCst)).unwrap()
    })
    .router();
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        clock.store(input["now"].as_i64().unwrap(), Ordering::SeqCst);
        let result = match input["op"].as_str().unwrap() {
            "seed" => seed(&store, &input).await,
            "records" => records(&store, &input).await,
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
                let body: Value = serde_json::from_slice(&bytes)
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
