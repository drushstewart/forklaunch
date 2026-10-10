use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkRequest {
    pub method: &'static str,
    pub url: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
}

fn encode_path_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    encoded
}

fn path_with_params(path: &str, params: &BTreeMap<String, String>) -> String {
    path.split('/')
        .map(|segment| {
            let name = segment.strip_prefix(':').or_else(|| {
                segment.strip_prefix('{').and_then(|name| name.strip_suffix('}'))
            });
            name.and_then(|name| params.get(name))
                .map_or_else(|| segment.to_owned(), |value| encode_path_segment(value))
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Debug)]
pub enum SdkError {
    Transport(reqwest::Error),
}

impl fmt::Display for SdkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for SdkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
        }
    }
}

impl From<reqwest::Error> for SdkError {
    fn from(error: reqwest::Error) -> Self {
        Self::Transport(error)
    }
}

#[derive(Clone)]
pub enum SdkAuth {
    Bearer(String),
    Basic { username: String, password: String },
    HmacHeader { header_name: String, value: String },
}

{{#models}}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct {{name}} {
{{#fields}}
    #[serde(rename = {{{wire_name_literal}}})]
    pub {{field_name}}: {{{field_type}}},
{{/fields}}
}

{{/models}}
{{#aliases}}
pub type {{name}} = {{{target_type}}};

{{/aliases}}
{{#operations}}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct {{request_type}} {
    pub params: BTreeMap<String, String>,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum {{responses_type}} {
{{#responses}}
    {{success_variant}}({{{rust_response_type}}}),
{{/responses}}
    Error { status: u16, message: String },
}

{{/operations}}
#[derive(Clone)]
pub struct {{client_name}} {
    base_url: String,
    http: reqwest::Client,
    default_headers: BTreeMap<String, String>,
    auth: Option<SdkAuth>,
    request_hook: Option<Arc<dyn Fn(&mut SdkRequest) + Send + Sync>>,
}

impl fmt::Debug for {{client_name}} {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("{{client_name}}")
            .field("base_url", &self.base_url)
            .field("default_headers", &self.default_headers)
            .field("has_auth", &self.auth.is_some())
            .field("has_request_hook", &self.request_hook.is_some())
            .finish()
    }
}

impl {{client_name}} {
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_http_client(base_url, reqwest::Client::new())
    }

    #[must_use]
    pub fn with_http_client(base_url: impl Into<String>, http: reqwest::Client) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http,
            default_headers: BTreeMap::new(),
            auth: None,
            request_hook: None,
        }
    }

    #[must_use]
    pub fn with_default_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.default_headers.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.auth = Some(SdkAuth::Bearer(token.into()));
        self
    }

    #[must_use]
    pub fn with_jwt(self, token: impl Into<String>) -> Self {
        self.with_bearer_token(token)
    }

    #[must_use]
    pub fn with_basic_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.auth = Some(SdkAuth::Basic {
            username: username.into(),
            password: password.into(),
        });
        self
    }

    #[must_use]
    pub fn with_hmac_authorization(mut self, value: impl Into<String>) -> Self {
        self.auth = Some(SdkAuth::HmacHeader {
            header_name: "authorization".to_string(),
            value: value.into(),
        });
        self
    }

    #[must_use]
    pub fn with_auth_hook<F>(mut self, hook: F) -> Self
    where
        F: Fn(&mut SdkRequest) + Send + Sync + 'static,
    {
        self.request_hook = Some(Arc::new(hook));
        self
    }

{{#operations}}
    #[must_use]
    pub fn {{method_name}}(&self, input: {{request_type}}) -> SdkRequest {
        let path = path_with_params("{{path}}", &input.params);
        SdkRequest {
            method: "{{http_method}}",
            url: format!("{}{}", self.base_url, path),
            query: input.query,
            headers: input.headers,
            body: input.body,
        }
    }

    pub async fn {{execute_method_name}}(&self, input: {{request_type}}) -> Result<{{responses_type}}, SdkError> {
        let mut request = self.{{method_name}}(input);
        let mut headers = self.default_headers.clone();
        headers.extend(request.headers);
        request.headers = headers;
        if let Some(hook) = &self.request_hook {
            hook(&mut request);
        }
        let method = request
            .method
            .parse::<reqwest::Method>()
            .expect("generated SDK HTTP method is valid");
        let mut builder = self.http.request(method, &request.url);

        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        if let Some(auth) = &self.auth {
            builder = match auth {
                SdkAuth::Bearer(token) => builder.bearer_auth(token),
                SdkAuth::Basic { username, password } => {
                    builder.basic_auth(username.as_str(), Some(password.as_str()))
                }
                SdkAuth::HmacHeader { header_name, value } => {
                    builder.header(header_name.as_str(), value.as_str())
                }
            };
        }
        if let Some(body) = request.body {
            builder = builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = builder.send().await?;
        let status = response.status().as_u16();
        match status {
{{#responses}}
{{#empty_response}}
            {{success_status}} => {
                let _ = response.bytes().await?;
                Ok({{responses_type}}::{{success_variant}}(()))
            },
{{/empty_response}}
{{^empty_response}}
            {{success_status}} => Ok({{responses_type}}::{{success_variant}}(
                response.json::<{{{rust_response_type}}}>().await?,
            )),
{{/empty_response}}
{{/responses}}
            _ => Ok({{responses_type}}::Error {
                status,
                message: response.text().await?,
            }),
        }
    }

{{/operations}}
}
