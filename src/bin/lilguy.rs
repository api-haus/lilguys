//! `lilguy` — the command a person or, more often, their coding agent uses to run the daemon.
//!
//! Non-interactive, idempotent, `--json` on everything: the first user of this is another program.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lilguysd::{config::Config, doctor, sensors::inbox, service, setup, voice};
use serde_json::json;
use std::io::{IsTerminal, Write};

#[derive(Parser)]
#[command(name = "lilguy", version, about = "run and diagnose the lilguys daemon")]
struct Cli {
    /// Machine-readable output, so a caller can branch on the result instead of parsing prose.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Work out what this machine has, fix what is missing, and end with them on screen.
    Setup {
        /// Answer yes to everything, including downloads.
        #[arg(long, short = 'y')]
        yes: bool,
        /// Use this provider rather than working one out.
        #[arg(long)]
        provider: Option<String>,
        /// Use this model rather than working one out.
        #[arg(long)]
        model: Option<String>,
        /// Use this voice: "engine:voice", an engine, or a voice.
        #[arg(long)]
        voice: Option<String>,
        /// Install the systemd user unit, so they come back after a reboot.
        #[arg(long)]
        service: bool,
        /// Leave systemd alone; `lilguy start` runs them as an ordinary process.
        #[arg(long, conflicts_with = "service")]
        no_service: bool,
    },
    /// Check everything, one line per check, and say how to fix what is wrong.
    Doctor,
    /// Start the daemon — the systemd unit if there is one, a detached process if not.
    Start,
    /// Stop the daemon.
    Stop,
    /// Stop and start again — the only way a changed config reaches a running daemon.
    Restart,
    /// Whether anybody is running, and who is on screen.
    Status,
    /// Say something to them. They may answer, and they may not.
    Say {
        /// What to say.
        text: String,
        /// One of them by name. Everybody hears it either way.
        #[arg(long)]
        to: Option<String>,
    },
    /// Where a model might come from, and which one they think with.
    Provider {
        #[command(subcommand)]
        what: Provider,
    },
    /// The model behind the current provider.
    Model {
        #[command(subcommand)]
        what: Model,
    },
    /// What they sound like.
    Voice {
        #[command(subcommand)]
        what: VoiceCmd,
    },
}

#[derive(Subcommand)]
enum Provider {
    /// Everything configured and every local endpoint that answers, with its models.
    List,
    /// Switch. Refuses a provider whose model cannot do native tool calls.
    Use { name: String },
}

#[derive(Subcommand)]
enum Model {
    /// What the current provider says it can run.
    List,
    /// Download one through the configured backend.
    Pull { name: String },
    /// Switch. Refuses a model that cannot do native tool calls.
    Use { name: String },
}

#[derive(Subcommand)]
enum VoiceCmd {
    /// Engines this machine has, and the piper voices already downloaded.
    List,
    /// Switch. "engine:voice", an engine, or a voice.
    Use { spec: String },
    /// Say a sentence, so the choice is audible rather than theoretical.
    Test {
        /// A voice to try without switching to it.
        #[arg(long)]
        spec: Option<String>,
        /// What to say.
        #[arg(default_value = "Hello. This is what I sound like.")]
        text: String,
    },
    /// Download a piper voice model, about 60 MB, from the piper-voices repository.
    Fetch { name: String },
}

fn main() {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            match cli.json {
                true => println!("{}", json!({ "ok": false, "error": format!("{e:#}") })),
                false => eprintln!("lilguy: {e:#}"),
            }
            std::process::exit(1);
        }
    }
}

/// `false` is a run that worked and found something wrong; `Err` is a run that could not happen.
fn run(cli: &Cli) -> Result<bool> {
    match &cli.command {
        Command::Setup { yes, provider, model, voice, service, no_service } => wizard(
            cli.json,
            setup::Options {
                yes: *yes,
                provider: provider.clone(),
                model: model.clone(),
                voice: voice.clone(),
                service: match (service, no_service) {
                    (true, _) => Some(true),
                    (_, true) => Some(false),
                    _ => None,
                },
            },
        ),
        Command::Doctor => Ok(doctor(cli.json)),
        Command::Start => did(cli.json, service::start()?),
        Command::Stop => did(cli.json, service::stop()?),
        Command::Restart => did(cli.json, service::restart()?),
        Command::Status => status(cli.json),
        Command::Say { text, to } => say(cli.json, text, to.as_deref()),
        Command::Provider { what } => provider(cli.json, what),
        Command::Model { what } => model(cli.json, what),
        Command::Voice { what } => voice_cmd(cli.json, what),
    }
}

