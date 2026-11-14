use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use eaeh_core::{ErrorKind, HarnessError};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl From<HarnessError> for ApiError {
    fn from(e: HarnessError) -> Self {
        let status = match e.kind {
            ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            ErrorKind::Unauthenticated => StatusCode::UNAUTHORIZED,
            ErrorKind::Forbidden | ErrorKind::PolicyDenied | ErrorKind::HookBlocked => StatusCode::FORBIDDEN,
            ErrorKind::NotFound => StatusCode::NOT_FOUND,
            ErrorKind::Conflict | ErrorKind::Cancelled | ErrorKind::SupervisionHalt => StatusCode::CONFLICT,
            ErrorKind::ApprovalRequired => StatusCode::PRECONDITION_REQUIRED,
            ErrorKind::AwaitingExternal => StatusCode::ACCEPTED,
            ErrorKind::BudgetExceeded => StatusCode::TOO_MANY_REQUESTS,
            ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorKind::NotSupported => StatusCode::NOT_IMPLEMENTED,
            ErrorKind::Unavailable | ErrorKind::Isolation => StatusCode::SERVICE_UNAVAILABLE,
            ErrorKind::Integrity | ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %e, "internal error");
            "internal error".to_string()
        } else {
            e.message
        };
        Self {
            status,
            code: e.kind.as_str(),
            message,
        }
    }
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        if r.status() == StatusCode::PAYLOAD_TOO_LARGE {
            return ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "request body exceeds the configured limit",
            );
        }
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_input",
            format!("invalid JSON body: {}", r.body_text()),
        )
    }
}

impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> Self {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_input",
            format!("invalid query: {}", r.body_text()),
        )
    }
}

impl From<PathRejection> for ApiError {
    fn from(r: PathRejection) -> Self {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_input",
            format!("invalid path: {}", r.body_text()),
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}
