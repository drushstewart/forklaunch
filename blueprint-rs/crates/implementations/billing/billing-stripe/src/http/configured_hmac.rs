//! Actual configured billing HMAC header semantics. Legacy authentication remains separate.
//!
//! Signed path: the full wire path (`POST /payment-link`), matching the framework
//! verifier, every other module and the generated Rust/TypeScript SDK signers.
//! The Express router-relative path (`/` for `POST /payment-link`) that older
//! TypeScript callers signed is deprecated but still accepted by default so they
//! keep working. Turn it off with `BillingBackend::with_mount_relative_hmac_paths(false)`
//! or `HMAC_ACCEPT_MOUNT_RELATIVE_PATHS=false`, because it is not bound to a mount.
use forklaunch_axum::{
    module_auth::{HmacJwksAuth, HmacPayloadError},
    session::AuthenticatedSession,
};
use forklaunch_common::javascript_date::parse_host;
use forklaunch_core::contract::{
    AuthConfig, BodyContract, BodyKind, ResponseContract, RouteContract, SchemaRef,
};
use forklaunch_core::http::{Body, ErrorKind, ErrorResponse, FrameworkRequest};

fn mount(route: &RouteContract) -> Option<&'static str> {
    if !matches!(route.auth, Some(AuthConfig::Hmac(_))) {
        return None;
    }
    [
        "/payment-link",
        "/checkout-session",
        "/subscription",
        "/plan",
    ]
    .into_iter()
    .find(|prefix| route.path == *prefix || route.path.starts_with(&format!("{prefix}/")))
}
pub(super) fn selected(route: &RouteContract) -> bool {
    mount(route).is_some()
}

