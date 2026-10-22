//! Session auth wiring shared by generated Rust services, kept to the auth
//! variables so other compose generators stay independent:
//!
//! - the Rust IAM module signs RS256 session JWTs (`SESSION_JWT_*`) and its
//!   one-shot `<iam>-seed` service runs `--seed` after migrations;
//! - every JWT verifier pins one shared `JWT_ISSUER`/`JWT_AUDIENCE`;
//! - custom services verify IAM sessions through `JWKS_PUBLIC_KEY_URL` once an
//!   IAM module exists, and every Rust HTTP service reads `CORS_ORIGINS`;
//! - each service's `.env.local` gets the host-side JWKS URL and issuer/audience.
//!
//! Development values (a fresh RSA key, key id, issuer, audience and seed
//! password) are written to the git-ignored root `.env` by `local_runtime`.
use base64::Engine;
use indexmap::IndexMap;
use rsa::pkcs8::{EncodePrivateKey, LineEnding};

use super::*;

const ISSUER: &str = "${JWT_ISSUER:?Configure the trusted IAM session issuer}";
const AUDIENCE: &str = "${JWT_AUDIENCE:?Configure the trusted IAM session audience}";
const CORS: &str = "${CORS_ORIGINS:-}";
const SEED_PASSWORD: &str = "IAM_SEED_ADMIN_PASSWORD";

fn compose_path(root: &Path, manifest: &Value) -> PathBuf {
    root.join(
        manifest
            .get("docker_compose_path")
            .and_then(Value::as_str)
            .unwrap_or("docker-compose.yaml"),
    )
}

fn projects(manifest: &Value) -> impl Iterator<Item = &toml::Table> {
    manifest
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_table)
}

fn is_rust(project: &toml::Table) -> bool {
    project
        .get("metadata")
        .and_then(|metadata| metadata.get("language"))
        .and_then(Value::as_str)
        .is_none_or(|language| language == "rust")
}

fn iam_project(manifest: &Value) -> Option<String> {
    projects(manifest)
        .find(|project| project.get("variant").and_then(Value::as_str) == Some("iam-base") && is_rust(project))
        .and_then(|project| project.get("name").and_then(Value::as_str))
        .map(str::to_owned)
}

/// Rust HTTP services owned by the application (custom services and agents),
/// as opposed to prebuilt modules, which configure their own auth.
fn custom_services(manifest: &Value) -> Vec<String> {
    projects(manifest)
        .filter(|project| project.get("type").and_then(Value::as_str) == Some("Service") && is_rust(project))
        .filter(|project| {
            project
                .get("variant")
                .and_then(Value::as_str)
                .is_none_or(|variant| variant == "agent")
        })
        .filter_map(|project| project.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn ensure(env: &mut IndexMap<String, String>, key: &str, value: impl Into<String>) {
    env.entry(key.to_owned()).or_insert_with(|| value.into());
}

/// Adds the auth variables and the IAM seed service to `docker-compose.yaml`.
pub(super) fn compose(root: &Path) -> Result<()> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    let path = compose_path(root, &manifest);
    if !path.is_file() {
        return Ok(());
    }
    let mut compose = read_or_create_rust_docker_compose(&path)?;
    let iam = iam_project(&manifest).filter(|name| compose.services.contains_key(name));
    if let Some(iam) = &iam {
        let service = compose.services.get_mut(iam).expect("IAM compose service");
        let env = service.environment.get_or_insert_with(IndexMap::new);
        let signing = ["SESSION_JWT_PRIVATE_KEY_PEM", "SESSION_JWT_KEY_ID", "SESSION_JWT_ISSUER", "SESSION_JWT_AUDIENCE"];
        // All four or none: an application that removed them chose opaque sessions.
        if !signing.iter().any(|key| env.contains_key(*key)) {
            ensure(env, "SESSION_JWT_PRIVATE_KEY_PEM", "${IAM_SESSION_JWT_PRIVATE_KEY_PEM:?Configure the IAM session signing key}");
            ensure(env, "SESSION_JWT_KEY_ID", "${IAM_SESSION_JWT_KEY_ID:?Configure the IAM session signing key id}");
            ensure(env, "SESSION_JWT_ISSUER", ISSUER);
            ensure(env, "SESSION_JWT_AUDIENCE", AUDIENCE);
        }
        ensure(env, "CORS_ORIGINS", CORS);
        let seed = format!("{iam}-seed");
        if !compose.services.contains_key(&seed) {
            let source = compose.services[iam].clone();
            if let Some(DockerCommand::Multiple(command)) = &source.command
                && let Some(binary) = command.first()
            {
                let mut environment = source.environment.clone().unwrap_or_default();
                environment.insert(SEED_PASSWORD.into(), format!("${{{SEED_PASSWORD}:?Set the seeded IAM admin password}}"));
                let service = DockerService {
                    image: source.image.clone(),
                    restart: Some(Restart::No),
                    environment: Some(environment),
                    networks: source.networks.clone(),
                    working_dir: source.working_dir.clone(),
                    command: Some(DockerCommand::Multiple(vec![binary.clone(), "--seed".into()])),
                    ..Default::default()
                };
                compose.services.insert(seed, service);
            }
        }
    }
    let custom = custom_services(&manifest);
    for (name, service) in compose.services.iter_mut() {
        let env = service.environment.get_or_insert_with(IndexMap::new);
        if custom.contains(name) {
            if iam.is_some() {
                let prefix = name.to_case(Case::UpperSnake);
                ensure(env, "JWKS_PUBLIC_KEY_URL", format!("${{{prefix}_JWKS_PUBLIC_KEY_URL:?Configure trusted JWKS}}"));
            }
            ensure(env, "CORS_ORIGINS", CORS);
        }
        if env.contains_key("JWKS_PUBLIC_KEY_URL") {
            ensure(env, "JWT_ISSUER", ISSUER);
            ensure(env, "JWT_AUDIENCE", AUDIENCE);
        }
    }
    write(&path, yaml_to_string(&compose)?)?;
    Ok(())
}

/// A development value for an auth variable `local_runtime` must fill.
pub(super) fn local_value(name: &str, app: &str) -> Option<String> {
    if name.ends_with("SESSION_JWT_PRIVATE_KEY_PEM") {
        return Some(development_key());
    }
    if name.ends_with("SESSION_JWT_KEY_ID") {
        return Some(format!("{app}-dev-1"));
    }
    match name {
        "JWT_ISSUER" => Some(format!("{app}-iam")),
        "JWT_AUDIENCE" => Some(format!("{app}-api")),
        SEED_PASSWORD => {
            let mut bytes = [0; 18];
            getrandom::fill(&mut bytes).expect("Failed to generate random bytes");
            Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
        }
        _ => None,
    }
}

/// A 2048-bit RSA PKCS#8 key, base64-encoded so it fits one `.env` line.
fn development_key() -> String {
    let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).expect("Failed to generate an RSA key");
    let pem = key.to_pkcs8_pem(LineEnding::LF).expect("Failed to encode the RSA key");
    base64::engine::general_purpose::STANDARD.encode(pem.as_bytes())
}

