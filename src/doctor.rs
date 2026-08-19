//! Every check `lilguy doctor` runs, and the one report `lilguysd --check` prints.
//!
//! The first caller is another program, so every check has a name to branch on and a fix to act on.

use crate::config::{Config, Provider};
use crate::mind;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::wl_registry,
    Connection, Dispatch, QueueHandle,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// Works, but not the way somebody would want it to.
    Warn,
    /// Something is broken, and named.
    Fail,
}

impl Status {
    pub fn tag(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
    /// What to do about it, in a form a caller can run or a person can follow.
    pub fix: Option<String>,
}

impl Check {
    fn new(name: &'static str, status: Status, detail: impl Into<String>) -> Self {
        Self { name, status, detail: detail.into(), fix: None }
    }

    fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    pub fn json(&self) -> Value {
        json!({
            "name": self.name,
            "status": self.status.tag(),
            "detail": self.detail,
            "fix": self.fix,
        })
    }

    pub fn line(&self) -> String {
        format!("{:<5} {:<10} {}", self.status.tag(), self.name, self.detail)
    }
}

fn ok(name: &'static str, detail: impl Into<String>) -> Check {
    Check::new(name, Status::Ok, detail)
}

fn warn(name: &'static str, detail: impl Into<String>) -> Check {
    Check::new(name, Status::Warn, detail)
}

fn fail(name: &'static str, detail: impl Into<String>) -> Check {
    Check::new(name, Status::Fail, detail)
}

pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn ok(&self) -> bool {
        !self.checks.iter().any(|c| c.status == Status::Fail)
    }

    pub fn json(&self) -> Value {
        json!({
            "ok": self.ok(),
            "failed": self.checks.iter().filter(|c| c.status == Status::Fail)
                          .map(|c| c.name).collect::<Vec<_>>(),
            "checks": self.checks.iter().map(Check::json).collect::<Vec<_>>(),
        })
    }
}

/// Runs every check in order, handing each one over as it finishes. The provider probe is a real
/// model turn and can take a minute, which is why this takes a sink instead of returning at the end.
pub fn run(on: &mut dyn FnMut(&Check)) -> Report {
    let mut checks: Vec<Check> = Vec::new();
    let mut emit = |c: Check, checks: &mut Vec<Check>| {
        on(&c);
        checks.push(c);
    };

    let (config, from) = match Config::load() {
        Ok((c, f)) => {
            let where_ = f.clone().map(|p| p.display().to_string());
            emit(ok("config", where_.unwrap_or_else(|| "built-in defaults".into())), &mut checks);
            (c, f)
        }
        Err(e) => {
            emit(
                fail("config", format!("{e:#}")).fix("`lilguysd --print-config` writes a valid one"),
                &mut checks,
            );
            return Report { checks };
        }
    };
    let dir = from.as_deref().and_then(Path::parent);

    emit(roster(&config, dir), &mut checks);
    emit(prompt(&config, dir), &mut checks);
    emit(font(), &mut checks);
    emit(wayland(), &mut checks);

    let provider = provider(&config);
    let reachable = provider.status != Status::Fail;
    emit(provider, &mut checks);
    emit(tools(&config, reachable), &mut checks);
    emit(voice(&config), &mut checks);
    emit(senses(&config), &mut checks);
    emit(pace(&config), &mut checks);
    emit(logging(&config), &mut checks);
    emit(crate::service::status_check(), &mut checks);
    emit(inbox(), &mut checks);

    Report { checks }
}

