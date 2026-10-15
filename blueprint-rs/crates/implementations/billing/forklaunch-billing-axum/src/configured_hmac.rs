//! Actual configured billing HMAC header semantics. Legacy authentication remains separate.
use forklaunch_axum::{
    module_auth::{HmacJwksAuth, HmacPayloadError},
    session::AuthenticatedSession,
};
use forklaunch_blueprint_core::contract::{
    AuthConfig, BodyContract, BodyKind, ResponseContract, RouteContract, SchemaRef,
};
use forklaunch_http::{Body, ErrorKind, ErrorResponse, FrameworkRequest};
use forklaunch_validation::typescript_coerce::date_epoch_millis;
use serde_json::Value;

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
    let response =
        forklaunch_http::typescript_parser_error_response(500, &request.context.correlation_id);
    ErrorResponse::new(
        ErrorKind::Internal,
        response.body.as_text().unwrap_or("Internal server error"),
    )
}
fn part<'a>(value: &'a str, label: &str) -> Option<&'a str> {
    let (key, value) = value.split_once('=')?;
    (key == label && !value.is_empty()).then_some(value)
}
pub(super) fn authenticate(
    auth: &HmacJwksAuth,
    request: &FrameworkRequest,
    route: &RouteContract,
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
    let millis =
        date_epoch_millis(Some(&Value::String(values[1].into()))).map_err(|_| internal(request))?;
    let timestamp = iso_timestamp(millis);
    let wire_path = request.raw_path.as_deref().unwrap_or(&request.path);
    let path = wire_path
        .strip_prefix(mount(route).unwrap_or_default())
        .unwrap_or(wire_path);
    let path = if path.is_empty() { "/" } else { path };
    let body = match &request.body {
        Body::Empty => "undefined".into(),
        Body::Text(value) => format!("{value}\n"),
        Body::Binary(value) => format!("{}\n", String::from_utf8_lossy(value)),
    };
    let payload = format!(
        "{}\n{path}\n{body}{timestamp}\n{}",
        request.method.as_str(),
        values[2]
    );
    auth.verify_hmac_payload_for_route(route, values[0], payload.as_bytes(), values[3])
        .map_err(|error| match error {
            HmacPayloadError::InvalidSignature => {
                ErrorResponse::new(ErrorKind::Forbidden, "Invalid Authorization signature.")
            }
            HmacPayloadError::NotHmac | HmacPayloadError::UnknownKey => internal(request),
        })
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
    use forklaunch_auth::jwt::{RemoteJwks, RemoteJwksConfig};
    use forklaunch_http::RequestContext;
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
            forklaunch_auth::hmac_sign(KEY, payload.as_bytes())
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
                authenticate(&auth, &request(value), &route)
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
            assert!(authenticate(&auth, &request(Some(value.clone())), &route).is_ok());
            assert!(
                authenticate(
                    &auth,
                    &request(Some(format!("{value} ignored=extra"))),
                    &route
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
                authenticate(&auth, &request(Some(header(stamp, canonical))), &route).is_ok(),
                "{stamp}"
            );
        }
        let private = auth
            .with_hmac_keys_for_route(&route, [("other".into(), b"different".to_vec())])
            .unwrap();
        let payload = b"PUT\n/owned\n{\"amount\":\"12\"}\n2026-10-15T07:23:45.678Z\nrepeat";
        let private_header = format!(
            "HMAC keyId=other ts=2026-10-15T07:23:45.678Z nonce=repeat signature={}",
            forklaunch_auth::hmac_sign(b"different", payload)
        );
        assert!(authenticate(&private, &request(Some(private_header.clone())), &route).is_ok());
        let mut other = route.clone();
        other.path = "/payment-link/another".into();
        assert_eq!(
            authenticate(&private, &request(Some(private_header)), &other)
                .unwrap_err()
                .kind,
            ErrorKind::Internal
        );
        assert_eq!(
            authenticate(&private, &request(Some(valid)), &route)
                .unwrap_err()
                .kind,
            ErrorKind::Internal
        );
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
        use forklaunch_blueprint_core::contract::HmacAuth;
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
                        authenticate(&verifier, &request, &route)
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
        ] {
            let stamp = "1900-01-01T00:00:00.000Z";
            let payload = format!("GET\n{signed}\nundefined{stamp}\nnonce");
            let header = format!(
                "HMAC keyId=default ts={stamp} nonce=nonce signature={}",
                forklaunch_auth::hmac_sign(KEY, payload.as_bytes())
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
        use forklaunch_blueprint_core::contract::HttpMethod;
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
                forklaunch_auth::hmac_sign(KEY, payload.as_bytes())
            ),
        );
        let mut route = route();
        route.method = HttpMethod::Get;
        assert!(authenticate(&auth(), &req, &route).is_ok());
    }
}
