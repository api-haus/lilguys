//! Pluggable TTS. Every engine is a command line, so piper, kokoro, XTTS and espeak-ng are one thing.

use crate::config::{Engine, Voice as VoiceConfig};
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;

/// Whether a lilguy is mid-sentence, so the mouth can move and a second utterance can be refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Silent,
    Speaking,
}

pub struct Voice {
    tx: Option<mpsc::Sender<String>>,
    queued: usize,
    limit: usize,
}

impl Voice {
    /// A disabled or unknown engine yields a voice that silently accepts and drops everything, so
    /// no caller needs to know whether speech is configured.
    pub fn new(config: &VoiceConfig, done: calloop::channel::Sender<State>) -> Self {
        if !config.enabled {
            return Self { tx: None, queued: 0, limit: 0 };
        }
        let Some(engine) = config.engines.get(&config.engine).cloned() else {
            eprintln!("voice engine '{}' has no [voice.engines.{}] entry", config.engine, config.engine);
            return Self { tx: None, queued: 0, limit: 0 };
        };
        if engine.synth.is_empty() {
            eprintln!("voice engine '{}' has an empty synth command", config.engine);
            return Self { tx: None, queued: 0, limit: 0 };
        }

        let (tx, rx) = mpsc::channel::<String>();
        let cfg = config.clone();
        std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || {
                while let Ok(text) = rx.recv() {
                    let _ = done.send(State::Speaking);
                    if let Err(e) = utter(&engine, &cfg, &text) {
                        eprintln!("voice: {e:#}");
                    }
                    let _ = done.send(State::Silent);
                }
            })
            .ok();

        Self { tx: Some(tx), queued: 0, limit: config.queue_limit.max(1) }
    }

    pub fn available(&self) -> bool {
        self.tx.is_some()
    }

    /// Refuses rather than queues once the limit is reached — a buddy talking over itself is worse
    /// than one that missed a line.
    pub fn say(&mut self, text: &str) -> bool {
        let Some(tx) = self.tx.as_ref() else { return false };
        if self.queued >= self.limit {
            return false;
        }
        if tx.send(text.to_string()).is_err() {
            return false;
        }
        self.queued += 1;
        true
    }

    pub fn finished_one(&mut self) {
        self.queued = self.queued.saturating_sub(1);
    }
}

fn utter(engine: &Engine, config: &VoiceConfig, text: &str) -> Result<()> {
    let out = std::env::temp_dir().join(format!("lilguys-{}.wav", std::process::id()));
    let out_str = out.to_string_lossy().to_string();
    // A model path in a config file is written with a tilde; exec never expands one.
    let voice = match config.voice.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest).to_string_lossy().into_owned()),
        None => None,
    }
    .unwrap_or_else(|| config.voice.clone());
    let subst = |arg: &String| -> String {
        arg.replace("{text}", text)
            .replace("{voice}", &voice)
            .replace("{speed}", &format!("{:.2}", 1.0 / config.speed.max(0.05)))
            .replace("{out}", &out_str)
    };

    let args: Vec<String> = engine.synth.iter().map(subst).collect();
    let wants_stdin = !engine.synth.iter().any(|a| a.contains("{text}"));
    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .stdin(if wants_stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("could not start '{}'", args[0]))?;
    if wants_stdin {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
    }
    let status = child.wait()?;
    if !status.success() {
        bail!("{} exited with {status}", args[0]);
    }

    if engine.play.is_empty() {
        return Ok(());
    }
    let play: Vec<String> = engine.play.iter().map(subst).collect();
    let status = Command::new(&play[0])
        .args(&play[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("could not start '{}'", play[0]))?;
    if !status.success() {
        bail!("{} exited with {status}", play[0]);
    }
    let _ = std::fs::remove_file(&out);
    Ok(())
}
