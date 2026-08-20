//! One member of the roster: a body, a face, a gate and a mind of its own.

use crate::attention::{Attention, Verdict};
use crate::avatar::{graybox::Graybox, Avatar, Drive, Emotion, Pose};
use crate::config::{Character, Config, Guy as GuyConfig, Skin};
use crate::locomotion::Body;
use crate::mind::capability::Intent;
use crate::mind::{Quantiser, Reaction, ToMind};
use crate::sensors::{Bus, Observation};
use crate::voice::Voice;
use std::sync::mpsc::Sender;
use std::time::Instant;

pub struct Guy {
    pub name: String,
    pub character: String,
    pub size: f32,
    pub skin: Skin,

    pub body: Body,
    pub pose: Pose,
    pub avatar: Box<dyn Avatar>,
    /// Each keeps its own novelty and dwell state, so one may notice what another let pass.
    pub attention: Attention,
    pub quantiser: Quantiser,
    pub voice: Voice,
    /// Observations addressed to this one alone — its own feelings, and what the others did.
    pub bus: Bus,

    pub to_mind: Option<Sender<ToMind>>,
    pub thought: Option<(Instant, f32, String)>,
    pub last_reaction: Option<Reaction>,
    pub mind_error: Option<String>,
    pub turns: u32,
    pub input_region: Option<[i32; 4]>,
}

impl Guy {
    pub fn new(
        cfg: &Config, guy: &GuyConfig, character: &Character, bounds: [f32; 2],
        voice: Voice, to_mind: Option<Sender<ToMind>>,
    ) -> Self {
        let size = guy.size.or(character.size).unwrap_or(cfg.buddy.size);
        let name = guy.name.clone().unwrap_or_else(|| character.name.clone());
        Self {
            name,
            character: guy.character.clone(),
            size,
            skin: character.skin.unwrap_or_default(),
            body: Body::new(cfg.motion.clone(), size, bounds),
            pose: Pose::default(),
            avatar: Box::new(Graybox::annotated(cfg.debug.rig)),
            attention: Attention::new(cfg.into()),
            quantiser: Quantiser::default(),
            voice,
            bus: Bus::default(),
            to_mind,
            thought: None,
            last_reaction: None,
            mind_error: None,
            turns: 0,
            input_region: None,
        }
    }

    /// The system prompt for this one: the shared sutras with its own persona in the last layer,
    /// and the engine's situational facts filled in.
    pub fn system_prompt(
        cfg: &Config, character: &Character, name: &str, others: &[String],
        config_dir: Option<&std::path::Path>,
    ) -> String {
        let mut prompt = cfg.prompt.clone();
        if let Some(layer) = prompt.texts.get_mut("persona") {
            layer.text = character.persona.clone();
            layer.file = None;
        }
        prompt.assemble(&Self::facts(cfg, character, name, others), config_dir)
    }

    /// What the engine knows that does not change: who you are, who else is here, what you can do.
    /// Anything that changes goes in the stream, or the system prompt stops being byte-stable.
    pub fn facts(
        cfg: &Config, character: &Character, name: &str, others: &[String],
    ) -> crate::config::Facts {
        let mut facts = crate::config::Facts::default();
        facts
            .set("name", name)
            .set("others", join_names(others))
            .set("cast", join_names(&[&[name.to_string()][..], others].concat()))
            .set("capabilities", crate::mind::capability::names().join(", "))
            .set("emotions", crate::avatar::Emotion::NAMES.join(", "))
            .set("gestures", crate::avatar::Gesture::NAMES.join(", "))
            .set("size", format!("{:.0} pixels tall", character.size.unwrap_or(cfg.buddy.size)));
        facts
    }

    /// Everything sensed since the last tick, ruled on and bucketed for this one's own mind.
    pub fn sense(&mut self, shared: &[crate::sensors::Sensed], now: Instant) -> Vec<String> {
        for feeling in self.body.drain_feelings() {
            self.bus.push(Observation::Feeling(feeling));
        }
        let mut sensed = self.bus.drain();
        sensed.extend_from_slice(shared);
        sensed.sort_by_key(|s| s.at);

        let mut lines = Vec::new();
        if sensed.is_empty() && self.attention.waiting() == 0 {
            return lines;
        }
        for (verdict, what) in self.attention.consider(sensed, now) {
            let already_reflexive = matches!(&what, Observation::Feeling(f) if f.reflective);
            if verdict == Verdict::Emote && !already_reflexive {
                let (emotion, intensity, hold) = crate::app::reflex(&what);
                self.body.reflex(emotion, intensity, hold, &what.summary_for(&self.name));
            }
            if what.addressed_to(&self.name) {
                self.quantiser.urge();
            }
            self.quantiser.observe(what.summary_for(&self.name));
        }
        for e in self.attention.log.iter().take(self.attention.fresh()) {
            lines.push(format!("[{}] {} · {}", e.verdict.tag(), self.name, e.text));
            crate::log::event(e.verdict.tag(), &format!("{}: {}", self.name, e.text));
        }
        lines
    }

    pub fn drive(&self) -> Drive {
        Drive {
            speed: self.body.speed(),
            heading: self.body.heading(),
            bob_phase: self.body.bob_phase(),
            gesture: self.body.gesture(),
            speaking: self.body.speaking,
        }
    }

    /// What one particular onlooker learns about an action this one just took. Doing something in
    /// front of somebody is an event in their world, which is the whole of how a roster is social.
    ///
    /// Being addressed is not overhearing, so the same sentence reads differently to the one it
    /// was aimed at.
    pub fn witnessed_by(&self, intent: &Intent, listener: &str) -> Option<Observation> {
        let (state, detail) = match intent {
            Intent::Speak { text, to: None } => ("said something".into(), format!("\"{text}\"")),
            Intent::Speak { text, to: Some(who) } if who.eq_ignore_ascii_case(listener) => {
                ("said to you".into(), format!("\"{text}\""))
            }
            Intent::Speak { text, to: Some(who) } => {
                (format!("said to {who}"), format!("\"{text}\""))
            }
            Intent::Think { text, .. } => ("is thinking".into(), format!("\"{text}\"")),
            Intent::Gesture { gesture } => ("made a gesture".into(), gesture.name().to_string()),
            // A face is noticed only when it is a strong one; a flicker is nobody else's business.
            Intent::React { emotion, intensity, .. } if *intensity >= 0.5 => {
                (format!("looks {}", emotion.name()), String::new())
            }
            _ => return None,
        };
        Some(Observation::Witnessed { who: self.name.clone(), did: state, detail })
    }

    pub fn expression_of(&self, emotion: Emotion) -> &'static str {
        emotion.name()
    }
}

/// "a", "a and b", "a, b and c" — a list a character can read aloud.
fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
