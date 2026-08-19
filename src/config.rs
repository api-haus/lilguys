//! The whole of lilguys' configurable surface. Nothing here names an internal type.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_TOML: &str = include_str!("../lilguys.default.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub buddy: Buddy,
    pub prompt: Prompt,
    pub mind: Mind,
    pub voice: Voice,
    pub senses: Senses,
    pub motion: Motion,
    pub debug: Debug,
    pub log: Logging,
    pub providers: BTreeMap<String, Provider>,
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str(DEFAULT_TOML).expect("bundled default config must parse")
    }
}

impl Config {
    /// `$LILGUYS_CONFIG`, then `$XDG_CONFIG_HOME/lilguys/lilguys.toml`, then the bundled default.
    pub fn load() -> Result<(Self, Option<PathBuf>)> {
        let Some(path) = Self::path() else { return Ok((Self::default(), None)) };
        if !path.exists() {
            return Ok((Self::default(), None));
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        // Merging onto the defaults would need a second schema; re-reading them as a base document
        // keeps one source of truth for every unset field.
        let mut doc: toml::Table = toml::from_str(DEFAULT_TOML)?;
        let user: toml::Table =
            toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        merge(&mut doc, user);
        let cfg: Config = doc.try_into().with_context(|| format!("apply {}", path.display()))?;
        Ok((cfg, Some(path)))
    }

    pub fn path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("LILGUYS_CONFIG") {
            return Some(PathBuf::from(p));
        }
        Some(dirs::config_dir()?.join("lilguys").join("lilguys.toml"))
    }

    pub fn provider(&self) -> Result<&Provider> {
        self.providers
            .get(&self.mind.provider)
            .with_context(|| format!("no [providers.{}] in config", self.mind.provider))
    }
}

fn merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Buddy {
    pub name: String,
    pub skin: String,
    /// Character height in pixels.
    pub size: f32,
}

/// The system prompt, assembled from named layers in the order `layers` lists them.
///
/// Layering is not decoration. Each layer answers a different question — what may not be done,
/// what this program is, who this creature is — and each is separately replaceable. Rewriting the
/// character must not be able to delete the rule that keeps system text out of its mouth.
#[derive(Debug, Clone, Deserialize)]
pub struct Prompt {
    /// Layer names, in the order they are sent. A name with no matching table is skipped.
    pub layers: Vec<String>,
    #[serde(flatten)]
    pub texts: BTreeMap<String, Layer>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    #[serde(default)]
    pub text: String,
    /// Overrides `text`. Relative paths resolve beside the config file.
    #[serde(default)]
    pub file: Option<PathBuf>,
}

impl Prompt {
    /// Joins the layers. `{name}` in any layer becomes the buddy's name.
    pub fn assemble(&self, name: &str, config_dir: Option<&Path>) -> String {
        self.layers
            .iter()
            .filter_map(|key| Some((key, self.texts.get(key)?)))
            .map(|(key, layer)| read_layer(key, layer, config_dir))
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
            .replace("{name}", name)
    }

    /// Layer names with the length of each, for `--check`.
    pub fn outline(&self, config_dir: Option<&Path>) -> Vec<(String, usize)> {
        self.layers
            .iter()
            .map(|key| match self.texts.get(key) {
                Some(l) => (key.clone(), read_layer(key, l, config_dir).trim().len()),
                None => (format!("{key} (missing)"), 0),
            })
            .collect()
    }
}

fn read_layer(key: &str, layer: &Layer, config_dir: Option<&Path>) -> String {
    let Some(file) = layer.file.as_ref() else { return layer.text.trim().to_string() };
    let path = match (file.is_relative(), config_dir) {
        (true, Some(dir)) => dir.join(file),
        _ => file.clone(),
    };
    std::fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|e| {
            eprintln!("prompt layer {key}: {} — {e}", path.display());
            layer.text.trim().to_string()
        })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mind {
    /// Off means the buddy still senses and emotes locally, but never calls a model.
    pub enabled: bool,
    /// Key into `[providers.*]`.
    pub provider: String,
    /// Events are bucketed into quanta; one quantum is at most one model turn.
    #[serde(with = "humantime_serde")]
    pub quantum: Duration,
    /// Quantum used once the user is away.
    #[serde(with = "humantime_serde")]
    pub idle_quantum: Duration,
    /// Compaction starts above this many tokens of conversation.
    pub context_tokens: usize,
    /// Turns kept verbatim when compacting; everything older becomes one summary.
    pub keep_recent: usize,
    /// Hard ceiling on model turns per hour, regardless of how much happens.
    pub turns_per_hour: f32,
    /// Model turns are skipped while the user is away.
    pub think_while_away: bool,
}

