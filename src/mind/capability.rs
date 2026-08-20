//! The four things a lilguy can do. Tool schemas out, typed intents back.

use crate::avatar::{Emotion, Gesture};
use serde::Deserialize;
use serde_json::{json, Value};

/// Where a lilguy can decide to be.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "target", rename_all = "lowercase")]
pub enum FocusTarget {
    /// The window whose app id or title contains `match`.
    Window { r#match: String },
    Pointer,
    /// A corner or edge, named rather than numbered so the model never guesses coordinates.
    Place { where_: Place },
    /// Leave the screen entirely and stop being visible.
    Away,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Centre,
}

impl Place {
    /// Fraction of the surface, so it survives any resolution.
    pub fn fraction(self) -> [f32; 2] {
        match self {
            Place::TopLeft => [0.10, 0.14],
            Place::TopRight => [0.90, 0.14],
            Place::BottomLeft => [0.10, 0.86],
            Place::BottomRight => [0.90, 0.86],
            Place::Centre => [0.50, 0.50],
        }
    }
}

/// What the model decided to do about a quantum.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    React { emotion: Emotion, intensity: f32, hold: f32 },
    Think { text: String, hold: f32 },
    Gesture { gesture: Gesture },
    Speak { text: String, to: Option<String> },
    Focus { target: FocusTarget, linger: f32 },
}

impl Intent {
    /// The closed set of body capabilities, named once. Five is the whole vocabulary; a sixth
    /// costs tokens on every turn for every guy and needs an argument in philosophy.md's terms.
    pub fn kind(&self) -> &'static str {
        match self {
            Intent::React { .. } => "react",
            Intent::Think { .. } => "think",
            Intent::Gesture { .. } => "gesture",
            Intent::Speak { .. } => "speak",
            Intent::Focus { .. } => "focus",
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Intent::React { emotion, intensity, .. } => {
                format!("react {} {:.0}%", emotion.name(), intensity * 100.0)
            }
            Intent::Think { text, .. } => format!("think \"{}\"", crate::sensors::clip(text, 40)),
            Intent::Gesture { gesture } => format!("gesture {}", gesture.name()),
            Intent::Speak { text, to: None } => {
                format!("speak \"{}\"", crate::sensors::clip(text, 40))
            }
            Intent::Speak { text, to: Some(who) } => {
                format!("speak to {who} \"{}\"", crate::sensors::clip(text, 40))
            }
            Intent::Focus { target, .. } => match target {
                FocusTarget::Window { r#match: m } => format!("focus window {m}"),
                FocusTarget::Pointer => "focus pointer".into(),
                FocusTarget::Place { where_ } => format!("focus {where_:?}"),
                FocusTarget::Away => "focus away".into(),
            },
        }
    }
}

/// Models routinely send numbers as strings. That is a JSON-typing slip, not a broken call, and
/// rejecting the whole thing over it throws away an intention that was perfectly clear.
fn lenient_f32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    use serde::de::Error;
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Number(n) => Ok(n.as_f64().unwrap_or(0.0) as f32),
        serde_json::Value::String(s) => s.trim().parse().map_err(D::Error::custom),
        other => Err(D::Error::custom(format!("expected a number, got {other}"))),
    }
}

#[derive(Deserialize)]
struct ReactArgs {
    emotion: String,
    #[serde(default = "half", deserialize_with = "lenient_f32")]
    intensity: f32,
    #[serde(default = "default_hold", deserialize_with = "lenient_f32")]
    hold: f32,
}

#[derive(Deserialize)]
struct GestureArgs {
    gesture: String,
}

#[derive(Deserialize)]
struct SpeakArgs {
    text: String,
    #[serde(default)]
    to: Option<String>,
}

#[derive(Deserialize)]
struct ThinkArgs {
    text: String,
    #[serde(default = "default_linger", deserialize_with = "lenient_f32")]
    hold: f32,
}

#[derive(Deserialize)]
struct FocusArgs {
    target: String,
    #[serde(default)]
    r#match: Option<String>,
    #[serde(default)]
    place: Option<Place>,
    #[serde(default = "default_linger", deserialize_with = "lenient_f32")]
    linger: f32,
}

fn half() -> f32 {
    0.5
}
fn default_hold() -> f32 {
    6.0
}
fn default_linger() -> f32 {
    20.0
}

