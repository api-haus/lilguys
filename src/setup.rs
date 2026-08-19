//! Turning "what is on this machine" into a config that boots. Every step is a `doctor` check with
//! permission to fix it, which is what keeps setup idempotent.

use crate::config::{self, Config};
use crate::mind;
use crate::mind::provider;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// How long to wait on a local endpoint that is probably not there.
const REACH: Duration = Duration::from_millis(1200);

/// One place a model might be served from, and what it turned out to be.
pub struct Endpoint {
    pub name: String,
    pub url: String,
    /// Named by `[providers.*]` in the config, rather than merely guessed at.
    pub configured: bool,
    pub current: bool,
    pub models: Vec<String>,
    pub error: Option<String>,
}

impl Endpoint {
    pub fn reachable(&self) -> bool {
        self.error.is_none()
    }

    pub fn line(&self) -> String {
        let mark = match (self.current, self.configured) {
            (true, _) => "*",
            (false, true) => "·",
            (false, false) => "+",
        };
        let what = match &self.error {
            Some(e) => format!("unreachable — {e}"),
            None if self.models.is_empty() => "reachable, no models".into(),
            None => format!("{} models · {}", self.models.len(), self.models.join(", ")),
        };
        format!("{mark} {:<12} {:<34} {what}", self.name, self.url)
    }
}

/// Everything configured, plus the local endpoints worth guessing at. Local first, because a
/// creature that costs nothing to run should be the default answer.
pub fn endpoints(config: &Config) -> Vec<Endpoint> {
    let mut out: Vec<Endpoint> = Vec::new();
    for (name, p) in &config.providers {
        let key = p.api_key_env.as_ref().and_then(|v| std::env::var(v).ok());
        let missing_key = p.api_key_env.is_some() && key.is_none();
        let (models, error) = match missing_key {
            true => (Vec::new(), Some(format!("${} is not set", p.api_key_env.clone().unwrap()))),
            false => match provider::models(&p.url, key.as_deref(), REACH) {
                Ok(m) => (m, None),
                Err(e) => (Vec::new(), Some(format!("{e}"))),
            },
        };
        out.push(Endpoint {
            name: name.clone(),
            url: p.url.clone(),
            configured: true,
            current: *name == config.mind.provider,
            models,
            error,
        });
    }
    for (name, url) in provider::LOCAL {
        if out.iter().any(|e| e.url.trim_end_matches('/') == url.trim_end_matches('/')) {
            continue;
        }
        if let Ok(models) = provider::models(url, None, REACH) {
            out.push(Endpoint {
                name: name.into(),
                url: url.into(),
                configured: false,
                current: false,
                models,
                error: None,
            });
        }
    }
    out
}

/// Switches provider, having first proved the model can do the one thing that matters.
pub fn use_provider(config: &Config, name: &str) -> Result<String> {
    let p = config.provider_named(name)?;
    let probe = mind::probe_provider(p).with_context(|| format!("probing {name}"))?;
    if !probe.native_tools {
        bail!("{name} has no native tool calls — {}", probe.detail);
    }
    let path = Config::path_for_writing()?;
    config::set(&path, &[("mind.provider", name.into())])?;
    Ok(format!("{name} · {} · {} · called {}", p.url, p.model, probe.detail))
}

/// Switches the model the current provider runs, probing it before writing anything.
pub fn use_model(config: &Config, model: &str) -> Result<String> {
    let name = config.mind.provider.clone();
    let mut p = config.provider_named(&name)?.clone();
    p.model = model.into();
    let probe = mind::probe_provider(&p).with_context(|| format!("probing {model}"))?;
    if !probe.native_tools {
        bail!("{model} has no native tool calls — {}", probe.detail);
    }
    let path = Config::path_for_writing()?;
    config::set(&path, &[(&format!("providers.{name}.model"), model.into())])?;
    Ok(format!("{name} now runs {model} · called {}", probe.detail))
}

/// Downloads a model through the backend that is configured. Only ollama has a pull.
pub fn pull_model(config: &Config, model: &str) -> Result<String> {
    let p = config.provider()?;
    if !p.url.contains("11434") {
        bail!(
            "{} is not ollama, and nothing else exposes a pull — download the model the way that \
             backend expects, then `lilguy model use {model}`",
            config.mind.provider
        );
    }
    if !crate::service::which("ollama") {
        bail!("ollama is not on PATH; install it, or pull the model on the machine serving it");
    }
    let status = Command::new("ollama").arg("pull").arg(model).status().context("ollama pull")?;
    match status.success() {
        true => Ok(format!("pulled {model}")),
        false => bail!("ollama pull {model} exited with {status}"),
    }
}