fn roster(config: &Config, dir: Option<&Path>) -> Check {
    match config.roster(dir) {
        Err(e) => {
            fail("roster", format!("{e:#}")).fix("name a file in characters/ from a [[guys]] block")
        }
        Ok(roster) if roster.is_empty() => {
            warn("roster", "nobody on screen").fix("add a [[guys]] block")
        }
        Ok(roster) => ok(
            "roster",
            roster
                .iter()
                .map(|(g, c)| {
                    let name = g.name.clone().unwrap_or_else(|| c.name.clone());
                    match &g.provider {
                        Some(p) => format!("{name} ({}, {p})", g.character),
                        None => format!("{name} ({})", g.character),
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
}

fn prompt(config: &Config, dir: Option<&Path>) -> Check {
    let outline = config.prompt.outline(dir);
    let detail = outline.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(" · ");
    match outline.iter().find(|(k, _)| k.ends_with("(missing)")) {
        Some((k, _)) => fail("prompt", format!("{detail} — {k}"))
            .fix("every name in [prompt] layers needs a [prompt.<name>] table"),
        None => ok("prompt", detail),
    }
}

fn font() -> Check {
    match Path::new(crate::FONT).is_file() {
        true => ok("font", crate::FONT),
        false => fail("font", format!("{} is missing", crate::FONT))
            .fix("install noto-fonts (the package carrying NotoSansMono-Regular.ttf)"),
    }
}

/// A dummy peer, so the registry can be listed without building the whole app state.
struct Peek;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Peek {
    fn event(
        _: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents,
        _: &Connection, _: &QueueHandle<Self>,
    ) {
    }
}

/// Either the compositor offers wlr-layer-shell or the daemon has nowhere to draw, and learning
/// that from a crash at startup is the worst way to learn it.
fn wayland() -> Check {
    let Ok(conn) = Connection::connect_to_env() else {
        return fail("wayland", "no display")
            .fix("run this inside a wayland session; $WAYLAND_DISPLAY is how it is found");
    };
    let Ok((globals, _)) = registry_queue_init::<Peek>(&conn) else {
        return fail("wayland", "the display is there but its registry would not answer");
    };
    let has =
        |want: &str| globals.contents().with_list(|list| list.iter().any(|g| g.interface == want));
    match has("zwlr_layer_shell_v1") {
        true => ok(
            "wayland",
            format!(
                "layer-shell · toplevels {} · workspaces {}",
                yes(has("zwlr_foreign_toplevel_manager_v1")),
                yes(has("ext_workspace_manager_v1")),
            ),
        ),
        false => fail("wayland", "this compositor has no wlr-layer-shell")
            .fix("use a wlroots compositor (hyprland, sway, river, niri); GNOME does not offer it"),
    }
}

fn yes(b: bool) -> &'static str {
    match b {
        true => "yes",
        false => "no",
    }
}

fn provider(config: &Config) -> Check {
    let Ok(p) = config.provider() else {
        return fail("provider", format!("no [providers.{}] in config", config.mind.provider))
            .fix("`lilguy provider list` shows what is configured and what answers");
    };
    let detail = format!("{} · {} · {}", config.mind.provider, p.url, p.model);
    match key_missing(p) {
        Some(var) => fail("provider", format!("{detail} — ${var} is not set"))
            .fix(format!("export {var}=… in the daemon's environment")),
        None => ok("provider", detail),
    }
}

/// A named key that is not in the environment. Keys are named in the config and stored nowhere.
fn key_missing(p: &Provider) -> Option<String> {
    let var = p.api_key_env.clone()?;
    std::env::var_os(&var).is_none().then_some(var)
}

/// The one capability that decides whether any of this works.
fn tools(config: &Config, provider_ok: bool) -> Check {
    if !config.mind.enabled {
        return warn("tools", "[mind] enabled = false — bodies only, nobody thinks")
            .fix("set [mind] enabled = true");
    }
    if !provider_ok {
        return fail("tools", "not probed; the provider is not usable yet");
    }
    match mind::probe(config) {
        Ok(p) if p.native_tools => ok("tools", format!("native tool calls · called {}", p.detail)),
        Ok(p) => fail("tools", format!("no native tool calls — {}", p.detail))
            .fix("pick a model that supports tool calling; lilguys never parses calls out of text"),
        Err(e) => fail("tools", format!("unreachable: {e:#}"))
            .fix("start the backend, or `lilguy provider use <name>`"),
    }
}

fn voice(config: &Config) -> Check {
    if !config.voice.enabled {
        return warn("voice", "disabled — everything else still works, silently")
            .fix("set [voice] enabled = true");
    }
    let Some(engine) = config.voice.engines.get(&config.voice.engine) else {
        return fail("voice", format!("no [voice.engines.{}]", config.voice.engine))
            .fix("`lilguy voice list` names the engines this machine has");
    };
    let Some(bin) = engine.synth.first() else {
        return fail("voice", format!("[voice.engines.{}] has no synth command", config.voice.engine));
    };
    let detail = format!("{} · {} · {bin}", config.voice.engine, config.voice.voice);
    if !crate::service::which(bin) {
        return fail("voice", format!("{detail} not on PATH"))
            .fix(format!("install {bin}, or `lilguy voice use <engine>`"));
    }
    // A missing voice model fails at the first word rather than at startup, which reads as a bug.
    let model = expand(&config.voice.voice);
    if config.voice.voice.contains('/') && !model.is_file() {
        return fail("voice", format!("{detail} — no voice model at {}", model.display()))
            .fix("fetch one from https://huggingface.co/rhasspy/piper-voices");
    }
    ok("voice", detail)
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    }
}

fn senses(config: &Config) -> Check {
    let s = &config.senses;
    ok(
        "senses",
        format!("windows={} workspaces={} media={} idle={}", s.windows, s.workspaces, s.media, s.idle),
    )
}

fn pace(config: &Config) -> Check {
    let m = &config.mind;
    ok(
        "pace",
        format!(
            "{:?} awake · {:?} away · {} turns/hour · restless after {:?}",
            m.quantum, m.idle_quantum, m.turns_per_hour, m.restless_after
        ),
    )
}

fn logging(config: &Config) -> Check {
    if !config.log.enabled {
        return warn("log", "disabled — misbehaviour will not be diagnosable")
            .fix("set [log] enabled = true");
    }
    let dir = config.log.dir.clone().unwrap_or_else(crate::log::default_dir);
    match dir.is_dir() {
        true => ok("log", dir.display().to_string()),
        false => ok("log", format!("{} (created on start)", dir.display())),
    }
}

fn inbox() -> Check {
    let path = crate::sensors::inbox::socket_path();
    let listening = std::os::unix::net::UnixStream::connect(&path).is_ok();
    match (listening, crate::service::running()) {
        (true, _) => ok("inbox", path.display().to_string()),
        (false, false) => ok("inbox", format!("{} (opens with the daemon)", path.display())),
        (false, true) => fail("inbox", format!("{} is not listening", path.display()))
            .fix("restart the daemon: `lilguy stop && lilguy start`"),
    }
}
