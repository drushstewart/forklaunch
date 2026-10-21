use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use fieldnotes_contracts::ErrorBody;
use fieldnotes_security::SecurityError;
use uuid::Uuid;
pub struct ApiError(pub(super) StatusCode, pub(super) &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(ErrorBody {
                error: self.1.into(),
            }),
        )
            .into_response()
    }
}
impl From<SecurityError> for ApiError {
    fn from(error: SecurityError) -> Self {
        match error {
            SecurityError::Unauthorized => {
                Self(StatusCode::UNAUTHORIZED, "Authentication required")
            }
            SecurityError::Forbidden => Self(StatusCode::FORBIDDEN, "Access denied"),
            SecurityError::Conflict => {
                Self(StatusCode::CONFLICT, "Note changed; reload before saving")
            }
            SecurityError::NotFound => Self(StatusCode::NOT_FOUND, "Note not found"),
            SecurityError::InvalidInput => Self(StatusCode::BAD_REQUEST, "Invalid note"),
            SecurityError::Unavailable => Self(
                StatusCode::SERVICE_UNAVAILABLE,
                "Authentication unavailable",
            ),
            SecurityError::Storage | SecurityError::Encryption => {
                Self(StatusCode::INTERNAL_SERVER_ERROR, "Request failed")
            }
        }
    }
}
pub(super) fn invalid() -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, "Invalid request")
}
pub(super) fn id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| invalid())
}

impl From<ApiError> for framework::native::EndpointError {
    fn from(error: ApiError) -> Self {
        match error.0 {
            StatusCode::BAD_REQUEST => Self::InvalidRequest,
            StatusCode::UNAUTHORIZED => Self::Unauthorized,
            StatusCode::FORBIDDEN => Self::Forbidden,
            StatusCode::NOT_FOUND => Self::NotFound,
            StatusCode::CONFLICT => Self::Conflict,
            StatusCode::PAYLOAD_TOO_LARGE => Self::PayloadTooLarge,
            StatusCode::SERVICE_UNAVAILABLE => Self::Unavailable,
            _ => Self::InvalidResponse,
        }
    }
}