/// One TTS engine as this machine has it.
pub struct Sound {
    pub engine: String,
    pub command: String,
    pub present: bool,
    pub current: bool,
}

impl Sound {
    pub fn line(&self) -> String {
        format!(
            "{} {:<14} {:<12} {}",
            if self.current { "*" } else { "·" },
            self.engine,
            self.command,
            if self.present { "found" } else { "not on PATH" }
        )
    }
}

pub fn voices(config: &Config) -> Vec<Sound> {
    config
        .voice
        .engines
        .iter()
        .map(|(name, e)| {
            let command = e.synth.first().cloned().unwrap_or_default();
            Sound {
                present: crate::service::which(&command),
                command,
                current: *name == config.voice.engine,
                engine: name.clone(),
            }
        })
        .collect()
}

/// Piper voice models already on this machine, newest naming convention first.
pub fn piper_voices() -> Vec<PathBuf> {
    let dir = piper_dir();
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "onnx"))
        .collect();
    found.sort();
    found
}

pub fn piper_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("piper-voices")
}

/// What an engine sounds like when somebody switches to it without naming a voice. Switching
/// engines must not leave a piper model path in espeak's `-v` argument.
fn default_voice(engine: &str) -> String {
    match engine {
        "piper" => piper_voices()
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "en_GB-alba-medium".into()),
        "kokoro" => "af_sky".into(),
        "openai-speech" => "alloy".into(),
        _ => "en-gb".into(),
    }
}

/// Switches voice. `spec` is `"engine:voice"`, an engine, or a voice.
pub fn use_voice(config: &Config, spec: &str) -> Result<String> {
    let spec = spec.trim();
    let names_engine_only = match spec.split_once(':') {
        Some((_, v)) => v.trim().is_empty(),
        None => config.voice.engines.contains_key(spec),
    };
    let (engine, voice) = config.voice.resolve(Some(spec));
    let voice = match names_engine_only {
        true => default_voice(&engine),
        false => voice,
    };
    let e = config
        .voice
        .engines
        .get(&engine)
        .with_context(|| format!("no [voice.engines.{engine}]"))?;
    let bin = e.synth.first().context("that engine has no synth command")?;
    if !crate::service::which(bin) {
        bail!("{engine} wants {bin}, which is not on PATH");
    }
    // A piper voice is a file. A bare name is the one somebody has downloaded, if they have.
    let voice = match (engine.as_str(), voice.contains('/')) {
        ("piper", false) => {
            let model = piper_dir().join(format!("{voice}.onnx"));
            match model.is_file() {
                true => model.display().to_string(),
                false => bail!("no piper voice at {} — `lilguy voice fetch {voice}`", model.display()),
            }
        }
        _ => voice,
    };
    let path = Config::path_for_writing()?;
    config::set(
        &path,
        &[
            ("voice.enabled", true.into()),
            ("voice.engine", engine.as_str().into()),
            ("voice.voice", voice.as_str().into()),
        ],
    )?;
    Ok(format!("{engine} · {voice}"))
}

/// Fetches one piper voice from the model repository. About 60 MB, and it is a download, so
/// nothing calls this without being asked to.
pub fn fetch_piper_voice(name: &str) -> Result<PathBuf> {
    let (lang, region) = name.split_once('-').context("a voice name looks like en_GB-alba-medium")?;
    let (speaker, quality) =
        region.split_once('-').context("a voice name looks like en_GB-alba-medium")?;
    let short = lang.split('_').next().unwrap_or(lang);
    let base = format!(
        "https://huggingface.co/rhasspy/piper-voices/resolve/main/{short}/{lang}/{speaker}/{quality}/{name}"
    );
    let dir = piper_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

    let model = dir.join(format!("{name}.onnx"));
    for (url, to) in [
        (format!("{base}.onnx"), model.clone()),
        (format!("{base}.onnx.json"), dir.join(format!("{name}.onnx.json"))),
    ] {
        if to.is_file() {
            continue;
        }
        download(&url, &to).with_context(|| format!("fetch {url}"))?;
    }
    Ok(model)
}

