use crate::error::{AppError, Result};
use std::env;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub public_origin: Option<String>,
    pub docs_path: String,
    pub docs_server_urls: Vec<String>,
    pub docs_server_descriptions: Vec<String>,
    pub database_url: String,
}

impl ServiceConfig {
    #[must_use]
    pub fn from_env() -> Self {
        let name = env::var("SERVICE_NAME").unwrap_or_else(|_| "{{{name}}}".to_string());
        let protocol = env::var("PROTOCOL").unwrap_or_else(|_| "http".to_string());
        let host = env::var("SERVICE_HOST")
            .or_else(|_| env::var("HOST"))
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = env::var("SERVICE_PORT")
            .or_else(|_| env::var("PORT"))
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8080);
        let docs_path = env::var("DOCS_PATH").unwrap_or_else(|_| "/docs".to_string());
        let docs_server_urls = env::var("DOCS_SERVER_URLS")
            .ok()
            .map(|value| csv_values(&value))
            .filter(|values| !values.is_empty())
            .unwrap_or_else(|| vec![format!("{protocol}://{host}:{port}")]);
        let docs_server_descriptions = env::var("DOCS_SERVER_DESCRIPTIONS")
            .ok()
            .map(|value| csv_values(&value))
            .filter(|values| !values.is_empty())
            .unwrap_or_else(|| vec!["Main Server".to_string()]);
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://localhost/forklaunch".to_string());

        Self {
            name,
            protocol,
            host,
            port,
            public_origin: env::var("PUBLIC_ORIGIN").ok(),
            docs_path,
            docs_server_urls,
            docs_server_descriptions,
            database_url,
        }
    }

    #[must_use]
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    #[must_use]
    pub fn origin(&self) -> String {
        self.public_origin.clone().unwrap_or_else(|| {
            format!("{}://{}:{}", self.protocol, self.host, self.port)
        })
    }

    /// Resolve an ephemeral listener port without replacing configured public URLs.
    pub fn set_bound_port(&mut self, port: u16) {
        if self.port == 0 {
            let previous = format!("{}://{}:0", self.protocol, self.host);
            self.port = port;
            for server in &mut self.docs_server_urls {
                if *server == previous {
                    *server = format!("{}://{}:{}", self.protocol, self.host, self.port);
                }
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::InvalidConfig("service name cannot be empty".to_string()));
        }
        if self.host.trim().is_empty() {
            return Err(AppError::InvalidConfig("service host cannot be empty".to_string()));
        }
        if self.protocol.trim().is_empty() {
            return Err(AppError::InvalidConfig(
                "service protocol cannot be empty".to_string(),
            ));
        }
        if !self.docs_path.starts_with('/') {
            return Err(AppError::InvalidConfig(
                "docs path must start with '/'".to_string(),
            ));
        }
        if self.docs_server_urls.iter().any(|url| url.trim().is_empty()) {
            return Err(AppError::InvalidConfig(
                "docs server urls cannot contain empty values".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            name: "{{{name}}}".to_string(),
            protocol: "http".to_string(),
            host: "127.0.0.1".to_string(),
            port: 8080,
            public_origin: None,
            docs_path: "/docs".to_string(),
            docs_server_urls: vec!["http://127.0.0.1:8080".to_string()],
            docs_server_descriptions: vec!["Main Server".to_string()],
            database_url: "postgres://localhost/forklaunch".to_string(),
        }
    }
}

fn csv_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}
