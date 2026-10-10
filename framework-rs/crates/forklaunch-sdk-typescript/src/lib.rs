#![forbid(unsafe_code)]

use std::fmt::Write as _;

use forklaunch_sdk_codegen::{SdkGenerationPlan, SdkOperation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScriptSdkOptions {
    pub client_name: String,
    pub import_fetch: bool,
}

impl TypeScriptSdkOptions {
    #[must_use]
    pub fn new(client_name: impl Into<String>) -> Self {
        Self {
            client_name: client_name.into(),
            import_fetch: false,
        }
    }

    #[must_use]
    pub const fn with_fetch_import(mut self, import_fetch: bool) -> Self {
        self.import_fetch = import_fetch;
        self
    }
}

#[must_use]
pub fn typescript_plan_summary(plan: &SdkGenerationPlan) -> String {
    format!("typescript sdk operations: {}", plan.operations.len())
}

#[must_use]
pub fn generate_typescript_sdk_source(
    plan: &SdkGenerationPlan,
    options: &TypeScriptSdkOptions,
) -> String {
    let mut source = String::new();
    if options.import_fetch {
        let _ = writeln!(source, "import fetch from 'cross-fetch';");
        let _ = writeln!(source);
    }

    let _ = writeln!(
        source,
        "export type SdkQueryValue = string | number | boolean | undefined;"
    );
    let _ = writeln!(
        source,
        "export type SdkPathValue = string | number | boolean;"
    );
    let _ = writeln!(source);
    let _ = writeln!(source, "export interface SdkRequestOptions {{");
    let _ = writeln!(source, "  params?: Record<string, SdkPathValue>;");
    let _ = writeln!(source, "  query?: Record<string, SdkQueryValue>;");
    let _ = writeln!(source, "  body?: unknown;");
    let _ = writeln!(source, "  headers?: Record<string, string>;");
    let _ = writeln!(source, "}}");
    let _ = writeln!(source);
    let _ = writeln!(
        source,
        "function pathWithParams(path: string, params?: Record<string, SdkPathValue>) {{"
    );
    let _ = writeln!(source, "  if (!params) return path;");
    source.push_str(
        r"  return path.split('/').map(segment => {
    const name = segment.startsWith(':') ? segment.slice(1)
      : segment.startsWith('{') && segment.endsWith('}') ? segment.slice(1, -1) : undefined;
    return name !== undefined && Object.prototype.hasOwnProperty.call(params, name)
      ? encodeURIComponent(String(params[name])) : segment;
  }).join('/');
",
    );
    let _ = writeln!(source, "}}");
    let _ = writeln!(source);
    let _ = writeln!(source, "export class {} {{", options.client_name);
    let _ = writeln!(
        source,
        "  constructor(private readonly baseUrl: string, private readonly fetcher: typeof fetch = fetch) {{}}"
    );

    for operation in &plan.operations {
        write_operation(&mut source, operation);
    }

    let _ = writeln!(source, "}}");
    source
}

fn write_operation(source: &mut String, operation: &SdkOperation) {
    let options_default = if operation.request_body.is_none()
        && operation.request_params.iter().all(|field| !field.required)
    {
        " = {}"
    } else {
        ""
    };
    let body_line = if operation.request_body.is_some() {
        "      body: options.body === undefined ? undefined : JSON.stringify(options.body),"
    } else {
        "      body: undefined,"
    };

    let _ = writeln!(source);
    let _ = writeln!(
        source,
        "  {}(options: SdkRequestOptions{}): Promise<Response> {{",
        typescript_identifier(&operation.name),
        options_default
    );
    let _ = writeln!(
        source,
        "    const path = pathWithParams(`{}`, options.params);",
        escape_template_literal(&operation.path)
    );
    let _ = writeln!(
        source,
        "    const query = options.query ? `?${{new URLSearchParams(Object.entries(options.query).filter(([, value]) => value !== undefined).map(([key, value]) => [key, String(value)]))}}` : '';"
    );
    let _ = writeln!(
        source,
        "    return this.fetcher(`${{this.baseUrl}}${{path}}${{query}}`, {{"
    );
    let _ = writeln!(source, "      method: '{}',", operation.method.as_str());
    let _ = writeln!(source, "      headers: {{");
    let _ = writeln!(
        source,
        "        ...(options.body === undefined ? {{}} : {{ 'content-type': 'application/json' }}),"
    );
    let _ = writeln!(source, "        ...options.headers,");
    let _ = writeln!(source, "      }},");
    let _ = writeln!(source, "{body_line}");
    let _ = writeln!(source, "    }});");
    let _ = writeln!(source, "  }}");
}

fn typescript_identifier(input: &str) -> String {
    let mut output = String::new();
    for (index, ch) in input.chars().enumerate() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if index == 0 && ch.is_ascii_digit() {
                output.push('_');
            }
            output.push(ch);
        }
    }

    if output.is_empty() {
        "operation".to_owned()
    } else {
        output
    }
}

fn escape_template_literal(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${")
}

#[cfg(test)]
mod tests {
    use forklaunch_contract::{BodyContract, FieldContract, HttpMethod, RouteContract};
    use forklaunch_sdk_codegen::SdkTarget;

    use super::*;

    #[test]
    fn generates_frontend_client_matching_live_typed_syntax() {
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
        let plan = SdkGenerationPlan::from_registry(SdkTarget::TypeScript, &registry);

        let source = generate_typescript_sdk_source(&plan, &TypeScriptSdkOptions::new("ApiClient"));

        assert!(source.contains("export class ApiClient"));
        assert!(source.contains("pathWithParams"));
        assert!(source.contains("createUser(options: SdkRequestOptions)"));
        assert!(source.contains("options.query"));
        assert!(source.contains("method: 'POST'"));
        assert!(source.contains("JSON.stringify(options.body)"));
    }
}
