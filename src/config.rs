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
    /// Everyone currently on screen. More than one is the normal case, not a special mode.
    pub guys: Vec<Guy>,
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
        self.provider_named(&self.mind.provider)
    }

    pub fn provider_named(&self, name: &str) -> Result<&Provider> {
        self.providers.get(name).with_context(|| format!("no [providers.{name}] in config"))
    }

    /// Loads every guy's character file. A roster with a missing character is a startup failure,
    /// not a guy who quietly fails to appear.
    pub fn roster(&self, config_dir: Option<&Path>) -> Result<Vec<(Guy, Character)>> {
        self.guys
            .iter()
            .map(|g| {
                let path = character_file(&g.character, config_dir)
                    .with_context(|| format!("no character named {:?}", g.character))?;
                let raw = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let character: Character = toml::from_str(&raw)
                    .with_context(|| format!("parse {}", path.display()))?;
                Ok((g.clone(), character))
            })
            .collect()
    }
}

/// Beside the config first, so a personal character shadows a shipped one of the same name.
pub fn character_file(name: &str, config_dir: Option<&Path>) -> Option<PathBuf> {
    let leaf = format!("{name}.toml");
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(dir) = config_dir {
        roots.push(dir.join("characters"));
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("characters"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("characters"));
        }
    }
    roots.into_iter().map(|r| r.join(&leaf)).find(|p| p.is_file())
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

/// One member of the roster. `character` names a file supplying the rest; anything set here wins
/// over it, and anything neither supplies falls back to `[buddy]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guy {
    pub character: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub size: Option<f32>,
    /// Key into `[providers.*]`. Different characters may think with different models.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
}

/// A character file: a persona, a palette, and defaults for the body wearing them.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    pub name: String,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub voice: Option<String>,
    /// Replaces the `persona` prompt layer.
    pub persona: String,
    #[serde(default)]
    pub skin: Option<Skin>,
}

/// Palette for the built-in skin. Changing it changes who is standing there.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skin {
    pub head: [u8; 3],
    pub torso: [u8; 3],
    pub arm: [u8; 3],
    pub leg: [u8; 3],
}

impl Default for Skin {
    fn default() -> Self {
        Self {
            head: [232, 196, 160],
            torso: [96, 124, 176],
            arm: [104, 168, 128],
            leg: [200, 136, 84],
        }
    }
}

/// Fallbacks for anything a guy or their character leaves unset.
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

/// Situational facts the engine contributes to the sutras, as `{placeholder}` substitutions.
///
/// Every one must be **constant for the life of the process**. The system prompt has to stay
/// byte-stable or per-conversation caching is lost on every turn, so anything that changes belongs
/// in the event stream instead. Who you are is a fact; what you are looking at is not.
#[derive(Debug, Clone, Default)]
pub struct Facts(BTreeMap<String, String>);

impl Facts {
    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.0.insert(key.into(), value.into());
        self
    }

    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map(String::as_str).unwrap_or_default()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }
}

impl Prompt {
    /// Joins the layers and fills in the engine's facts.
    ///
    /// One rule handles the awkward case: **a line whose placeholder resolves to nothing is
    /// dropped whole.** So `Also here: {others}.` simply vanishes when a character is alone,
    /// instead of leaving a sentence with a hole in it, and no layer needs conditionals.
    pub fn assemble(&self, facts: &Facts, config_dir: Option<&Path>) -> String {
        self.layers
            .iter()
            .filter_map(|key| Some((key, self.texts.get(key)?)))
            .map(|(key, layer)| fill(&read_layer(key, layer, config_dir), facts).trim().to_string())
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
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

fn fill(text: &str, facts: &Facts) -> String {
    let kept: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let mut out = line.to_string();
            let mut emptied = false;
            for key in facts.keys() {
                let token = format!("{{{key}}}");
                if out.contains(&token) {
                    let value = facts.get(key);
                    emptied |= value.is_empty();
                    out = out.replace(&token, value);
                }
            }
            (!emptied).then_some(out)
        })
        .collect();

    // A dropped line leaves its paragraph break behind; close the gap it opened.
    let mut out = kept.join("\n");
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out
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
    /// How long a guy will go with nothing happening before he thinks anyway. Without this,
    /// silence is self-sustaining: an empty slice is never sent, so nobody acts, so nobody has
    /// anything to react to. Zero disables idling entirely.
    #[serde(with = "humantime_serde")]
    pub restless_after: Duration,
    /// Minimum gap between anybody speaking aloud, shared by the whole cast. Six of them on a
    /// forty-five second quantum is a room that will not shut up; this is the floor under it.
    #[serde(with = "humantime_serde")]
    pub speech_floor: Duration,
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
    /// Key into `[voice.engines.*]`. A guy or a character may name a different one.
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
impl Voice {
    /// Resolves what one character sounds like: `"engine:voice"`, `"engine"`, or just a voice.
    ///
    /// A character wanting espeak's flatness is choosing an engine, not only a model, so the
    /// per-guy setting has to be able to say both.
    pub fn resolve(&self, spec: Option<&str>) -> (String, String) {
        let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) else {
            return (self.engine.clone(), self.voice.clone());
        };
        let (engine, voice) = match spec.split_once(':') {
            Some((e, v)) => (e.trim(), v.trim()),
            None if self.engines.contains_key(spec) => (spec, ""),
            None => ("", spec),
        };
        (
            (!engine.is_empty()).then(|| engine.to_string()).unwrap_or_else(|| self.engine.clone()),
            (!voice.is_empty()).then(|| voice.to_string()).unwrap_or_else(|| self.voice.clone()),
        )
    }
}

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
    /// Observations one source may contribute inside that window before the rest are dropped.
    pub per_source_cap: usize,
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
