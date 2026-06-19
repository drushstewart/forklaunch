use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
};

use forklaunch_contract::HttpMethod;
use forklaunch_http::{Body, FrameworkRequest, RequestContext};
use {{crate_module}}::{
    AppState, SERVICE_NAME, build_state, config::ServiceConfig, local_router,
    write_contract_artifacts,
};

fn main() -> std::io::Result<()> {
    if export_contract_artifacts()? {
        return Ok(());
    }

    let config = ServiceConfig::from_env();
    let addr = config.bind_addr();
    let state = build_state(config).map_err(to_io_error)?;
    let router = local_router(state.clone());

    println!("{SERVICE_NAME} listening on http://{addr}");
    let listener = TcpListener::bind(addr)?;
    for stream in listener.incoming() {
        handle_client(stream?, &state, &router)?;
    }

    Ok(())
}

fn handle_client(
    mut stream: TcpStream,
    _state: &AppState,
    router: &forklaunch_http::LocalRouter,
) -> std::io::Result<()> {
    let mut buffer = [0_u8; 2048];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let (method, path) = request_line_parts(&request);
    let response = router
        .execute(FrameworkRequest::new(
            method,
            path,
            RequestContext::new("local-request"),
        ))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let status = status_line(response.status_code);
    let body = match response.body {
        Body::Empty => String::new(),
        Body::Text(body) => body,
        Body::Binary(bytes) => String::from_utf8_lossy(&bytes).to_string(),
    };
    let content_type = response
        .headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("application/json");

    write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn export_contract_artifacts() -> std::io::Result<bool> {
    let mode = env::var("FORKLAUNCH_MODE").unwrap_or_default();
    if mode != "openapi" && mode != "contract" {
        return Ok(false);
    }

    let contract_output = env::var("FORKLAUNCH_CONTRACT_OUTPUT")
        .unwrap_or_else(|_| ".forklaunch/contracts/contract.json".to_string());
    let openapi_output = env::var("FORKLAUNCH_OPENAPI_OUTPUT")
        .unwrap_or_else(|_| ".forklaunch/openapi/openapi.json".to_string());

    if let Some(parent) = Path::new(&openapi_output).parent() {
        fs::create_dir_all(parent)?;
    }
    write_contract_artifacts(contract_output, openapi_output)?;
    Ok(true)
}

fn request_line_parts(request: &str) -> (HttpMethod, &str) {
    let mut parts = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = match parts.next().unwrap_or("GET") {
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        "TRACE" => HttpMethod::Trace,
        _ => HttpMethod::Get,
    };
    let path = parts.next().unwrap_or("/");
    (method, path)
}

fn status_line(status_code: u16) -> String {
    match status_code {
        200 => "200 OK".to_string(),
        201 => "201 Created".to_string(),
        204 => "204 No Content".to_string(),
        400 => "400 Bad Request".to_string(),
        401 => "401 Unauthorized".to_string(),
        403 => "403 Forbidden".to_string(),
        404 => "404 Not Found".to_string(),
        429 => "429 Too Many Requests".to_string(),
        500 => "500 Internal Server Error".to_string(),
        code => format!("{code} Response"),
    }
}

fn to_io_error(error: {{crate_module}}::error::AppError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, error)
}