/// Words above which an utterance is prompt leakage rather than a remark.
const SPEAK_WORD_CAP: usize = 25;

/// Rejects an utterance that is structurally not a remark. Small models leak their tool-calling
/// preamble into `speak`, and the user hears it; length and JSON debris catch every case seen.
fn vet_speech(text: &str) -> Result<String, String> {
    let text = text.trim().trim_matches('"').trim().to_string();
    if text.is_empty() {
        return Err("empty".into());
    }
    let words = text.split_whitespace().count();
    if words > SPEAK_WORD_CAP {
        return Err(format!("{words} words, cap is {SPEAK_WORD_CAP}"));
    }
    vet_leakage(&text)?;
    Ok(text)
}

/// Structural markers of machinery escaping into something a person sees or hears.
fn vet_leakage(text: &str) -> Result<(), String> {
    let caught = |why: String| {
        crate::telemetry::count(
            crate::telemetry::name::REFUSALS,
            &[("reason", "speech vetted")],
        );
        Err(why)
    };
    for debris in ['{', '}', '[', ']', '<', '>'] {
        if text.contains(debris) {
            return caught(format!("contains {debris:?}, looks like markup"));
        }
    }
    let lower = text.to_lowercase();
    for tell in ["given the following", "\"name\"", "arguments", "parameters", "json schema"] {
        if lower.contains(tell) {
            return caught(format!("contains {tell:?}, looks like prompt leakage"));
        }
    }
    Ok(())
}

/// Parses one tool call. A malformed call is reported rather than dropped, so the log says why.
pub fn parse(name: &str, arguments: &str) -> Result<Intent, String> {
    let args = if arguments.trim().is_empty() { "{}" } else { arguments };
    match name {
        "react" => {
            let a: ReactArgs = serde_json::from_str(args).map_err(|e| e.to_string())?;
            let emotion = Emotion::from_name(&a.emotion)
                .ok_or_else(|| format!("unknown emotion {:?}", a.emotion))?;
            Ok(Intent::React {
                emotion,
                intensity: a.intensity.clamp(0.0, 1.0),
                hold: a.hold.clamp(1.0, 60.0),
            })
        }
        "gesture" => {
            let a: GestureArgs = serde_json::from_str(args).map_err(|e| e.to_string())?;
            let gesture = Gesture::from_name(&a.gesture)
                .ok_or_else(|| format!("unknown gesture {:?}", a.gesture))?;
            Ok(Intent::Gesture { gesture })
        }
        "think" => {
            let a: ThinkArgs = serde_json::from_str(args).map_err(|e| e.to_string())?;
            // A thought is silent, so it is vetted for leakage but not for length.
            let text = a.text.trim().trim_matches('"').trim().to_string();
            if text.is_empty() {
                return Err("empty".into());
            }
            vet_leakage(&text)?;
            Ok(Intent::Think { text, hold: a.hold.clamp(12.0, 300.0) })
        }
        "speak" => {
            let a: SpeakArgs = serde_json::from_str(args).map_err(|e| e.to_string())?;
            let to = a.to.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            // A name reaches a person's ears too, so it is vetted like anything else spoken.
            if let Some(name) = to.as_deref() {
                vet_leakage(name)?;
            }
            Ok(Intent::Speak { text: vet_speech(&a.text)?, to })
        }
        "focus" => {
            let a: FocusArgs = serde_json::from_str(args).map_err(|e| e.to_string())?;
            let target = match a.target.as_str() {
                "window" => FocusTarget::Window {
                    r#match: a.r#match.ok_or("target=window needs a match")?,
                },
                "pointer" => FocusTarget::Pointer,
                "place" => FocusTarget::Place { where_: a.place.ok_or("target=place needs a place")? },
                "away" => FocusTarget::Away,
                other => return Err(format!("unknown target {other:?}")),
            };
            Ok(Intent::Focus { target, linger: a.linger.clamp(2.0, 600.0) })
        }
        other => Err(format!("unknown tool {other:?}")),
    }
}

/// Every verb a body has, for the sutras to name without hardcoding a second list.
pub fn names() -> Vec<&'static str> {
    vec!["react", "gesture", "think", "speak", "focus"]
}

