//! Full transcript of every model turn, so misbehaviour is diagnosable after the fact.

use crate::config::Logging;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static SINK: OnceLock<Option<Mutex<Sink>>> = OnceLock::new();

struct Sink {
    turns: File,
    events: File,
}

/// Opens the log files once. Everything after this is fire-and-forget: a full disk must never
/// take the buddy down.
pub fn init(config: &Logging) -> Option<PathBuf> {
    let dir: PathBuf = match config.enabled {
        false => return None,
        true => config.dir.clone().unwrap_or_else(default_dir),
    };
    if std::fs::create_dir_all(&dir).is_err() {
        eprintln!("logging disabled: cannot create {}", dir.display());
        return None;
    }
    let open = |name: &str| {
        OpenOptions::new().create(true).append(true).open(dir.join(name))
    };
    let (Ok(turns), Ok(events)) = (open("turns.jsonl"), open("events.jsonl")) else {
        eprintln!("logging disabled: cannot open files in {}", dir.display());
        return None;
    };
    SINK.set(Some(Mutex::new(Sink { turns, events }))).ok();
    Some(dir)
}

fn default_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("lilguys")
}


fn stamp() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn write(pick: fn(&mut Sink) -> &mut File, mut value: Value) {
    let Some(Some(lock)) = SINK.get() else { return };
    let Ok(mut sink) = lock.lock() else { return };
    value["at"] = json!(stamp());
    let file = pick(&mut sink);
    let _ = writeln!(file, "{value}");
    let _ = file.flush();
}

/// One line per sensed thing the gate ruled on, whatever the ruling.
pub fn event(verdict: &str, text: &str) {
    write(|s| &mut s.events, json!({ "kind": "gate", "verdict": verdict, "text": text }));
}

pub fn note(kind: &str, detail: &str) {
    write(|s| &mut s.events, json!({ "kind": kind, "detail": detail }));
}

/// The whole of one model turn: what went in, what came back verbatim, what was made of it.
#[allow(clippy::too_many_arguments)]
pub fn turn(
    prompt: &str,
    raw_content: &str,
    raw_calls: &Value,
    intents: &[String],
    rejected: &[String],
    tokens: usize,
    compacted: bool,
    error: Option<&str>,
    elapsed_ms: u128,
) {
    write(
        |s| &mut s.turns,
        json!({
            "prompt": prompt,
            "content": raw_content,
            "tool_calls": raw_calls,
            "intents": intents,
            "rejected": rejected,
            "tokens": tokens,
            "compacted": compacted,
            "error": error,
            "elapsed_ms": elapsed_ms,
        }),
    );
}