fn download(url: &str, to: &PathBuf) -> Result<()> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into();
    let mut res = agent.get(url).call()?;
    let mut body = res.body_mut().as_reader();
    let tmp = to.with_extension("part");
    let mut file = std::fs::File::create(&tmp)?;
    std::io::copy(&mut body, &mut file)?;
    std::fs::rename(&tmp, to)?;
    Ok(())
}

/// Installs the systemd user unit, so the cast comes back after a reboot.
pub fn install_unit() -> Result<String> {
    if !crate::service::which("systemctl") {
        bail!("no systemctl here; `lilguy start` runs it as an ordinary process instead");
    }
    let path = crate::service::unit_path();
    let dir = path.parent().context("no systemd user directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    // The shipped unit names the cargo-install path; a build tree is just as normal a place to
    // run this from, so whichever daemon sits beside this binary is the one the unit starts.
    let exec = crate::service::daemon_binary();
    let unit: String = UNIT
        .lines()
        .map(|l| match l.starts_with("ExecStart=") {
            true => format!("ExecStart={}", exec.display()),
            false => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, unit).with_context(|| format!("write {}", path.display()))?;
    Command::new("systemctl").args(["--user", "daemon-reload"]).status().ok();
    Command::new("systemctl")
        .args(["--user", "enable", crate::service::UNIT])
        .status()
        .context("systemctl --user enable")?;
    Ok(format!("{} · enabled at login", path.display()))
}

const UNIT: &str = include_str!("../packaging/lilguys.service");

/// What a step did. `Blocked` is the one that matters to a caller: something needs a decision
/// nobody was there to make, and the question is in the step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Did {
    Kept,
    Changed,
    Skipped,
    Blocked,
}

impl Did {
    pub fn tag(self) -> &'static str {
        match self {
            Did::Kept => "kept",
            Did::Changed => "set",
            Did::Skipped => "skip",
            Did::Blocked => "ask",
        }
    }
}

pub struct Step {
    pub name: &'static str,
    pub did: Did,
    pub detail: String,
    /// The decision that was needed, when nobody was there to make it.
    pub question: Option<String>,
}

impl Step {
    pub fn line(&self) -> String {
        format!("{:<5} {:<10} {}", self.did.tag(), self.name, self.detail)
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name, "did": self.did.tag(),
            "detail": self.detail, "question": self.question,
        })
    }
}

/// What setup was told before it started. Anything left unset it works out, and anything it
/// cannot work out becomes a question.
#[derive(Default)]
pub struct Options {
    pub yes: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub voice: Option<String>,
    pub service: Option<bool>,
}

/// Models known to do native tool calls, best first. Not a whitelist — a starting order, so a
/// machine with fifteen models is not probed fifteen times.
const TOOL_CAPABLE: [&str; 8] =
    ["qwen3", "qwen2.5", "llama3.3", "llama3.2", "llama3.1", "mistral", "hermes", "command-r"];

/// The model to pull when nothing on the machine can do tool calls, sized to what will hold it.
fn recommended_model() -> (&'static str, &'static str) {
    match vram_gb() {
        Some(gb) if gb >= 15.0 => ("qwen3:8b", "5.2 GB"),
        Some(gb) if gb >= 7.0 => ("qwen3:4b", "2.5 GB"),
        Some(_) => ("llama3.2:3b", "2.0 GB"),
        None => ("qwen3:4b", "2.5 GB"),
    }
}

/// Video memory, from whichever of the two places on this machine has it.
pub fn vram_gb() -> Option<f32> {
    if let Ok(cards) = std::fs::read_dir("/sys/class/drm") {
        let mut best = 0.0f32;
        for card in cards.flatten() {
            let total = card.path().join("device/mem_info_vram_total");
            if let Some(bytes) = std::fs::read_to_string(total).ok().and_then(|s| s.trim().parse::<f64>().ok()) {
                best = best.max((bytes / 1e9) as f32);
            }
        }
        if best > 0.0 {
            return Some(best);
        }
    }
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next()?.trim().parse::<f32>().ok().map(|mb| mb / 1024.0)
}

