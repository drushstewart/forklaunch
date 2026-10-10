#![forbid(unsafe_code)]

use std::fmt::Write as _;

use forklaunch_sdk_codegen::{SdkGenerationPlan, SdkOperation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustSdkOptions {
    pub crate_name: String,
    pub client_struct: String,
}

impl RustSdkOptions {
    #[must_use]
    pub fn new(crate_name: impl Into<String>) -> Self {
        Self {
            crate_name: crate_name.into(),
            client_struct: "ForkLaunchClient".to_owned(),
        }
    }

    #[must_use]
    pub fn with_client_struct(mut self, client_struct: impl Into<String>) -> Self {
        self.client_struct = client_struct.into();
        self
    }
}

#[must_use]
pub fn rust_plan_summary(plan: &SdkGenerationPlan) -> String {
    format!("rust sdk operations: {}", plan.operations.len())
}

#[must_use]
pub fn generate_rust_sdk_source(plan: &SdkGenerationPlan, options: &RustSdkOptions) -> String {
    let mut source = String::new();
    let _ = writeln!(source, "#![forbid(unsafe_code)]");
    let _ = writeln!(source);
    let _ = writeln!(source, "use std::collections::BTreeMap;");
    let _ = writeln!(source);
    let _ = writeln!(source, "#[derive(Debug, Clone, PartialEq, Eq)]");
    let _ = writeln!(source, "pub struct SdkRequest {{");
    let _ = writeln!(source, "    pub method: &'static str,");
    let _ = writeln!(source, "    pub url: String,");
    let _ = writeln!(source, "    pub query: BTreeMap<String, String>,");
    let _ = writeln!(source, "    pub headers: BTreeMap<String, String>,");
    let _ = writeln!(source, "    pub body: Option<String>,");
    let _ = writeln!(source, "    pub body_schema: Option<&'static str>,");
    let _ = writeln!(source, "}}");
    let _ = writeln!(source);
    source.push_str(
        r#"fn encode_path_segment(value: &str) -> String {
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
"#,
    );
    let _ = writeln!(source);
    for operation in &plan.operations {
        write_request_struct(&mut source, operation);
    }

    let _ = writeln!(source, "#[derive(Debug, Clone)]");
    let _ = writeln!(source, "pub struct {} {{", options.client_struct);
    let _ = writeln!(source, "    base_url: String,");
    let _ = writeln!(source, "}}");
    let _ = writeln!(source);
    let _ = writeln!(source, "impl {} {{", options.client_struct);
    let _ = writeln!(source, "    #[must_use]");
    let _ = writeln!(
        source,
        "    pub fn new(base_url: impl Into<String>) -> Self {{"
    );
    let _ = writeln!(
        source,
        "        Self {{ base_url: base_url.into().trim_end_matches('/').to_owned() }}"
    );
    let _ = writeln!(source, "    }}");

    for operation in &plan.operations {
        write_operation(&mut source, operation);
    }

    let _ = writeln!(source, "}}");
    source
}

fn write_operation(source: &mut String, operation: &SdkOperation) {
    let operation_ident = rust_identifier(&operation.name);
    let request_type = format!("{}Request", rust_type_name(&operation.name));
    let method_name = format!("{operation_ident}_request");
    let body_schema = operation
        .request_body
        .as_ref()
        .map_or_else(|| "None".to_owned(), |schema| format!("Some(\"{schema}\")"));

    let _ = writeln!(source);
    let _ = writeln!(source, "    #[must_use]");
    let _ = writeln!(
        source,
        "    pub fn {method_name}(&self, input: {request_type}) -> SdkRequest {{"
    );
    let _ = writeln!(
        source,
        "        let path = path_with_params(\"{}\", &input.params);",
        escape_rust_string(&operation.path)
    );
    let _ = writeln!(source, "        SdkRequest {{");
    let _ = writeln!(
        source,
        "            method: \"{}\",",
        operation.method.as_str()
    );
    let _ = writeln!(
        source,
        "            url: format!(\"{{}}{{}}\", self.base_url, path),"
    );
    let _ = writeln!(source, "            query: input.query,");
    let _ = writeln!(source, "            headers: input.headers,");
    let _ = writeln!(source, "            body: input.body,");
    let _ = writeln!(source, "            body_schema: {body_schema},");
    let _ = writeln!(source, "        }}");
    let _ = writeln!(source, "    }}");
}

fn write_request_struct(source: &mut String, operation: &SdkOperation) {
    let request_type = format!("{}Request", rust_type_name(&operation.name));
    let _ = writeln!(source, "#[derive(Debug, Clone, Default, PartialEq, Eq)]");
    let _ = writeln!(source, "pub struct {request_type} {{");
    let _ = writeln!(source, "    pub params: BTreeMap<String, String>,");
    let _ = writeln!(source, "    pub query: BTreeMap<String, String>,");
    let _ = writeln!(source, "    pub headers: BTreeMap<String, String>,");
    let _ = writeln!(source, "    pub body: Option<String>,");
    let _ = writeln!(source, "}}");
    let _ = writeln!(source);
}

fn rust_identifier(input: &str) -> String {
    let mut output = String::new();
    for (index, ch) in input.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 && !output.ends_with('_') {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            output.push(ch);
        } else if !output.ends_with('_') {
            output.push('_');
        }
    }

    let output = output.trim_matches('_');
    if output.is_empty() {
        return "operation".to_owned();
    }

    if output.starts_with(|ch: char| ch.is_ascii_digit()) {
        format!("operation_{output}")
    } else {
        output.to_owned()
    }
}

fn rust_type_name(input: &str) -> String {
    let mut output = String::new();
    let mut uppercase_next = true;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                output.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                output.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }

    if output.is_empty() {
        "Operation".to_owned()
    } else if output.starts_with(|ch: char| ch.is_ascii_digit()) {
        format!("Operation{output}")
    } else {
        output
    }
}

fn escape_rust_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use forklaunch_contract::{BodyContract, FieldContract, HttpMethod, RouteContract};
    use forklaunch_sdk_codegen::SdkTarget;

    use super::*;

    #[test]
    fn generates_request_builders_for_rust_sdk() {
        let mut registry = forklaunch_contract::ContractRegistry::new();
        registry.register(
            RouteContract::new(
                "Create User",
                "Create a user",
                HttpMethod::Post,
                "/users/:id",
            )
            .with_param(FieldContract::required("id", "String"))
            .with_query(FieldContract::optional("dryRun", "Boolean"))
            .with_request_body(BodyContract::json("CreateUserBody"))
            .with_response(201, Some(BodyContract::json("User"))),
        );
        let plan = SdkGenerationPlan::from_registry(SdkTarget::Rust, &registry);

        let source = generate_rust_sdk_source(&plan, &RustSdkOptions::new("accounts-sdk"));

        assert!(source.contains("pub struct ForkLaunchClient"));
        assert!(source.contains("pub struct CreateUserRequest"));
        assert!(source.contains("pub fn create_user_request(&self, input: CreateUserRequest)"));
        assert!(source.contains("path_with_params"));
        assert!(source.contains("method: \"POST\""));
        assert!(source.contains("query: input.query"));
        assert!(source.contains("body_schema: Some(\"CreateUserBody\")"));
    }
}
