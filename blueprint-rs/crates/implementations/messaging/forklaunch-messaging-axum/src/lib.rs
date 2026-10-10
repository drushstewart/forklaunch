//! Durable messaging module handlers with request-bound HMAC, verified JWT
//! compliance access and authenticated Twilio form callbacks.
#![forbid(unsafe_code)]
use axum::{
    Extension, Json,
    extract::{Form, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use forklaunch_auth::{AuthError, Principal};
use forklaunch_axum::{
    ForkLaunchRouter, RouterConfig, module_auth::HmacJwksAuth, session::AuthenticatedSession,
};
use forklaunch_blueprint_core::{AuditContext, BlueprintError, BlueprintResult, http::Body};
use forklaunch_core::ServiceInfo;
use forklaunch_messaging_base::{BaseSmsService, postgres::PostgresSmsStore};
use forklaunch_messaging_interface::{
    SendSmsDto, SmsErasureDto, SmsExportDto, SmsPiiRecordDto, SmsRecordDto, SmsService, SmsStore,
    contracts,
};
use forklaunch_messaging_twilio::TwilioSmsService;
use std::sync::Arc;

pub enum MessagingBackend {
    Base(PostgresSmsStore),
    Twilio(TwilioSmsService<PostgresSmsStore>),
}
impl MessagingBackend {
    #[must_use]
    pub const fn is_twilio(&self) -> bool {
        matches!(self, Self::Twilio(_))
    }
    fn store(&self) -> &PostgresSmsStore {
        match self {
            Self::Base(store) => store,
            Self::Twilio(provider) => provider.store(),
        }
    }
    fn for_session(&self, session: &AuthenticatedSession) -> BlueprintResult<PostgresSmsStore> {
        self.store().with_audit_context(AuditContext::actor(
            &session.principal.subject,
            uuid::Uuid::new_v4().to_string(),
        ))
    }
    async fn send(
        &self,
        request: SendSmsDto,
        session: &AuthenticatedSession,
    ) -> BlueprintResult<SmsRecordDto> {
        let store = self.for_session(session)?;
        match self {
            Self::Base(_) => BaseSmsService::new(store).send_sms(request).await,
            Self::Twilio(provider) => provider.with_store(store).send_sms(request).await,
        }
    }
}
fn failure(error: &BlueprintError) -> Response {
    if error.code == "sms_not_found" {
        return (StatusCode::NOT_FOUND, "SMS record not found").into_response();
    }
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Messaging operation failed",
    )
        .into_response()
}
fn response(result: BlueprintResult<SmsRecordDto>) -> Response {
    match result {
        Ok(record) => Json(record).into_response(),
        Err(error) => failure(&error),
    }
}

/// Builds runtime routes from the same registry used for contracts and SDKs.
/// Database tenant and provider credentials are owned by `backend`, never by
/// HTTP fields or unverified claims. Each plaintext read audits a verified actor.
/// # Panics
/// Panics if package-owned route names and handlers disagree.
#[must_use]
pub fn messaging_router(
    backend: MessagingBackend,
    auth: HmacJwksAuth,
    service: ServiceInfo,
) -> ForkLaunchRouter {
    let registry = contracts::module_registry(backend.is_twilio());
    let route = |operation: &str| {
        registry
            .routes()
            .iter()
            .find(|route| route.operation_name == operation)
            .expect("registered messaging route")
            .clone()
    };
    let backend = Arc::new(backend);
    let config = router_config(Arc::clone(&backend), auth);
    let send = Arc::clone(&backend);
    let get = Arc::clone(&backend);
    let export = Arc::clone(&backend);
    let erase = Arc::clone(&backend);
    let service_name = service.name.clone();
    let mut router = ForkLaunchRouter::new(service)
        .post(
            route("sendSms"),
            move |Extension(session): Extension<AuthenticatedSession>,
                  Json(request): Json<SendSmsDto>| {
                let backend = Arc::clone(&send);
                async move { response(backend.send(request, &session).await) }
            },
        )
        .get(
            route("getSmsRecord"),
            move |Extension(session): Extension<AuthenticatedSession>, Path(id): Path<String>| {
                let backend = Arc::clone(&get);
                async move {
                    match backend.for_session(&session) {
                        Ok(store) => response(store.get(&id).await),
                        Err(error) => failure(&error),
                    }
                }
            },
        )
        .get(
            route("exportUserData"),
            move |Extension(session): Extension<AuthenticatedSession>,
                  Path(subject): Path<String>| {
                let backend = Arc::clone(&export);
                async move { export_subject(&backend, &session, subject).await }
            },
        )
        .delete(
            route("eraseUserData"),
            move |Extension(session): Extension<AuthenticatedSession>,
                  Path(subject): Path<String>| {
                let backend = Arc::clone(&erase);
                async move { erase_subject(&backend, &session, &subject).await }
            },
        )
        .get(route("getHealth"), move || {
            let service = service_name.clone();
            async move { Json(serde_json::json!({"service":service,"status":"ok"})) }
        });
    if backend.is_twilio() {
        router = router.post(
            route("handleStatusCallback"),
            move |Extension(session): Extension<AuthenticatedSession>,
                  Form(params): Form<Vec<(String, String)>>| {
                let backend = Arc::clone(&backend);
                async move { callback(backend, session, params).await }
            },
        );
    }
    router.with_config(config)
}
async fn callback(
    backend: Arc<MessagingBackend>,
    session: AuthenticatedSession,
    params: Vec<(String, String)>,
) -> Response {
    let MessagingBackend::Twilio(provider) = &*backend else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let field = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let Some(message_sid) = field("MessageSid") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(message_status) = field("MessageStatus") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let store = match backend.for_session(&session) {
        Ok(store) => store,
        Err(error) => return failure(&error),
    };
    match provider
        .with_store(store)
        .process_status_callback(forklaunch_messaging_twilio::TwilioStatusCallbackDto {
            message_sid,
            message_status,
            error_message: field("ErrorMessage"),
        })
        .await
    {
        Ok(_) => (StatusCode::OK, "Status callback processed").into_response(),
        Err(error) => failure(&error),
    }
}
fn router_config(backend: Arc<MessagingBackend>, auth: HmacJwksAuth) -> RouterConfig {
    let verifier = backend;
    let auth = Arc::new(auth);
    RouterConfig::default().with_authenticator(move |request, contract| {
        let auth = Arc::clone(&auth);
        let backend = Arc::clone(&verifier);
        async move {
            if contract.operation_name == "handleStatusCallback" {
                let MessagingBackend::Twilio(provider) = &*backend else {
                    return Err(AuthError::InvalidCredentials);
                };
                let Body::Text(body) = &request.body else {
                    return Err(AuthError::InvalidCredentials);
                };
                let params: Vec<_> = form_urlencoded::parse(body.as_bytes())
                    .into_owned()
                    .collect();
                provider
                    .verify_callback(
                        &params,
                        request
                            .headers
                            .get("x-twilio-signature")
                            .ok_or(AuthError::MissingCredentials)?,
                    )
                    .map_err(|_| AuthError::InvalidCredentials)?;
                return Ok(AuthenticatedSession::new(
                    Principal::new("provider:twilio"),
                    serde_json::json!({"sub":"provider:twilio"}),
                ));
            }
            auth.authenticate(request, contract).await
        }
    })
}
async fn export_subject(
    backend: &MessagingBackend,
    session: &AuthenticatedSession,
    subject: String,
) -> Response {
    let store = match backend.for_session(session) {
        Ok(store) => store,
        Err(error) => return failure(&error),
    };
    match store.export_subject(&subject).await {
        Ok(records) if records.is_empty() => (
            StatusCode::NOT_FOUND,
            "User not found or no PII data to export",
        )
            .into_response(),
        Ok(records) => Json(SmsExportDto {
            user_id: subject,
            entities: std::collections::BTreeMap::from([(
                "SmsRecord".into(),
                records
                    .into_iter()
                    .map(|r| SmsPiiRecordDto {
                        id: r.id,
                        to: r.to,
                        body: r.body,
                    })
                    .collect(),
            )]),
            failures: vec![],
        })
        .into_response(),
        Err(error) => failure(&error),
    }
}
async fn erase_subject(
    backend: &MessagingBackend,
    session: &AuthenticatedSession,
    subject: &str,
) -> Response {
    let store = match backend.for_session(session) {
        Ok(store) => store,
        Err(error) => return failure(&error),
    };
    match store.erase_subject(subject).await {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            "User not found or no PII data to erase",
        )
            .into_response(),
        Ok(count) => Json(SmsErasureDto {
            entities_affected: vec!["SmsRecord".into()],
            records_deleted: count,
            failures: vec![],
        })
        .into_response(),
        Err(error) => failure(&error),
    }
}
