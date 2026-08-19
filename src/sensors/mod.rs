//! Pluggable IO. Every sensor pushes `Observation`s; none of them decide what any of it means.

pub mod inbox;
pub mod mpris;
pub mod wayland;

use std::time::Instant;

#[derive(Clone, Debug)]
pub enum Observation {
    /// Focus moved to a different window.
    Focus { app_id: String, title: String },
    /// The focused window renamed itself — a new page, a new file, a new video.
    Title { app_id: String, title: String },
    Workspace { name: String },
    /// The user stopped touching the machine, or started again.
    Presence { present: bool },
    /// Something began or changed playing. `url` is the whole point: MPRIS hands over a YouTube
    /// watch id for free, so the transcript never needs a single pixel.
    Media { player: String, title: String, artist: String, url: String, playing: bool },
    /// Interoception — the body reporting on itself rather than on the desktop.
    ///
    /// Everything above is something happening *out there*. This is what it feels like in here:
    /// picked up, set down, drifted off the edge, and whatever a plugin decides to add through the
    /// inbox socket. A feeling gets an immediate bodily response and also lands in the next slice,
    /// so the mind reflects on it at its own pace rather than in the moment.
    Feeling(Feeling),
}

/// One interoceptive signal. `source` names the subsystem so the body's own feelings and a
/// plugin's read identically to the mind.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Feeling {
    pub source: String,
    pub state: String,
    #[serde(default)]
    pub detail: String,
    /// Name of the expression this warrants. Unknown or absent means mild curiosity.
    #[serde(default)]
    pub tone: Option<String>,
    #[serde(default = "middling")]
    pub intensity: f32,
    /// Seconds to hold the expression.
    #[serde(default = "a_while")]
    pub hold: f32,
    /// This feeling *is* the record of a reflex that already fired. It reaches the mind like any
    /// other, but it must never provoke a second reflex — that way lies an echo.
    #[serde(default, skip_deserializing)]
    pub reflective: bool,
}

fn middling() -> f32 {
    0.5
}

fn a_while() -> f32 {
    8.0
}

impl Feeling {
    /// A feeling the body raises about itself.
    pub fn own(state: &str, detail: &str, tone: &str, intensity: f32, hold: f32) -> Self {
        Self {
            source: "body".into(),
            state: state.into(),
            detail: detail.into(),
            tone: Some(tone.into()),
            intensity,
            hold,
            reflective: false,
        }
    }

    /// The body noticing what it just did without being asked.
    pub fn reflex_record(did: &str, because: &str) -> Self {
        Self {
            source: "reflex".into(),
            state: did.into(),
            detail: because.into(),
            tone: None,
            intensity: 0.0,
            hold: 0.0,
            reflective: true,
        }
    }
}

impl Observation {
    /// Identity for novelty checks. Two observations with the same key say nothing new.
    pub fn key(&self) -> String {
        match self {
            Observation::Focus { app_id, title } => format!("focus:{app_id}:{title}"),
            Observation::Title { app_id, title } => format!("title:{app_id}:{title}"),
            Observation::Workspace { name } => format!("ws:{name}"),
            Observation::Presence { present } => format!("presence:{present}"),
            Observation::Media { url, title, .. } => format!("media:{url}:{title}"),
            Observation::Feeling(f) => format!("feeling:{}:{}", f.source, f.state),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Observation::Focus { app_id, title } => format!("focus {app_id} — {}", clip(title, 34)),
            Observation::Title { app_id, title } => format!("retitle {app_id} — {}", clip(title, 30)),
            Observation::Workspace { name } => format!("workspace {name}"),
            Observation::Presence { present } => {
                format!("user {}", if *present { "back" } else { "away" })
            }
            Observation::Media { title, artist, playing, .. } => {
                format!("{} {} — {}", if *playing { "playing" } else { "paused" }, clip(artist, 16), clip(title, 26))
            }
            Observation::Feeling(f) if f.reflective => {
                // The cause is the line immediately above this one, so name it, do not restate it.
                let at = f.detail.trim_start_matches("you feel ").trim_start_matches("you found ");
                match at.split(['—', '(']).next().map(str::trim).filter(|s| !s.is_empty()) {
                    Some(at) => format!("you found yourself {} at {}", f.state, clip(at, 34)),
                    None => format!("you found yourself {}", f.state),
                }
            }
            Observation::Feeling(f) if f.detail.is_empty() => {
                format!("you feel {} ({})", f.state, f.source)
            }
            Observation::Feeling(f) => {
                format!("you feel {} ({}) — {}", f.state, f.source, clip(&f.detail, 40))
            }
        }
    }
}

pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
}

#[derive(Clone, Debug)]
pub struct Sensed {
    pub at: Instant,
    pub what: Observation,
}

/// Sensors write here; the attention loop drains it. Bounded, because a buddy that falls behind
/// should forget the middle of a burst, not grow without limit.
#[derive(Default)]
pub struct Bus {
    queue: Vec<Sensed>,
}

impl Bus {
    const CAP: usize = 256;

    pub fn push(&mut self, what: Observation) {
        if self.queue.len() >= Self::CAP {
            self.queue.remove(0);
        }
        self.queue.push(Sensed { at: Instant::now(), what });
    }

    pub fn drain(&mut self) -> Vec<Sensed> {
        std::mem::take(&mut self.queue)
    }
}