/// Every provider speaks the OpenAI chat-completions wire format: llama.cpp's server, ollama,
/// vLLM, LM Studio, OpenRouter and OpenAI itself. One client, many base URLs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub url: String,
    pub model: String,
    /// Name of the environment variable holding the key. Keys never live in this file.
    #[serde(default)]
    pub api_key_env: Option<String>,
    pub temperature: f32,
    pub max_tokens: u32,
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
    /// Extra headers some gateways want, e.g. OpenRouter's `HTTP-Referer`.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Merged into the request body verbatim. The escape hatch for backend-specific switches —
    /// `think = false` for a reasoning model on ollama, `reasoning_effort`, `top_k`, anything.
    /// The field names here belong to the provider's API, not to lilguys.
    #[serde(default)]
    pub extra: toml::Table,
}

impl Default for Provider {
    fn default() -> Self {
        Self {
            url: "http://127.0.0.1:8080/v1".into(),
            model: "local".into(),
            api_key_env: None,
            temperature: 0.7,
            max_tokens: 512,
            timeout: Duration::from_secs(60),
            headers: BTreeMap::new(),
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voice {
    pub enabled: bool,
    /// Key into `[voice.engines.*]`.
    pub engine: String,
    /// Voice or model name, substituted into the engine's command as `{voice}`.
    pub voice: String,
    pub speed: f32,
    pub volume: f32,
    /// Utterances queued beyond this are dropped; a buddy that talks over itself is worse.
    pub queue_limit: usize,
    pub engines: BTreeMap<String, Engine>,
}

/// A TTS pipeline as a command line. `{text}` `{voice}` `{speed}` `{out}` are substituted;
/// `{out}` is a temporary wav path. This is what makes piper, kokoro, F5-TTS, XTTS and espeak-ng
/// all the same thing to lilguys.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    /// Synthesis command. Receives the text on stdin unless `{text}` appears in the arguments.
    pub synth: Vec<String>,
    /// Playback command for the produced file. Leave empty if `synth` plays it itself.
    #[serde(default)]
    pub play: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Senses {
    pub windows: bool,
    pub workspaces: bool,
    pub media: bool,
    pub idle: bool,
    /// Seconds of no input before the buddy treats the user as away.
    #[serde(with = "humantime_serde")]
    pub away_after: Duration,
    /// A window must hold focus this long before it is worth reporting.
    #[serde(with = "humantime_serde")]
    pub focus_dwell: Duration,
    /// A track must play this long before it is worth reporting.
    #[serde(with = "humantime_serde")]
    pub media_dwell: Duration,
    /// Repeats of the same observation inside this window are dropped.
    #[serde(with = "humantime_serde")]
    pub novelty_window: Duration,
}

/// Where the turn-by-turn record goes. On by default — a buddy that misbehaves silently is
/// undiagnosable, and the volume is a few lines an hour.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logging {
    pub enabled: bool,
    /// Defaults to `$XDG_STATE_HOME/lilguys`.
    #[serde(default)]
    pub dir: Option<PathBuf>,
}

/// Grayboxing aids. All off by default — this is what he looks like, not scaffolding.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Debug {
    /// The readouts panel beside him and the senses panel in the corner.
    pub hud: bool,
    /// Joint dots, part labels, the bounds frame and the gaze ray.
    pub rig: bool,
    /// Awareness radius rings and the line to the pointer.
    pub radii: bool,
    /// Lines of recent gate rulings to float above his head. 0 is off. This is the debugging
    /// instrument that moves with him instead of covering the screen.
    pub overhead: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    /// Pixels per second at a full-effort drift.
    pub speed: f32,
    /// How sharply he changes course. Lower reads as heavier.
    pub agility: f32,
    /// He will not approach the pointer closer than this.
    pub personal_space: f32,
    /// Pointer closer than this earns a look.
    pub notice_radius: f32,
    /// Pointer further than this stops earning one.
    pub forget_radius: f32,
    /// Fraction of the screen he may drift beyond the edge before turning back. 0 pins him on
    /// screen; 1 lets him leave entirely.
    pub offscreen_margin: f32,
    /// Chance per minute of wandering somewhere for no reason at all.
    pub wander_per_minute: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Config::default().motion
    }
}