/// Setup is `doctor` plus permission to fix. Every step checks first, so running it twice changes
/// nothing the second time.
pub fn run(
    opts: &Options, ask: &mut dyn FnMut(&str) -> bool, say: &mut dyn FnMut(&Step),
) -> Result<Vec<Step>> {
    let mut steps: Vec<Step> = Vec::new();
    let mut push = |step: Step, steps: &mut Vec<Step>| {
        say(&step);
        let blocked = step.did == Did::Blocked;
        steps.push(step);
        blocked
    };

    let path = Config::path_for_writing()?;
    let existed = path.is_file();
    if !existed {
        config::set(&path, &[("mind.enabled", true.into())])?;
    }
    push(
        Step {
            name: "config",
            did: if existed { Did::Kept } else { Did::Changed },
            detail: path.display().to_string(),
            question: None,
        },
        &mut steps,
    );

    let (config, _) = Config::load()?;
    if push(provider_step(&config, opts, ask)?, &mut steps) {
        return Ok(steps);
    }
    // Re-read, because the step above may have written a provider the next one has to see.
    let (config, _) = Config::load()?;
    if push(voice_step(&config, opts, ask)?, &mut steps) {
        return Ok(steps);
    }
    let (config, _) = Config::load()?;
    push(roster_step(&config)?, &mut steps);
    push(service_step(opts, ask)?, &mut steps);
    Ok(steps)
}

/// Keeps a provider that already works. Otherwise finds one, and offers to pull a model if
/// nothing on the machine can do the one thing that matters.
fn provider_step(config: &Config, opts: &Options, ask: &mut dyn FnMut(&str) -> bool) -> Result<Step> {
    let kept = |detail: String| Step { name: "provider", did: Did::Kept, detail, question: None };
    let changed =
        |detail: String| Step { name: "provider", did: Did::Changed, detail, question: None };
    let blocked = |q: String| Step {
        name: "provider",
        did: Did::Blocked,
        detail: q.clone(),
        question: Some(q),
    };

    if let (Some(name), Some(model)) = (&opts.provider, &opts.model) {
        let mut p = config.provider_named(name)?.clone();
        p.model = model.clone();
        return match mind::probe_provider(&p)?.native_tools {
            true => {
                write_provider(name, model)?;
                Ok(changed(format!("{name} · {model}")))
            }
            false => Ok(blocked(format!("{name} running {model} cannot do native tool calls"))),
        };
    }

    if opts.provider.is_none() && opts.model.is_none() {
        if let Ok(p) = config.provider() {
            if mind::probe_provider_n(p, 2).map(|r| r.native_tools).unwrap_or(false) {
                return Ok(kept(format!("{} · {} · {}", config.mind.provider, p.url, p.model)));
            }
        }
    }

    let found = endpoints(config);
    let wanted = opts.provider.as_deref();
    for endpoint in found.iter().filter(|e| e.reachable() && e.configured) {
        if wanted.is_some_and(|w| w != endpoint.name) {
            continue;
        }
        let base = config.provider_named(&endpoint.name)?.clone();
        for model in candidates(&endpoint.models, &base.model, opts.model.as_deref()) {
            let mut p = base.clone();
            p.model = model.clone();
            p.max_tokens = p.max_tokens.max(3000);
            if mind::probe_provider_n(&p, 2).map(|r| r.native_tools).unwrap_or(false) {
                write_provider(&endpoint.name, &model)?;
                return Ok(changed(format!("{} · {model}", endpoint.name)));
            }
        }
    }

    // A named provider is a decision already made; pulling a model into a different backend
    // would be answering a question nobody asked.
    if let Some(name) = wanted {
        let why = found
            .iter()
            .find(|e| e.name == name)
            .and_then(|e| e.error.clone())
            .unwrap_or_else(|| "none of its models can do native tool calls".into());
        return Ok(blocked(format!("{name} cannot be used — {why}")));
    }

    // Nothing here can do it. Pulling is the one fix that does not need a human at a keyboard.
    let ollama = found.iter().find(|e| e.reachable() && e.url.contains("11434"));
    let (model, size) = recommended_model();
    let Some(ollama) = ollama else {
        return Ok(blocked(
            "no model backend answered. Install ollama (https://ollama.com) and run \
             `lilguy setup` again, or point [providers.*] at one that is already running"
                .into(),
        ));
    };
    let question = format!("nothing installed can do native tool calls. Pull {model} ({size})?");
    if !ask(&question) {
        return Ok(blocked(question));
    }
    let name = ollama.name.clone();
    let status = Command::new("ollama").arg("pull").arg(model).status().context("ollama pull")?;
    if !status.success() {
        return Ok(blocked(format!("ollama pull {model} exited with {status}")));
    }
    let mut p = config.provider_named(&name).cloned().unwrap_or_default();
    p.url = ollama.url.clone();
    p.model = model.into();
    p.max_tokens = p.max_tokens.max(3000);
    match mind::probe_provider(&p)?.native_tools {
        true => {
            write_provider(&name, model)?;
            Ok(changed(format!("{name} · {model}, pulled")))
        }
        false => Ok(blocked(format!("{model} was pulled and still cannot do native tool calls"))),
    }
}