fn env_values(path: &Path) -> IndexMap<String, String> {
    read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect()
}

/// Host-run (`cargo run`) settings mirroring the compose auth wiring: the IAM
/// JWKS URL on its published localhost port and the shared issuer/audience.
/// Existing values are kept.
pub(super) fn local_envs(root: &Path) -> Result<()> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    let path = compose_path(root, &manifest);
    if !path.is_file() {
        return Ok(());
    }
    let compose = read_or_create_rust_docker_compose(&path)?;
    let root_env = env_values(&root.join(".env"));
    let jwks = iam_project(&manifest)
        .and_then(|iam| compose.services.get(&iam))
        .and_then(|iam| iam.ports.as_deref())
        .and_then(|ports| ports.iter().find_map(|port| crate::core::docker::compose_host_port(port)))
        .map(|port| format!("http://localhost:{port}/api/auth/jwks"));
    for project in projects(&manifest).filter(|project| is_rust(project)) {
        let Some(name) = project.get("name").and_then(Value::as_str) else { continue };
        let Some(env) = compose.services.get(name).and_then(|service| service.environment.as_ref()) else { continue };
        if !env.contains_key("JWKS_PUBLIC_KEY_URL") {
            continue;
        }
        let crate_path = project
            .get("metadata")
            .and_then(|metadata| metadata.get("cratePath"))
            .and_then(Value::as_str)
            .map_or_else(|| format!("src/{name}"), str::to_owned);
        let local = root.join(crate_path).join(".env.local");
        if !local.is_file() {
            continue;
        }
        let existing = read_to_string(&local)?;
        let present = env_values(&local);
        let mut content = existing.clone();
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        let values = [
            ("JWKS_PUBLIC_KEY_URL", jwks.clone()),
            ("JWT_ISSUER", root_env.get("JWT_ISSUER").cloned()),
            ("JWT_AUDIENCE", root_env.get("JWT_AUDIENCE").cloned()),
        ];
        for (key, value) in values {
            if let Some(value) = value.filter(|_| !present.contains_key(key)) {
                content.push_str(&format!("{key}={value}\n"));
            }
        }
        if content != existing {
            write(&local, content)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_values_are_fresh_and_single_line() {
        let key = local_value("IAM_SESSION_JWT_PRIVATE_KEY_PEM", "app").unwrap();
        assert!(!key.contains('\n'));
        let pem = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(&key).unwrap()).unwrap();
        assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----"));
        assert_ne!(local_value("IAM_SESSION_JWT_PRIVATE_KEY_PEM", "app").unwrap(), key);
        assert_eq!(local_value("IAM_SESSION_JWT_KEY_ID", "app").as_deref(), Some("app-dev-1"));
        assert_eq!(local_value("JWT_ISSUER", "app").as_deref(), Some("app-iam"));
        assert_eq!(local_value("JWT_AUDIENCE", "app").as_deref(), Some("app-api"));
        let password = local_value(SEED_PASSWORD, "app").unwrap();
        assert_eq!(password.len(), 24);
        assert_ne!(local_value(SEED_PASSWORD, "app").unwrap(), password);
        assert_eq!(local_value("OTHER", "app"), None);
    }
}
