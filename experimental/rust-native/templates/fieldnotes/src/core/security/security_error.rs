//! Sanitized application security and persistence errors.

#[derive(Debug)]
pub enum SecurityError {
    Unauthorized,
    Forbidden,
    NotFound,
    InvalidInput,
    Conflict,
    Unavailable,
    Storage,
    Encryption,
}
impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not found",
            Self::InvalidInput => "invalid input",
            Self::Conflict => "revision conflict",
            Self::Unavailable => "authentication unavailable",
            Self::Storage => "storage failure",
            Self::Encryption => "protected data unavailable",
        })
    }
}
impl std::error::Error for SecurityError {}
impl From<sqlx::Error> for SecurityError {
    fn from(_: sqlx::Error) -> Self {
        Self::Storage
    }
}
