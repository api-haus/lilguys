//! `lilguy` — the command a person or, more often, their coding agent uses to run the daemon.
//!
//! Non-interactive, idempotent, `--json` on everything: the first user of this is another program.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lilguysd::{config::Config, doctor, sensors::inbox, service};
use serde_json::json;
use std::io::Write;

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
    /// Check everything, one line per check, and say how to fix what is wrong.
    Doctor,
    /// Start the daemon — the systemd unit if there is one, a detached process if not.
    Start,
    /// Stop the daemon.
    Stop,
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
        Command::Doctor => Ok(doctor(cli.json)),
        Command::Start => did(cli.json, service::start()?),
        Command::Stop => did(cli.json, service::stop()?),
        Command::Status => status(cli.json),
        Command::Say { text, to } => say(cli.json, text, to.as_deref()),
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
            })
        );
        return Ok(!pids.is_empty());
    }

    println!("{}", service::status_check().line());
    println!("{:<5} {:<10} {}", "ok", "roster", names.join(", "));
    Ok(!pids.is_empty())
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
