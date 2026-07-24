use std::io::Write;

use anyhow::{Context, Result, bail};
use clap::{Arg, ArgAction, ArgMatches, Command};
use reqwest::Method;
use serde_json::{Value, json};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use crate::{
    CliCommand,
    constants::get_studio_api_url,
    core::{command::command, http_client::make_authenticated_request},
};

/// `forklaunch score` — run the ForkLaunch Score (enterprise-readiness report card) against the
/// app at `--path` and render it in the terminal. The scoring itself runs on the studio analyzer
/// (`FORKLAUNCH_STUDIO_API_URL`), which must be able to read the given path — i.e. local dev,
/// self-hosted, or the studio build harness. Designed for the agent self-heal loop: `--json`
/// emits the raw card (per-item scores + concrete fixes) and `--threshold` gates the exit code,
/// so "doesn't pass" is machine-checkable.
pub(crate) struct ScoreCommand;

impl ScoreCommand {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

/// Canonical pillar order + display labels (mirrors REPORT_CARD_RAILS in the platform).
const RAILS: [(&str, &str); 7] = [
    ("compliance", "Compliance"),
    ("security", "Security"),
    ("governance", "Governance"),
    ("scalability", "Scalability"),
    ("observability", "Observability"),
    ("manageable", "Manageability"),
    ("performant", "Performance"),
];

fn score_color(score: f64) -> Color {
    if score >= 80.0 {
        Color::Green
    } else if score >= 60.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn write_scored(stdout: &mut StandardStream, score: f64, text: &str) -> Result<()> {
    stdout.set_color(ColorSpec::new().set_fg(Some(score_color(score))).set_bold(true))?;
    write!(stdout, "{}", text)?;
    stdout.reset()?;
    Ok(())
}

impl CliCommand for ScoreCommand {
    fn command(&self) -> Command {
        command(
            "score",
            "Score the app across the seven readiness pillars (ForkLaunch Score)",
        )
        .arg(
            Arg::new("base_path")
                .short('p')
                .long("path")
                .help("The application path to score (defaults to the current directory)"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Print the raw report card JSON (for agents and CI)"),
        )
        .arg(
            Arg::new("threshold")
                .short('t')
                .long("threshold")
                .help("Exit non-zero when the overall score is below this value (self-heal / CI gate)"),
        )
    }

    fn handler(&self, matches: &ArgMatches) -> Result<()> {
        let mut stdout = StandardStream::stdout(ColorChoice::Always);

        let base_path = matches
            .get_one::<String>("base_path")
            .map(String::as_str)
            .unwrap_or(".");
        let workspace_root = std::fs::canonicalize(base_path)
            .with_context(|| format!("Cannot resolve path: {}", base_path))?;
        let app_id = workspace_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("app")
            .to_string();

        let url = format!(
            "{}/studio-orchestrator/analysis/repo",
            get_studio_api_url()
        );
        writeln!(
            stdout,
            "Scoring {} across the seven readiness pillars (~1-2 min)...",
            workspace_root.display()
        )?;

        let response = make_authenticated_request(
            Method::POST,
            &url,
            Some(json!({
                "appId": app_id,
                "workspaceRoot": workspace_root.to_string_lossy(),
            })),
        )?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            bail!(
                "Scoring failed ({}): {}\nHint: the analyzer must be able to read the path — point FORKLAUNCH_STUDIO_API_URL at a studio orchestrator with access to this workspace.",
                status,
                body.chars().take(400).collect::<String>()
            );
        }

        let result: Value = response.json().context("Analyzer returned invalid JSON")?;
        let card = &result["reportCard"];
        if card.is_null() {
            bail!("Analyzer response carried no report card");
        }

        if matches.get_flag("json") {
            writeln!(stdout, "{}", serde_json::to_string_pretty(card)?)?;
        } else {
            let overall = card["overall"].as_f64().unwrap_or(0.0);
            writeln!(stdout)?;
            stdout.set_color(ColorSpec::new().set_bold(true))?;
            write!(stdout, "ForkLaunch Score  ")?;
            stdout.reset()?;
            write_scored(&mut stdout, overall, &format!("{:.0}/100", overall))?;
            writeln!(stdout)?;
            if let Some(headline) = card["headline"].as_str() {
                writeln!(stdout, "{}", headline)?;
            }
            if let Some(caveat) = card["caveat"].as_str() {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
                writeln!(stdout, "⚠ {}", caveat)?;
                stdout.reset()?;
            }

            for (key, label) in RAILS {
                let dim = &card["dimensions"][key];
                if dim.is_null() {
                    continue;
                }
                let score = dim["score"].as_f64().unwrap_or(0.0);
                writeln!(stdout)?;
                stdout.set_color(ColorSpec::new().set_bold(true))?;
                write!(stdout, "{:<15}", label)?;
                stdout.reset()?;
                write_scored(&mut stdout, score, &format!("{:.0}", score))?;
                writeln!(stdout)?;

                if let Some(items) = dim["items"].as_array() {
                    for item in items {
                        let status = item["status"].as_str().unwrap_or("unmet");
                        let icon = match status {
                            "met" => "✓",
                            "pending" => "?",
                            _ => "○",
                        };
                        let item_score = item["score"].as_f64();
                        write!(stdout, "  {} ", icon)?;
                        write!(stdout, "{}", item["label"].as_str().unwrap_or(""))?;
                        if let Some(s) = item_score {
                            write!(stdout, "  ")?;
                            write_scored(&mut stdout, s, &format!("{:.0}", s))?;
                        }
                        writeln!(stdout)?;
                        // The self-heal food: a concrete remediation per unmet criterion.
                        if status != "met" {
                            if let Some(fix) = item["fix"].as_str() {
                                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                                writeln!(stdout, "      fix: {}", fix)?;
                                stdout.reset()?;
                            }
                        }
                    }
                }
            }
            writeln!(stdout)?;
        }

        if let Some(threshold) = matches.get_one::<String>("threshold") {
            let threshold: f64 = threshold
                .parse()
                .context("--threshold must be a number between 0 and 100")?;
            let overall = card["overall"].as_f64().unwrap_or(0.0);
            if overall < threshold {
                bail!(
                    "ForkLaunch Score {:.0} is below the required threshold {:.0} — does not pass.",
                    overall,
                    threshold
                );
            }
        }

        Ok(())
    }
}