/// A question is answered by whoever is there to answer it: the flag, then the terminal, and
/// otherwise nobody — in which case setup stops and says what it needed, rather than guessing.
fn wizard(as_json: bool, opts: setup::Options) -> Result<bool> {
    let interactive = std::io::stdin().is_terminal() && !opts.yes;
    let yes = opts.yes;
    let mut ask = |question: &str| -> bool {
        if yes {
            return true;
        }
        if !interactive {
            return false;
        }
        eprint!("{question} [Y/n] ");
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            return false;
        }
        matches!(answer.trim().to_lowercase().as_str(), "" | "y" | "yes")
    };
    let mut say = |step: &setup::Step| {
        if !as_json {
            println!("{}", step.line());
        }
    };
    let steps = setup::run(&opts, &mut ask, &mut say)?;
    let blocked: Vec<&setup::Step> =
        steps.iter().filter(|s| s.did == setup::Did::Blocked).collect();
    if as_json {
        println!(
            "{}",
            json!({
                "ok": blocked.is_empty(),
                "steps": steps.iter().map(setup::Step::json).collect::<Vec<_>>(),
                "question": blocked.first().and_then(|s| s.question.clone()),
            })
        );
    } else if let Some(step) = blocked.first() {
        println!("\nneeded: {}", step.question.clone().unwrap_or_default());
    }
    Ok(blocked.is_empty())
}

fn provider(as_json: bool, what: &Provider) -> Result<bool> {
    let (config, _) = Config::load()?;
    match what {
        Provider::Use { name } => did(as_json, setup::use_provider(&config, name)?),
        Provider::List => {
            let found = setup::endpoints(&config);
            match as_json {
                true => println!(
                    "{}",
                    json!({ "ok": true, "providers": found.iter().map(|e| json!({
                        "name": e.name, "url": e.url, "configured": e.configured,
                        "current": e.current, "reachable": e.reachable(),
                        "models": e.models, "error": e.error,
                    })).collect::<Vec<_>>() })
                ),
                false => {
                    for e in &found {
                        println!("{}", e.line());
                    }
                    println!("\n* in use   · configured   + found on this machine");
                }
            }
            Ok(found.iter().any(|e| e.current && e.reachable()))
        }
    }
}

fn model(as_json: bool, what: &Model) -> Result<bool> {
    let (config, _) = Config::load()?;
    match what {
        Model::Use { name } => did(as_json, setup::use_model(&config, name)?),
        Model::Pull { name } => did(as_json, setup::pull_model(&config, name)?),
        Model::List => {
            let current = config.provider()?;
            let found = setup::endpoints(&config);
            let mine = found.iter().find(|e| e.current);
            let models: Vec<String> = mine.map(|e| e.models.clone()).unwrap_or_default();
            match as_json {
                true => println!(
                    "{}",
                    json!({ "ok": true, "provider": config.mind.provider,
                            "model": current.model, "models": models })
                ),
                false => {
                    for m in &models {
                        println!("{} {m}", if *m == current.model { "*" } else { "·" });
                    }
                }
            }
            Ok(models.contains(&current.model))
        }
    }
}

fn voice_cmd(as_json: bool, what: &VoiceCmd) -> Result<bool> {
    let (config, _) = Config::load()?;
    match what {
        VoiceCmd::Use { spec } => did(as_json, setup::use_voice(&config, spec)?),
        VoiceCmd::Fetch { name } => {
            let path = setup::fetch_piper_voice(name)?;
            did(as_json, format!("{}", path.display()))
        }
        VoiceCmd::Test { spec, text } => {
            let sound = voice::speak_once(&config.voice, spec.as_deref(), text)?;
            did(as_json, format!("{sound} said it"))
        }
        VoiceCmd::List => {
            let engines = setup::voices(&config);
            let models = setup::piper_voices();
            match as_json {
                true => println!(
                    "{}",
                    json!({ "ok": true,
                            "engines": engines.iter().map(|e| json!({
                                "engine": e.engine, "command": e.command,
                                "present": e.present, "current": e.current })).collect::<Vec<_>>(),
                            "piper_voices": models.iter().map(|p| p.display().to_string())
                                                  .collect::<Vec<_>>() })
                ),
                false => {
                    for e in &engines {
                        println!("{}", e.line());
                    }
                    for m in &models {
                        println!("  voice {}", m.display());
                    }
                }
            }
            Ok(engines.iter().any(|e| e.current && e.present))
        }
    }
}

fn did(as_json: bool, what: String) -> Result<bool> {
    match as_json {
        true => println!("{}", json!({ "ok": true, "did": what })),
        false => println!("{what}"),
    }
    Ok(true)
}

