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
    Gesture { gesture: Gesture },
    Speak { text: String },
    Focus { target: FocusTarget, linger: f32 },
}

impl Intent {
    pub fn summary(&self) -> String {
        match self {
            Intent::React { emotion, intensity, .. } => {
                format!("react {} {:.0}%", emotion.name(), intensity * 100.0)
            }
            Intent::Gesture { gesture } => format!("gesture {}", gesture.name()),
            Intent::Speak { text } => format!("speak \"{}\"", crate::sensors::clip(text, 40)),
            Intent::Focus { target, .. } => match target {
                FocusTarget::Window { r#match: m } => format!("focus window {m}"),
                FocusTarget::Pointer => "focus pointer".into(),
                FocusTarget::Place { where_ } => format!("focus {where_:?}"),
                FocusTarget::Away => "focus away".into(),
            },
        }
    }
}

#[derive(Deserialize)]
struct ReactArgs {
    emotion: String,
    #[serde(default = "half")]
    intensity: f32,
    #[serde(default = "default_hold")]
    hold: f32,
}

#[derive(Deserialize)]
struct GestureArgs {
    gesture: String,
}

#[derive(Deserialize)]
struct SpeakArgs {
    text: String,
}

#[derive(Deserialize)]
struct FocusArgs {
    target: String,
    #[serde(default)]
    r#match: Option<String>,
    #[serde(default)]
    place: Option<Place>,
    #[serde(default = "default_linger")]
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

/// Parses one tool call. An unknown name or malformed arguments yields `None` rather than an
/// error — a small local model will get this wrong sometimes, and that is not a reason to stop.
pub fn parse(name: &str, arguments: &str) -> Option<Intent> {
    let args = if arguments.trim().is_empty() { "{}" } else { arguments };
    match name {
        "react" => {
            let a: ReactArgs = serde_json::from_str(args).ok()?;
            Some(Intent::React {
                emotion: Emotion::from_name(&a.emotion)?,
                intensity: a.intensity.clamp(0.0, 1.0),
                hold: a.hold.clamp(1.0, 60.0),
            })
        }
        "gesture" => {
            let a: GestureArgs = serde_json::from_str(args).ok()?;
            Some(Intent::Gesture { gesture: Gesture::from_name(&a.gesture)? })
        }
        "speak" => {
            let a: SpeakArgs = serde_json::from_str(args).ok()?;
            let text = a.text.trim().to_string();
            (!text.is_empty()).then_some(Intent::Speak { text })
        }
        "focus" => {
            let a: FocusArgs = serde_json::from_str(args).ok()?;
            let target = match a.target.as_str() {
                "window" => FocusTarget::Window { r#match: a.r#match? },
                "pointer" => FocusTarget::Pointer,
                "place" => FocusTarget::Place { where_: a.place? },
                "away" => FocusTarget::Away,
                _ => return None,
            };
            Some(Intent::Focus { target, linger: a.linger.clamp(2.0, 600.0) })
        }
        _ => None,
    }
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
        tool("speak", "Say something out loud. This interrupts. Keep it under fifteen words and \
             use it rarely — silence is almost always better.", json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "what to say, one short sentence" }
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
