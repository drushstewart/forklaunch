//! Contract-backed HMAC relay/claim operations and the raw browser handoff.
use super::AuthHttp;
use crate::{
    relay::{RelayHooks, RelayService, RelayTokens},
    store::AuthStore,
};
use axum::{
    Json,
    extract::Query,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use forklaunch_auth::AuthError;
use forklaunch_axum::{ForkLaunchRouter, module_auth::HmacJwksAuth};
use forklaunch_blueprint_core::contract::{
    AccessLevel, AuthConfig, BodyContract, BodyKind, HmacAuth, RouteContract, SchemaRef,
};
use forklaunch_core::ServiceInfo;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[must_use]
pub fn contracts() -> Vec<RouteContract> {
    let tokens = [
        "access_token",
        "token_type",
        "scope",
        "id_token",
        "refresh_token",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), json!({"type":"string"})))
    .chain([("expires_in".into(), json!({"type":"number"}))])
    .collect::<serde_json::Map<_, _>>();
    vec![
        RouteContract::post("Relay Session Ingest","Accept a relay-exchanged OAuth session and return a one-time handoff path","/relay/session-ingest")
            .with_access(AccessLevel::Internal).with_auth(AuthConfig::Hmac(HmacAuth::new().with_header_name("authorization")))
            .with_request_body(body("RelayIngest",&json!({"type":"object","required":["nonce","tokens"],"properties":{"nonce":{"type":"string"},"tokens":{"type":"object","properties":tokens}}})))
            .with_response(200,Some(body("RelayIngestResult",&json!({"type":"object","required":["redirectPath"],"properties":{"redirectPath":{"type":"string"}}}))))
            .with_response(409,Some(text())),
        RouteContract::post("Mint Claim Token","Mint this product's own claim link for a freshly claimed instance","/claim/mint")
            .with_access(AccessLevel::Internal).with_auth(AuthConfig::Hmac(HmacAuth::new().with_header_name("authorization")))
            .with_request_body(body("ClaimMint",&json!({"type":"object","properties":{"purpose":{"type":"string"},"instanceId":{"type":"string"}}})))
            .with_response(200,Some(body("ClaimLink",&json!({"type":"object","required":["claimUrl"],"properties":{"claimUrl":{"type":"string"},"expiresAt":{"type":"string"}}}))))
            .with_response(400,Some(text())).with_response(501,Some(text())).with_response(409,Some(text())),
    ]
}
fn body(name: &str, schema: &Value) -> BodyContract {
    let mut body = BodyContract::json(name);
    body.schema.json_schema = Some(schema.to_string());
    body
}
fn text() -> BodyContract {
    BodyContract {
        kind: BodyKind::Text,
        schema: SchemaRef {
            name: "String".into(),
            json_schema: Some(json!({"type":"string"}).to_string()),
        },
        content_type: Some("text/plain".into()),
    }
}
/// Set per-instance and operator keys with the TypeScript template's fallbacks.
/// # Errors
/// Rejects an empty default secret or invalid HMAC policies.
pub fn configure_auth(
    auth: HmacJwksAuth,
    default_secret: &[u8],
    instance_id: &str,
    instance_secret: &[u8],
) -> Result<HmacJwksAuth, AuthError> {
    if default_secret.is_empty() {
        return Err(AuthError::InvalidCredentials);
    }
    let routes = contracts();
    let ingest_id = if instance_id.is_empty() {
        "default"
    } else {
        instance_id
    };
    let ingest_secret = if instance_secret.is_empty() {
        default_secret
    } else {
        instance_secret
    };
    let mut claim_keys = vec![("default".into(), default_secret.to_vec())];
    if !instance_secret.is_empty() {
        claim_keys.push(("platform".into(), instance_secret.to_vec()));
    }
    auth.with_hmac_keys_for_route(&routes[0], [(ingest_id.into(), ingest_secret.to_vec())])?
        .with_hmac_keys_for_route(&routes[1], claim_keys)
}
#[derive(Deserialize)]
struct Ingest {
    nonce: String,
    tokens: RelayTokens,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Claim {
    purpose: Option<String>,
    instance_id: Option<String>,
}
impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Add relay/claim contracts and raw handoff; use `configure_auth` for keys.
    /// # Panics
    /// Only missing package-owned metadata can panic.
    #[must_use]
    pub fn application_router_with_relay<H: RelayHooks<S> + 'static>(
        self,
        auth: HmacJwksAuth,
        info: ServiceInfo,
        hooks: Arc<H>,
    ) -> ForkLaunchRouter {
        let service = Arc::new(RelayService::new(
            self.store.clone(),
            hooks,
            self.cookie,
            self.secret.to_vec(),
        ));
        let clock = self.clock.clone();
        let ingest = service.clone();
        let ingest_clock = clock.clone();
        let claim = service.clone();
        let handoff = axum::Router::new().route(
            "/relay/handoff",
            axum::routing::get(
                move |headers: HeaderMap, Query(query): Query<BTreeMap<String, String>>| {
                    let service = service.clone();
                    let now = clock();
                    async move {
                        match query.get("ticket").filter(|ticket| !ticket.is_empty()) {
                            Some(ticket) => match service.redeem_ticket(ticket, now).await {
                                Ok(Some(result)) => redirect(
                                    &result.redirect_to,
                                    result.cookie.as_deref(),
                                    &headers,
                                ),
                                _ => redirect("/", None, &headers),
                            },
                            None => redirect("/", None, &headers),
                        }
                    }
                },
            ),
        );
        let mut routes = contracts().into_iter();
        self.application_router(auth,info)
            .post(routes.next().expect("relay contract"),move |Json(input):Json<Ingest>| {
                let service = ingest.clone(); let now = ingest_clock();
                async move { match service.ingest(&input.nonce,input.tokens,now).await {
                    Ok(result) => Json(result).into_response(),
                    Err(error) if error.code == "relay_replay" => (StatusCode::CONFLICT,"relay nonce already consumed").into_response(),
                    Err(_) => (StatusCode::INTERNAL_SERVER_ERROR,"Unable to complete relay handoff").into_response(),
                } }
            })
            .post(routes.next().expect("claim contract"),move |Json(input):Json<Claim>| {
                let service = claim.clone();
                async move { match service.mint_claim(input.purpose.as_deref(),input.instance_id.as_deref().unwrap_or("")).await {
                    Ok(Some(link)) => {
                        let mut result = json!({"claimUrl":link.claim_url});
                        if let Some(expires) = link.expires_at { result["expiresAt"] = json!(expires.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)); }
                        Json(result).into_response()
                    }
                    Ok(None) => (StatusCode::CONFLICT,"No claim link minted. Either this product is already claimed, or mintAppClaimLink is still a stub (domain/hooks/appClaimHooks.ts).").into_response(),
                    Err(error) if error.code == "relay_claim_purpose" => (StatusCode::BAD_REQUEST,error.message).into_response(),
                    Err(_) => (StatusCode::INTERNAL_SERVER_ERROR,"Unable to mint claim link").into_response(),
                } }
            })
            .merge_internal(handoff,["/relay/handoff"])
    }
}
fn redirect(path: &str, cookie: Option<&str>, headers: &HeaderMap) -> Response {
    let location = encode_location(path);
    let mut response = super::application::redirects::redirect(&location, headers);
    if let Some(cookie) = cookie.and_then(|cookie| cookie.parse().ok()) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}
// Express uses encodeurl, preserving valid escapes while encoding invalid
// sequences and non-URL characters with encodeURI's reserved-character set.
fn encode_location(path: &str) -> String {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    const URI: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b';')
        .remove(b',')
        .remove(b'/')
        .remove(b'?')
        .remove(b':')
        .remove(b'@')
        .remove(b'&')
        .remove(b'=')
        .remove(b'+')
        .remove(b'$')
        .remove(b'#')
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    static INVALID: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let invalid = INVALID.get_or_init(|| regex::Regex::new(r"(?:[^\x21\x23-\x3B\x3D\x3F-\x5F\x61-\x7A\x7C\x7E]|%(?:[^0-9A-Fa-f]|[0-9A-Fa-f][^0-9A-Fa-f]|$))+").expect("fixed URL encoding pattern"));
    invalid
        .replace_all(path, |capture: &regex::Captures<'_>| {
            utf8_percent_encode(&capture[0], URI).to_string()
        })
        .into_owned()
}

#[cfg(test)]
mod tests;
