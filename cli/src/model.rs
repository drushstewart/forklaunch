//! Machine-readable, short-lived credentials for local agents. Never exports the login token.
use std::{
    io::{IsTerminal, Read},
    time::Duration,
};

use anyhow::{Result, bail};
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde::{Deserialize, Serialize};

use crate::{
    CliCommand,
    constants::get_studio_orchestrator_api_url,
    core::token::{API_KEY_PREFIX, get_token},
};

pub(crate) struct ModelCommand;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Grant {
    grant_id: String,
    capability: String,
    gateway_base_url: String,
    model: String,
    expires_at: i64,
    max_calls: u32,
    max_units: u32,
    scope: String,
}
fn validate(grant: &Grant, base: &str, now: i64) -> Result<()> {
    let hex = |s: &str| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit());
    if !hex(&grant.grant_id)
        || !hex(&grant.capability)
        || grant.gateway_base_url
            != format!(
                "{base}/studio-orchestrator/desktop-runtime-model-gateway/{}/v1",
                grant.grant_id
            )
        || grant.scope != "local-runtime"
        || grant.expires_at <= now
        || grant.expires_at > now + 900_000
        || !(1..=20).contains(&grant.max_calls)
        || !(1..=100_000).contains(&grant.max_units)
        || grant.model.is_empty()
        || grant.model.len() > 128
        || !grant
            .model
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        bail!("Invalid local model credential received. No model request was sent.");
    }
    Ok(())
}
impl CliCommand for ModelCommand {
    fn command(&self) -> Command {
        Command::new("model")
            .about("Connect local agents using your ForkLaunch account")
            .subcommand_required(true)
            .subcommand(
                Command::new("credential")
                    .about("Issue a bounded local model credential (not your account token)")
                    .arg(
                        Arg::new("application-id")
                            .long("application-id")
                            .required(true),
                    )
                    .arg(Arg::new("model").long("model"))
                    .arg(
                        Arg::new("json")
                            .long("json")
                            .action(ArgAction::SetTrue)
                            .required(true),
                    ),
            )
    }
    fn handler(&self, matches: &ArgMatches) -> Result<()> {
        let (_, args) = matches
            .subcommand()
            .ok_or_else(|| anyhow::anyhow!("Choose a model command."))?;
        if std::io::stdout().is_terminal() {
            bail!(
                "Model credentials are secret. Use this helper through the framework client or pipe it to a trusted process."
            );
        }
        let app = args.get_one::<String>("application-id").unwrap();
        uuid::Uuid::parse_str(app)
            .map_err(|_| anyhow::anyhow!("A saved ForkLaunch application ID is required."))?;
        let base = get_studio_orchestrator_api_url()
            .trim_end_matches('/')
            .to_owned();
        let url = reqwest::Url::parse(&base)?;
        if url.scheme() != "https"
            || url.host_str() != Some("studio-orchestrator.forklaunch.com")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            bail!("Local model credentials currently require the hosted ForkLaunch gateway.");
        }
        let token = match std::env::var("FORKLAUNCH_API_KEY") {
            Ok(key) => {
                if !key.starts_with(API_KEY_PREFIX) {
                    bail!("FORKLAUNCH_API_KEY must be a ForkLaunch API key.");
                }
                // Explicit configuration wins; never silently fall back when it is refused.
                {
                    let client = reqwest::blocking::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .timeout(Duration::from_secs(30))
                        .build()?;
                    let response = client
                        .get("https://iam.forklaunch.com/api/auth/token")
                        .header("x-api-key", key)
                        .send()
                        .map_err(|_| {
                            anyhow::anyhow!(
                                "Could not authenticate the configured ForkLaunch API key."
                            )
                        })?;
                    if !response.status().is_success() {
                        bail!("The configured ForkLaunch API key was refused.");
                    }
                    let mut bytes = Vec::new();
                    response.take(65537).read_to_end(&mut bytes)?;
                    if bytes.len() > 65536 {
                        bail!("Invalid authentication response.");
                    }
                    let value: serde_json::Value = serde_json::from_slice(&bytes)
                        .map_err(|_| anyhow::anyhow!("Invalid authentication response."))?;
                    value["token"]
                        .as_str()
                        .filter(|v| !v.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("Invalid authentication response."))?
                        .to_owned()
                }
            }
            Err(std::env::VarError::NotPresent) => get_token()?,
            Err(_) => bail!("Invalid FORKLAUNCH_API_KEY."),
        };
        let mut body = serde_json::json!({"applicationId":app,"previewRequestId":uuid::Uuid::now_v7().to_string()});
        if let Some(model) = args.get_one::<String>("model") {
            body["modelId"] = serde_json::json!(model);
        }
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()?;
        let response = client
            .post(format!(
                "{base}/studio-orchestrator/desktop-runtime-model-grants"
            ))
            .bearer_auth(token)
            .json(&body)
            .send()
            .map_err(|_| anyhow::anyhow!("Could not connect to the ForkLaunch model gateway."))?;
        if !response.status().is_success() {
            bail!(
                "ForkLaunch refused the model connection (HTTP {}). Check your login, app access and model allowance.",
                response.status().as_u16()
            );
        }
        let mut bytes = Vec::new();
        response.take(65537).read_to_end(&mut bytes)?;
        if bytes.len() > 65536 {
            bail!("Invalid model credential response.");
        }
        let grant: Grant = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid model credential response."))?;
        validate(&grant, &base, chrono::Utc::now().timestamp_millis())?;
        println!("{}", serde_json::to_string(&grant)?);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_credentials_reject_wrong_origin_expiry_and_scope() {
        let base = "https://studio-orchestrator.forklaunch.com";
        let mut g = Grant {
            grant_id: "a".repeat(64),
            capability: "b".repeat(64),
            gateway_base_url: format!(
                "{base}/studio-orchestrator/desktop-runtime-model-gateway/{}/v1",
                "a".repeat(64)
            ),
            model: "model-a".into(),
            expires_at: 2000,
            max_calls: 20,
            max_units: 100000,
            scope: "local-runtime".into(),
        };
        assert!(validate(&g, base, 1000).is_ok());
        assert!(validate(&g, base, 2000).is_err());
        g.gateway_base_url = "https://attacker.invalid".into();
        assert!(validate(&g, base, 1000).is_err());
    }
}
