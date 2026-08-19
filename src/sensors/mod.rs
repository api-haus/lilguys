//! Pluggable IO. Every sensor pushes `Observation`s; none of them decide what any of it means.

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
        }
    }
}

fn clip(s: &str, n: usize) -> String {
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
