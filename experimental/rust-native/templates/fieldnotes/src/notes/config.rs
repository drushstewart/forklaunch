//! Required startup inputs, kept separate from dependency construction.
pub struct Config {
    pub database_url: String,
    pub auth_url: String,
    pub internal_secret: String,
    pub public_origin: String,
    pub listen_addr: String,
}
impl Config {
    pub fn from_env() -> Result<Self, std::env::VarError> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL")?,
            auth_url: std::env::var("AUTH_INTERNAL_URL")?,
            internal_secret: std::env::var("INTERNAL_AUTH_SECRET")?,
            public_origin: std::env::var("PUBLIC_ORIGIN")?,
            listen_addr: std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8000".into()),
        })
    }
}
