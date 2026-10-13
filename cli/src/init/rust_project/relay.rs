//! Relay extends an existing Rust Better Auth service, including mixed apps.
use super::*;

pub(super) fn generate(matches: &ArgMatches) -> Result<()> {
    let root = find_manifest_root(
        &option(matches, "base_path")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?),
    )?;
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    let name = option(matches, "name").unwrap_or_else(|| "iam".into());
    let project = project_table(&manifest, &name)
        .context("Relay requires an existing Rust iam-better-auth service")?;
    if project.get("variant").and_then(Value::as_str) != Some("iam-better-auth")
        || !is_rust_project(&root, &name)?
    {
        bail!("Relay requires an existing Rust iam-better-auth service");
    }
    let directory = root.join(
        crate_path_for_project(&manifest, &name).context("Rust auth service has no crate path")?,
    );
    let hooks = directory.join("src/relay_hooks.rs");
    if hooks.exists() {
        bail!("Relay is already installed; edit src/relay_hooks.rs to customize it");
    }
    let main_path = directory.join("src/main.rs");
    let lib_path = directory.join("src/lib.rs");
    let namespace = name.to_case(Case::Snake);
    // Prepare all edits before writing. Refuse ambiguous/custom wiring rather
    // than silently installing endpoints without their authorization policy.
    let main = replace_once(
        &read_to_string(&main_path)?,
        "let auth = HmacJwksAuth::new(secret.into_bytes(), jwks, validation)?;",
        "let auth = HmacJwksAuth::new(secret.as_bytes().to_vec(), jwks, validation)?;\n    let auth = forklaunch_iam_better_auth::http::relay::configure_auth(\n        auth, secret.as_bytes(), &env::var(\"INSTANCE_ID\").unwrap_or_default(),\n        env::var(\"INSTANCE_HMAC_KEY\").unwrap_or_default().as_bytes(),\n    )?;",
    )?;
    let main = replace_once(
        &main,
        "application.application_router(auth, service_info())",
        &format!(
            "application.application_router_with_relay(auth, service_info(), Arc::new({namespace}::relay_hooks::AppRelayHooks))"
        ),
    )?;
    let lib = replace_once(
        &read_to_string(&lib_path)?,
        "pub mod routes;",
        "pub mod routes;\npub mod relay_hooks;",
    )?;
    let lib = replace_once(
        &lib,
        "registry.register(health_contract());",
        "registry.register(health_contract());\n    for route in forklaunch_iam_better_auth::http::relay::contracts() { registry.register(route); }",
    )?;
    let templates = vec![
        RenderedTemplate {
            path: main_path,
            content: main,
            context: None,
        },
        RenderedTemplate {
            path: lib_path,
            content: lib,
            context: None,
        },
        RenderedTemplate {
            path: hooks,
            content: HOOKS.into(),
            context: None,
        },
        RenderedTemplate {
            path: directory.join("RELAY.md"),
            content: README.into(),
            context: None,
        },
    ];
    let dryrun = matches.get_flag("dryrun");
    ensure_embedded_framework(&root, dryrun)?;
    modules::ensure_blueprint_sources(&root, dryrun)?;
    write_rendered_templates(
        &templates,
        dryrun,
        &mut StandardStream::stdout(ColorChoice::Always),
    )?;
    Ok(())
}

fn replace_once(source: &str, anchor: &str, replacement: &str) -> Result<String> {
    if source.matches(anchor).count() != 1 {
        bail!(
            "Cannot safely install relay into customized auth wiring: expected exactly one {anchor}"
        );
    }
    Ok(source.replacen(anchor, replacement, 1))
}

const HOOKS: &str = r#"// Application-owned relay policy, preserved by generation and sync.
use std::sync::Arc;
use forklaunch_blueprint_core::BlueprintResult;
use forklaunch_iam_better_auth::{
    relay::{AppClaimLink, RelayEstablishment, RelayHooks, RelayTokens},
    store::postgres::PostgresAuthStore,
};

pub struct AppRelayHooks;
impl RelayHooks<PostgresAuthStore> for AppRelayHooks {
    async fn establish(&self, _tokens: RelayTokens, _store: Arc<PostgresAuthStore>) -> BlueprintResult<RelayEstablishment> {
        // Persist provider tokens securely and resolve an existing owner here.
        // The safe default redirects home without creating a signed-in session.
        // Nonces are already committed, even if this hook fails.
        Ok(RelayEstablishment::default())
    }

    async fn mint_claim(&self, _instance_id: &str, _store: Arc<PostgresAuthStore>) -> BlueprintResult<Option<AppClaimLink>> {
        // Implement the product-specific claim ceremony; None declines with 409.
        Ok(None)
    }
}
"#;

const README: &str = r#"# Managed-app relay

Customize `src/relay_hooks.rs` to persist provider tokens securely, resolve an
existing owner, and optionally implement the app claim ceremony. The initial
hooks redirect home without signing in and decline claim requests with HTTP 409.

Run the IAM migration command before starting the updated service. Relay uses
the existing encrypted, tenant-scoped auth store, including its audit grants.

`POST /relay/session-ingest` uses `INSTANCE_ID` as its HMAC key ID (default:
`default`) and `INSTANCE_HMAC_KEY` as its secret (fallback: `HMAC_SECRET_KEY`).
`POST /claim/mint` accepts the internal `default` key and the `platform` key
when `INSTANCE_HMAC_KEY` is set. These keys apply only to their relay routes.
Both operations are exposed through HTTP, MCP, and generated SDK contracts.

`GET /relay/handoff?ticket=...` is a browser redirect, outside MCP and SDKs.
Tickets expire after five minutes and are atomically consumed before minting
the one-day Better Auth session cookie. Failed hooks still consume the nonce.
Only root-relative redirects without protocol-relative prefixes or ASCII
control characters are accepted. Provider tokens are never stored in tickets.
"#;