fn doctor(as_json: bool) -> bool {
    // Human output streams, because the tool-call probe is a real model turn and takes its time.
    let mut print = |c: &doctor::Check| {
        if !as_json {
            println!("{}", c.line());
            if let (doctor::Status::Ok, _) | (_, None) = (c.status, c.fix.as_ref()) {
                return;
            }
            println!("      → {}", c.fix.clone().unwrap_or_default());
        }
    };
    let report = doctor::run(&mut print);
    if as_json {
        println!("{}", report.json());
    }
    report.ok()
}

fn status(as_json: bool) -> Result<bool> {
    let (config, from) = Config::load()?;
    let dir = from.as_deref().and_then(std::path::Path::parent);
    let roster = config.roster(dir).unwrap_or_default();
    let names: Vec<String> = roster
        .iter()
        .map(|(g, c)| g.name.clone().unwrap_or_else(|| c.name.clone()))
        .collect();
    let pids = service::pids();
    let unit = service::unit();

    let meter = meter_snapshot(&config);

    if as_json {
        println!(
            "{}",
            json!({
                "ok": true,
                "running": !pids.is_empty(),
                "pids": pids,
                "unit": match unit {
                    service::Unit::Absent => json!(null),
                    service::Unit::Installed { active, enabled } =>
                        json!({ "active": active, "enabled": enabled }),
                },
                "roster": names,
                "config": from.map(|p| p.display().to_string()),
                "provider": config.mind.provider,
                "telemetry": meter,
            })
        );
        return Ok(!pids.is_empty());
    }

    println!("{}", service::status_check().line());
    println!("{:<5} {:<10} {}", "ok", "roster", names.join(", "));
    for line in cost_lines(&meter) {
        println!("{:<5} {}", "ok", line);
    }
    Ok(!pids.is_empty())
}

/// The daemon's own counters, written beside the logs. Absent when telemetry is off or nobody has
/// run long enough to write one.
fn meter_snapshot(config: &Config) -> serde_json::Value {
    let dir = config.log.dir.clone().unwrap_or_else(lilguysd::log::default_dir);
    std::fs::read_to_string(dir.join("state.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!(null))
}

/// What it cost, one line each, in the same two columns as every other check.
fn cost_lines(meter: &serde_json::Value) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(m) = meter.as_object() else { return lines };
    let pairs = |v: Option<&serde_json::Value>| -> String {
        v.and_then(|v| v.as_object())
            .map(|o| {
                o.iter()
                    .filter(|(_, n)| n.as_u64().unwrap_or(0) > 0)
                    .map(|(k, n)| format!("{k} {n}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default()
    };

    if let Some(f) = m.get("frames").and_then(|f| f.as_object()) {
        let hz = f.get("tick_hz").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let render = f.get("render").and_then(|r| r.get("p99_ms")).and_then(|v| v.as_f64());
        let gap = f.get("interval").and_then(|r| r.get("p99_ms")).and_then(|v| v.as_f64());
        let submitted = f.get("submitted").and_then(serde_json::Value::as_u64).unwrap_or(0);
        lines.push(format!(
            "{:<10} {submitted} at {hz:.0} Hz · render p99 {:.1} ms · gap p99 {:.1} ms",
            "frames",
            render.unwrap_or(0.0),
            gap.unwrap_or(0.0),
        ));
    }
    for (who, per) in m.get("guys").and_then(|g| g.as_object()).into_iter().flatten() {
        lines.push(format!("{:<10} {}", who, pairs(Some(per))));
    }
    for field in ["gate", "refusals", "drops"] {
        let text = pairs(m.get(field));
        if !text.is_empty() {
            lines.push(format!("{field:<10} {text}"));
        }
    }
    if let Some(up) = m.get("uptime_s").and_then(serde_json::Value::as_u64) {
        lines.push(format!("{:<10} {}h {}m", "uptime", up / 3600, (up % 3600) / 60));
    }
    lines
}

/// A message goes in through the same socket a feeding mechanic uses, because there is one stream.
fn say(as_json: bool, text: &str, to: Option<&str>) -> Result<bool> {
    let path = inbox::socket_path();
    let mut sock = std::os::unix::net::UnixStream::connect(&path)
        .with_context(|| format!("nobody is listening on {}", path.display()))?;
    let line = match to {
        Some(name) => json!({ "text": text, "to": name }),
        None => json!({ "text": text }),
    };
    writeln!(sock, "{line}").context("write to the inbox")?;
    sock.flush().ok();

    match as_json {
        true => println!("{}", json!({ "ok": true, "said": text, "to": to })),
        false => println!("said"),
    }
    Ok(true)
}