/// Declare the configured internal routes' plain-text authentication failures.
/// Portal JWT and bound capability routes retain their original declarations.
pub fn project_configured_billing_hmac(route: &mut RouteContract) {
    if !selected(route) {
        return;
    }
    for status in [401, 403] {
        let body = BodyContract {
            kind: BodyKind::Text,
            schema: SchemaRef::named("String").with_json_schema(r#"{"type":"string"}"#),
            content_type: Some("text/plain".into()),
        };
        if let Some(response) = route
            .responses
            .iter_mut()
            .find(|response| response.status_code == status)
        {
            response.body = Some(body);
        } else {
            route
                .responses
                .push(ResponseContract::new(status, Some(body)));
        }
    }
}
fn format_error() -> ErrorResponse {
    ErrorResponse::new(
        ErrorKind::Unauthorized,
        "Invalid Authorization token format.",
    )
}
fn internal(request: &FrameworkRequest) -> ErrorResponse {
    let response = forklaunch_core::http::typescript_parser_error_response(
        500,
        &request.context.correlation_id,
    );
    ErrorResponse::new(
        ErrorKind::Internal,
        response.body.as_text().unwrap_or("Internal server error"),
    )
}
fn part<'a>(value: &'a str, label: &str) -> Option<&'a str> {
    let (key, value) = value.split_once('=')?;
    (key == label && !value.is_empty()).then_some(value)
}
/// `HMAC_ACCEPT_MOUNT_RELATIVE_PATHS`: `false`/`0` rejects the deprecated
/// router-relative form, `true`/`1` accepts it; unset leaves the default (accept).
pub(super) fn mount_relative_from_env() -> Option<bool> {
    let value = std::env::var("HMAC_ACCEPT_MOUNT_RELATIVE_PATHS").ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "false" | "0" => Some(false),
        "true" | "1" => Some(true),
        _ => None,
    }
}
pub(super) fn authenticate(
    auth: &HmacJwksAuth,
    request: &FrameworkRequest,
    route: &RouteContract,
    accept_mount_relative: bool,
) -> Result<AuthenticatedSession, ErrorResponse> {
    let token = request.headers.get("authorization").ok_or_else(|| {
        ErrorResponse::new(ErrorKind::Unauthorized, "Authorization token required.")
    })?;
    let mut pieces = token.split(' ');
    let prefix = pieces.next().unwrap_or_default();
    let first = pieces.next().ok_or_else(format_error)?;
    if prefix.is_empty() {
        return Err(format_error());
    }
    let rest = [Some(first), pieces.next(), pieces.next(), pieces.next()];
    if prefix != "HMAC" || rest.iter().any(Option::is_none) {
        return Err(ErrorResponse::new(
            ErrorKind::Forbidden,
            "Invalid Authorization token.",
        ));
    }
    let values = rest
        .into_iter()
        .zip(["keyId", "ts", "nonce", "signature"])
        .map(|(value, name)| part(value.unwrap_or_default(), name).ok_or_else(format_error))
        .collect::<Result<Vec<_>, _>>()?;
    let millis = parse_host(values[1]).map_err(|_| internal(request))?;
    let timestamp = iso_timestamp(millis);
    let wire_path = request.raw_path.as_deref().unwrap_or(&request.path);
    let body = match &request.body {
        Body::Empty => "undefined".into(),
        Body::Text(value) => format!("{value}\n"),
        Body::Binary(value) => format!("{}\n", String::from_utf8_lossy(value)),
    };
    let payload = |path: &str| {
        format!(
            "{}\n{path}\n{body}{timestamp}\n{}",
            request.method.as_str(),
            values[2]
        )
    };
    // The signed path is the full wire path, as every other module and both
    // generated SDK signers use. The deprecated Express router-relative form is a fallback.
    let mut candidates = vec![wire_path];
    if accept_mount_relative
        && let Some(relative) = mount_relative(wire_path, mount(route).unwrap_or_default())
    {
        candidates.push(relative);
    }
    let mut result = Err(HmacPayloadError::InvalidSignature);
    for path in candidates {
        result = auth.verify_hmac_payload_for_route(
            route,
            values[0],
            payload(path).as_bytes(),
            values[3],
        );
        if !matches!(result, Err(HmacPayloadError::InvalidSignature)) {
            break;
        }
    }
    result.map_err(|error| match error {
        HmacPayloadError::InvalidSignature => {
            ErrorResponse::new(ErrorKind::Forbidden, "Invalid Authorization signature.")
        }
        HmacPayloadError::NotHmac | HmacPayloadError::UnknownKey => internal(request),
    })
}
/// The Express router-relative form of a mounted path (`/payment-link/x` → `/x`).
fn mount_relative<'a>(wire_path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty()
        || !wire_path
            .get(..prefix.len())
            .is_some_and(|part| part.eq_ignore_ascii_case(prefix))
    {
        return None;
    }
    let path = &wire_path[prefix.len()..];
    Some(if path.is_empty() { "/" } else { path })
}
// Civil-date conversion over the complete JavaScript TimeClip range, including
// signed extended years, without chrono's narrower representable year range.
fn iso_timestamp(millis: i64) -> String {
    let days = millis.div_euclid(86_400_000);
    let within = millis.rem_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{year:+07}")
    };
    format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        within / 3_600_000,
        within / 60_000 % 60,
        within / 1000 % 60,
        within % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use forklaunch_core::auth::jwt::{RemoteJwks, RemoteJwksConfig};
    use forklaunch_core::http::RequestContext;
    use jsonwebtoken::{Algorithm, Validation};
    const KEY: &[u8] = b"owned-hmac-secret";
    fn auth() -> HmacJwksAuth {
        HmacJwksAuth::new(
            KEY.to_vec(),
            RemoteJwks::new(RemoteJwksConfig::new("http://127.0.0.1:9/jwks")).unwrap(),
            Validation::new(Algorithm::RS256),
        )
        .unwrap()
    }
    fn route() -> RouteContract {
        forklaunch_billing_interface::module_contracts::module_registry(true)
            .routes()
            .iter()
            .find(|route| route.operation_name == "updatePaymentLink")
            .unwrap()
            .clone()
    }
    fn request(header: Option<String>) -> FrameworkRequest {
        let mut request = FrameworkRequest::new(
            route().method,
            "/payment-link/owned",
            RequestContext::new("hmac-test"),
        )
        .with_body(Body::Text("{\"amount\":\"12\"}".into()));
        if let Some(header) = header {
            request = request.with_header("authorization", header);
        }
        request
    }
    fn header(stamp: &str, canonical: &str) -> String {
        let payload = format!("PUT\n/owned\n{{\"amount\":\"12\"}}\n{canonical}\nrepeat");
        format!(
            "HMAC keyId=default ts={stamp} nonce=repeat signature={}",
            forklaunch_core::auth::hmac_sign(KEY, payload.as_bytes())
        )
    }
    #[test]
    fn configured_header_errors_key_policy_and_clock_semantics() {
        let auth = auth();
        let route = route();
        let valid = header("2026-10-15T07:23:45.678Z", "2026-10-15T07:23:45.678Z");
        for (value, status) in [
            (None, 401),
            (Some(String::new()), 401),
            (Some("HMAC".into()), 401),
            (Some("HMAC keyId=default".into()), 403),
            (Some(valid.replace("HMAC", "OTHER")), 403),
            (Some(valid.replace("keyId=", "key=")), 401),
            (Some(valid.replace("keyId=default", "keyId=other")), 500),
            (
                Some(valid.replace("ts=2026-10-15T07:23:45.678Z", "ts=not-a-date")),
                500,
            ),
            (Some(valid.replace("signature=", "signature=bad")), 403),
        ] {
            assert_eq!(
                authenticate(&auth, &request(value), &route, true)
                    .unwrap_err()
                    .kind
                    .status_code(),
                status
            );
        }
        for stamp in [
            "1900-01-01T00:00:00.000Z",
            "2100-01-01T00:00:00.000Z",
            "-271821-04-20T00:00:00.000Z",
            "+275760-09-13T00:00:00.000Z",
        ] {
            let value = header(stamp, stamp);
            assert!(authenticate(&auth, &request(Some(value.clone())), &route, true).is_ok());
            assert!(
                authenticate(
                    &auth,
                    &request(Some(format!("{value} ignored=extra"))),
                    &route,
                    true
                )
                .is_ok()
            );
        }
        for (stamp, canonical) in [
            ("2026-10-15", "2026-10-15T00:00:00.000Z"),
            ("2026-10-15T07:23:45.6Z", "2026-10-15T07:23:45.600Z"),
            ("2026-10-15T07:23:45.678999Z", "2026-10-15T07:23:45.678Z"),
            ("2026-10-15T07:23:45.678+02:00", "2026-10-15T05:23:45.678Z"),
        ] {
            assert!(
                authenticate(
                    &auth,
                    &request(Some(header(stamp, canonical))),
                    &route,
                    true
                )
                .is_ok(),
                "{stamp}"
            );
        }
        let private = auth
            .with_hmac_keys_for_route(&route, [("other".into(), b"different".to_vec())])
            .unwrap();
        let payload = b"PUT\n/owned\n{\"amount\":\"12\"}\n2026-10-15T07:23:45.678Z\nrepeat";
        let private_header = format!(
            "HMAC keyId=other ts=2026-10-15T07:23:45.678Z nonce=repeat signature={}",
            forklaunch_core::auth::hmac_sign(b"different", payload)
        );
        assert!(
            authenticate(
                &private,
                &request(Some(private_header.clone())),
                &route,
                true
            )
            .is_ok()
        );
        let mut other = route.clone();
        other.path = "/payment-link/another".into();
        assert_eq!(
            authenticate(&private, &request(Some(private_header)), &other, true)
                .unwrap_err()
                .kind,
            ErrorKind::Internal
        );
        assert_eq!(
            authenticate(&private, &request(Some(valid)), &route, true)
                .unwrap_err()
                .kind,
            ErrorKind::Internal
        );
    }
    #[test]
    fn full_wire_path_is_canonical_and_router_relative_is_opt_in() {
        let auth = auth();
        let route = route();
        let stamp = "2026-10-15T07:23:45.678Z";
        let sign = |path: &str| {
            let payload = format!("PUT\n{path}\n{{\"amount\":\"12\"}}\n{stamp}\nfull");
            format!(
                "HMAC keyId=default ts={stamp} nonce=full signature={}",
                forklaunch_core::auth::hmac_sign(KEY, payload.as_bytes())
            )
        };
        for (path, default, opted_in) in [
            ("/payment-link/owned", true, true),
            // Deprecated router-relative form: rejected unless explicitly enabled.
            ("/owned", false, true),
            ("/plan/owned", false, false),
            ("/payment-link/other", false, false),
            ("/", false, false),
        ] {
            for (relative, accepted) in [(false, default), (true, opted_in)] {
                let result = authenticate(&auth, &request(Some(sign(path))), &route, relative);
                assert_eq!(result.is_ok(), accepted, "{path} relative={relative}");
                if !accepted {
                    assert_eq!(result.unwrap_err().kind, ErrorKind::Forbidden);
                }
            }
        }
        // The SDK signer's header verifies unchanged.
        let header = forklaunch_core::auth::request_hmac::generate_hmac_auth_header(
            KEY,
            "default",
            "PUT",
            "/payment-link/owned",
            forklaunch_core::auth::request_hmac::SignedBody::Text("{\"amount\":\"12\"}"),
            stamp,
            "sdk",
        )
        .unwrap();
        assert!(authenticate(&auth, &request(Some(header)), &route, false).is_ok());
        assert_eq!(mount_relative("/payment-link", "/payment-link"), Some("/"));
        assert_eq!(mount_relative("/PLAN/x", "/plan"), Some("/x"));
        assert_eq!(mount_relative("/plan", "/payment-link"), None);
    }
    #[test]
    fn full_time_clip_iso_and_selected_metadata() {
        for (value, expected) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (-1, "1969-12-31T23:59:59.999Z"),
            (-8_640_000_000_000_000, "-271821-04-20T00:00:00.000Z"),
            (8_640_000_000_000_000, "+275760-09-13T00:00:00.000Z"),
        ] {
            assert_eq!(iso_timestamp(value), expected);
        }
        let registry = forklaunch_billing_interface::module_contracts::module_registry(true);
        let mut count = 0;
        for original in registry.routes() {
            let mut projected = original.clone();
            project_configured_billing_hmac(&mut projected);
            if selected(original) {
                count += 1;
                for status in [401, 403] {
                    assert_eq!(
                        projected
                            .responses
                            .iter()
                            .find(|r| r.status_code == status)
                            .unwrap()
                            .body
                            .as_ref()
                            .unwrap()
                            .kind,
                        BodyKind::Text
                    );
                }
            } else {
                assert_eq!(original, &projected);
            }
        }
        assert!(count > 10);
    }
    #[tokio::test]
    async fn per_route_text_override_preserves_unselected_json_and_raw_path() {
        use forklaunch_axum::{AuthErrorFormat, ForkLaunchRouter, RouterConfig};
        use forklaunch_core::contract::HmacAuth;
        use std::sync::Arc;
        let selected_route = RouteContract::get("Selected", "Selected", "/payment-link/:id")
            .with_auth(AuthConfig::Hmac(HmacAuth::new()))
            .with_response(200, None);
        let other_route = RouteContract::get("Bound", "Bound", "/bound-payment-links/:id")
            .with_auth(AuthConfig::Hmac(HmacAuth::new()))
            .with_response(200, None);
        let verifier = Arc::new(auth());
        let config = RouterConfig::default()
            .with_optional_trailing_slash_for_path(&selected_route.openapi_path)
            .with_auth_error_format_for_route(&selected_route, AuthErrorFormat::TypeScriptText)
            .with_response_authenticator(move |request, route| {
                let verifier = Arc::clone(&verifier);
                async move {
                    if selected(&route) {
                        authenticate(&verifier, &request, &route, true)
                    } else {
                        verifier
                            .authenticate(request, route)
                            .await
                            .map_err(|e| e.to_error_response())
                    }
                }
            });
        let app = ForkLaunchRouter::anonymous()
            .get(selected_route, || async { "ok" })
            .get(other_route, || async { "ok" })
            .with_config(config)
            .into_router();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = reqwest::Client::new();
        for path in ["/payment-link/owned", "/bound-payment-links/owned"] {
            let response = http.get(format!("{url}{path}")).send().await.unwrap();
            assert_eq!(response.status(), 401);
            let kind = response.headers()["content-type"]
                .to_str()
                .unwrap()
                .to_owned();
            let text = response.text().await.unwrap();
            if path.starts_with("/payment-link/") {
                assert!(kind.starts_with("text/plain"));
                assert_eq!(text, "Authorization token required.");
            } else {
                assert!(kind.starts_with("application/json"));
            }
        }
        for (path, signed) in [
            ("/payment-link/owned?query=1", "/owned"),
            ("/payment-link/abc%2Fdef", "/abc%2Fdef"),
            ("/payment-link/owned/", "/owned/"),
            ("/payment-link/owned?query=1", "/payment-link/owned"),
            ("/payment-link/abc%2Fdef", "/payment-link/abc%2Fdef"),
            ("/payment-link/owned/", "/payment-link/owned/"),
        ] {
            let stamp = "1900-01-01T00:00:00.000Z";
            let payload = format!("GET\n{signed}\nundefined{stamp}\nnonce");
            let header = format!(
                "HMAC keyId=default ts={stamp} nonce=nonce signature={}",
                forklaunch_core::auth::hmac_sign(KEY, payload.as_bytes())
            );
            let response = http
                .get(format!("{url}{path}"))
                .header("authorization", header)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200, "{path}");
        }
        // The admitted alias still authenticates; generic/bound paths stay strict.
        let response = http
            .get(format!("{url}/payment-link/owned/"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        for path in ["/payment-link/owned//", "/bound-payment-links/owned/"] {
            assert_eq!(
                http.get(format!("{url}{path}"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                404
            );
        }
        task.abort();
        let _ = task.await;
    }

    #[test]
    fn raw_path_preserves_signed_trailing_slash() {
        use forklaunch_core::contract::HttpMethod;
        // URI provenance survives normalization without changing the normalized route.
        let mut req = FrameworkRequest::new(
            HttpMethod::Get,
            "/payment-link/owned/",
            RequestContext::new("raw"),
        );
        req.raw_path = Some("/payment-link/owned/".into());
        let stamp = "1900-01-01T00:00:00.000Z";
        let payload = format!("GET\n/owned/\nundefined{stamp}\nnonce");
        req = req.with_header(
            "authorization",
            format!(
                "HMAC keyId=default ts={stamp} nonce=nonce signature={}",
                forklaunch_core::auth::hmac_sign(KEY, payload.as_bytes())
            ),
        );
        let mut route = route();
        route.method = HttpMethod::Get;
        assert!(authenticate(&auth(), &req, &route, true).is_ok());
        assert!(authenticate(&auth(), &req, &route, false).is_err());
    }
}