/// The tool schemas sent on every turn.
pub fn schemas() -> Value {
    json!([
        tool("react", "Show a passing feeling on your face. Cheap and quiet — this is the right \
             response to most things. Does not interrupt the user.", json!({
            "type": "object",
            "properties": {
                "emotion": { "type": "string", "enum": Emotion::NAMES },
                "intensity": { "type": "number", "minimum": 0, "maximum": 1 },
                "hold": { "type": "number", "description": "seconds to hold the expression" }
            },
            "required": ["emotion"]
        })),
        tool("gesture", "Move your whole body deliberately. More visible than a reaction, so \
             use it when something genuinely warrants being noticed.", json!({
            "type": "object",
            "properties": {
                "gesture": { "type": "string", "enum": Gesture::NAMES }
            },
            "required": ["gesture"]
        })),
        tool("think", "Have a visible thought. It appears over your head in writing and nobody \
             hears it, so it interrupts nothing and may stay up for a long while. This is how you \
             say something without saying it.", json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "the thought, a phrase or a sentence" },
                "hold": { "type": "number", "description": "seconds to leave it up; 20 to 60 reads well, under 12 is too brief to finish reading" }
            },
            "required": ["text"]
        })),
        tool("speak", "Say something out loud. This interrupts. Keep it under fifteen words and \
             use it rarely — silence is almost always better. Answering somebody who spoke to you \
             is the exception: that is a conversation, and it is what a voice is for.", json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "what to say, one short sentence" },
                "to": { "type": "string", "description": "the name of whoever you are answering, if you are answering somebody. Everyone hears it either way." }
            },
            "required": ["text"]
        })),
        tool("focus", "Go somewhere and pay attention to it. Use `window` with a match on an \
             application id or title, `pointer` to hover near the cursor, `place` for a corner, \
             or `away` to leave the screen.", json!({
            "type": "object",
            "properties": {
                "target": { "type": "string", "enum": ["window", "pointer", "place", "away"] },
                "match": { "type": "string", "description": "for target=window: part of the app id or title" },
                "place": { "type": "string", "enum": [
                    "top_left", "top_right", "bottom_left", "bottom_right", "centre"
                ]},
                "linger": { "type": "number", "description": "seconds to stay before drifting off" }
            },
            "required": ["target"]
        })),
    ])
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_rejects_prompt_leakage() {
        // The exact shape a small model leaked into `speak` in the wild.
        let leak = "Given the following functions, please respond with a JSON for a function \
                    call with its proper arguments that best answers the given prompt.";
        assert!(parse("speak", &format!(r#"{{"text": "{leak}"}}"#)).is_err());
    }

    #[test]
    fn speech_rejects_markup_and_rambling() {
        assert!(parse("speak", r#"{"text": "{\"name\": \"react\"}"}"#).is_err());
        let long = vec!["word"; SPEAK_WORD_CAP + 1].join(" ");
        assert!(parse("speak", &format!(r#"{{"text": "{long}"}}"#)).is_err());
        assert!(parse("speak", r#"{"text": "   "}"#).is_err());
    }

    #[test]
    fn speech_accepts_a_remark() {
        let ok = parse("speak", r#"{"text": "  \"that video is long\"  "}"#).unwrap();
        assert_eq!(ok, Intent::Speak { text: "that video is long".into(), to: None });
    }

    #[test]
    fn speech_may_be_addressed_to_one_of_the_others() {
        let ok = parse("speak", r#"{"text": "you are wrong", "to": " SpongeBob "}"#).unwrap();
        assert_eq!(
            ok,
            Intent::Speak { text: "you are wrong".into(), to: Some("SpongeBob".into()) }
        );
        // An empty name is nobody, not somebody called "".
        let room = parse("speak", r#"{"text": "hello", "to": "  "}"#).unwrap();
        assert_eq!(room, Intent::Speak { text: "hello".into(), to: None });
    }

    #[test]
    fn unknown_names_are_reported_not_dropped() {
        assert!(parse("react", r#"{"emotion": "smug"}"#).is_err());
        assert!(parse("gesture", r#"{"gesture": "moonwalk"}"#).is_err());
        assert!(parse("focus", r#"{"target": "window"}"#).is_err());
        assert!(parse("teleport", "{}").is_err());
    }
}