/// The models worth probing, best first: what was asked for, what is already configured, then
/// whatever is named after a family known to do this.
fn candidates(models: &[String], configured: &str, wanted: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(w) = wanted {
        out.push(w.to_string());
    }
    if models.iter().any(|m| m == configured) {
        out.push(configured.to_string());
    }
    for family in TOOL_CAPABLE {
        for m in models.iter().filter(|m| m.starts_with(family)) {
            out.push(m.clone());
        }
    }
    out.dedup();
    out.truncate(3);
    out
}

/// A reasoning model with a small budget stops mid-thought having called nothing, so the budget
/// goes in beside the model rather than being discovered from a probe message.
fn write_provider(name: &str, model: &str) -> Result<()> {
    let path = Config::path_for_writing()?;
    config::set(
        &path,
        &[
            ("mind.enabled", true.into()),
            ("mind.provider", name.into()),
            (&format!("providers.{name}.model"), model.into()),
            (&format!("providers.{name}.max_tokens"), 3000.into()),
        ],
    )
}

fn voice_step(config: &Config, opts: &Options, ask: &mut dyn FnMut(&str) -> bool) -> Result<Step> {
    let step = |did, detail: String| Step { name: "voice", did, detail, question: None };
    if let Some(spec) = opts.voice.as_deref() {
        return Ok(step(Did::Changed, use_voice(config, spec)?));
    }
    let working = config.voice.enabled
        && voices(config).iter().any(|s| s.current && s.present)
        && (!config.voice.voice.contains('/') || expand(&config.voice.voice).is_file());
    if working {
        return Ok(step(Did::Kept, format!("{} · {}", config.voice.engine, config.voice.voice)));
    }
    if !piper_voices().is_empty() && crate::service::which("piper") {
        return Ok(step(Did::Changed, use_voice(config, "piper")?));
    }
    if crate::service::which("piper") {
        let question = "piper is installed but has no voice. Download en_GB-alba-medium (63 MB)?";
        if ask(question) {
            let model = fetch_piper_voice("en_GB-alba-medium")?;
            return Ok(step(Did::Changed, use_voice(config, &format!("piper:{}", model.display()))?));
        }
    }
    if crate::service::which("espeak-ng") {
        return Ok(step(Did::Changed, use_voice(config, "espeak")?));
    }
    Ok(Step {
        name: "voice",
        did: Did::Skipped,
        detail: "no TTS engine on this machine; they will be silent".into(),
        question: Some("install piper (`uv tool install piper-tts`) or espeak-ng".into()),
    })
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    }
}

fn roster_step(config: &Config) -> Result<Step> {
    let dir = Config::path().and_then(|p| p.parent().map(PathBuf::from));
    let roster = config.roster(dir.as_deref())?;
    let who: Vec<String> = roster
        .iter()
        .map(|(g, c)| g.name.clone().unwrap_or_else(|| c.name.clone()))
        .collect();
    Ok(Step { name: "roster", did: Did::Kept, detail: who.join(", "), question: None })
}

fn service_step(opts: &Options, ask: &mut dyn FnMut(&str) -> bool) -> Result<Step> {
    let step = |did, detail: String| Step { name: "service", did, detail, question: None };
    if opts.service == Some(false) {
        return Ok(step(Did::Skipped, "not installing a unit".into()));
    }
    if crate::service::unit() != crate::service::Unit::Absent {
        let started = crate::service::start()?;
        return Ok(step(Did::Kept, started));
    }
    let question = "Install the systemd user unit, so they come back after a reboot?";
    if opts.service != Some(true) && !ask(question) {
        let started = crate::service::start()?;
        return Ok(step(Did::Skipped, started));
    }
    let installed = install_unit()?;
    let started = crate::service::start()?;
    Ok(step(Did::Changed, format!("{installed} · {started}")))
}
