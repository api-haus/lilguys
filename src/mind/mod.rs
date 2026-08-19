//! The reactor. Sensed events are quantised into buckets, one bucket is at most one model turn.

pub mod capability;
pub mod provider;

use crate::config::{Config, Mind as MindConfig};
use crate::log;
use anyhow::Result;
use capability::Intent;
use provider::{Client, Message};
use serde_json::json;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// One bucket of the quantised stream: everything that happened in a quantum, already deduplicated
/// by the local gate, plus the state that frames it.
#[derive(Clone, Debug)]
pub struct Quantum {
    pub events: Vec<String>,
    pub focused: Option<String>,
    pub workspace: Option<String>,
    pub present: bool,
    pub since_last: Duration,
    /// How the body is right now, not what changed. Interoception is continuous.
    pub sensation: String,
}

impl Quantum {
    fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "[{}s elapsed · they are {} · workspace {} · they are looking at {}]\n[you are {}]\n",
            self.since_last.as_secs(),
            if self.present { "here" } else { "away" },
            self.workspace.as_deref().unwrap_or("?"),
            self.focused.as_deref().unwrap_or("nothing"),
            self.sensation,
        ));
        for e in &self.events {
            out.push_str("- ");
            out.push_str(e);
            out.push('\n');
        }
        if self.events.is_empty() {
            out.push_str("- nothing at all. it has been quiet a while\n");
        }
        out
    }
}

/// What came back from a turn, for the caller to enact and to show.
#[derive(Clone, Debug)]
pub struct Reaction {
    pub intents: Vec<Intent>,
    /// Calls the model made that were refused, with the reason. Empty is the healthy case.
    pub rejected: Vec<String>,
    pub note: Option<String>,
    pub tokens: usize,
    pub compacted: bool,
    pub error: Option<String>,
}

pub enum ToMind {
    Tick(Quantum),
}

/// Runs the model loop on its own thread. Blocking HTTP never touches the render loop.
pub fn spawn(
    config: &Config,
    provider: &str,
    system: String,
    out: calloop::channel::Sender<Reaction>,
) -> Result<mpsc::Sender<ToMind>> {
    let client = Client::new(config.provider_named(provider)?.clone())?;
    let mind = config.mind.clone();
    let (tx, rx) = mpsc::channel();

    std::thread::Builder::new()
        .name("mind".into())
        .spawn(move || {
            let mut reactor = Reactor::new(client, mind, system);
            while let Ok(ToMind::Tick(q)) = rx.recv() {
                let reaction = reactor.turn(q);
                if out.send(reaction).is_err() {
                    return;
                }
            }
        })
        .map(|_| tx)
        .map_err(Into::into)
}

struct Reactor {
    client: Client,
    config: MindConfig,
    system: String,
    history: Vec<Message>,
    budget: f32,
    budget_at: Instant,
}

impl Reactor {
    fn new(client: Client, config: MindConfig, system: String) -> Self {
        Self {
            client,
            budget: config.turns_per_hour * 0.25,
            budget_at: Instant::now(),
            config,
            system,
            history: Vec::new(),
        }
    }

    fn afford(&mut self) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.budget_at).as_secs_f32();
        self.budget_at = now;
        let ceiling = (self.config.turns_per_hour * 0.25).max(1.0);
        self.budget = (self.budget + dt * self.config.turns_per_hour / 3600.0).min(ceiling);
        if self.budget < 1.0 {
            return false;
        }
        self.budget -= 1.0;
        true
    }

    fn turn(&mut self, q: Quantum) -> Reaction {
        let mut reaction = Reaction {
            intents: Vec::new(),
            rejected: Vec::new(),
            note: None,
            tokens: self.tokens(),
            compacted: false,
            error: None,
        };
        if !q.present && !self.config.think_while_away {
            return reaction;
        }
        if !self.afford() {
            reaction.error = Some("over budget".into());
            return reaction;
        }

        reaction.compacted = self.compact_if_needed();
        let prompt = q.render();
        self.history.push(Message::user(prompt.clone()));

        let started = Instant::now();
        let reply = match self.client.chat(&self.messages(), &capability::schemas()) {
            Ok(m) => m,
            Err(e) => {
                // Drop the prompt again so a dead endpoint cannot inflate the window forever.
                self.history.pop();
                reaction.error = Some(format!("{e:#}"));
                log::turn(&prompt, "", &json!(null), &[], &[], self.tokens(), reaction.compacted,
                          reaction.error.as_deref(), started.elapsed().as_millis());
                return reaction;
            }
        };
        let elapsed = started.elapsed().as_millis();

        let mut rejected = Vec::new();
        for call in reply.tool_calls.iter().flatten() {
            match capability::parse(&call.function.name, &call.function.arguments) {
                Ok(intent) => reaction.intents.push(intent),
                Err(why) => rejected.push(format!("{}({}) — {why}", call.function.name,
                                                  crate::sensors::clip(&call.function.arguments, 60))),
            }
        }
        let text = reply.content.clone().unwrap_or_default();
        // Models leave stray braces and whitespace beside their tool calls; that is not a note.
        let noteworthy = text.chars().filter(|c| c.is_alphanumeric()).count() >= 4;
        reaction.note = noteworthy.then(|| text.clone());
        // A model that writes calls as prose is misconfigured, not something to parse around.
        if reply.tool_calls.is_none() && text.contains('(') && text.contains(')') {
            reaction.error = Some("model wrote calls as text; it cannot do native tool calls".into());
        }
        if !rejected.is_empty() {
            reaction.error.get_or_insert_with(|| format!("rejected {} call(s)", rejected.len()));
        }
        reaction.rejected = rejected.clone();

        log::turn(
            &prompt,
            &text,
            &serde_json::to_value(&reply.tool_calls).unwrap_or(json!(null)),
            &reaction.intents.iter().map(Intent::summary).collect::<Vec<_>>(),
            &rejected,
            self.tokens(),
            reaction.compacted,
            reaction.error.as_deref(),
            elapsed,
        );

        let calls = reply.tool_calls.clone().unwrap_or_default();
        self.history.push(reply);
        // Every tool call must be answered or the next turn is a protocol error on strict backends.
        for call in calls {
            self.history.push(Message::tool_result(call.id, "ok"));
        }

        reaction.tokens = self.tokens();
        reaction
    }

    fn messages(&self) -> Vec<Message> {
        let mut all = Vec::with_capacity(self.history.len() + 1);
        all.push(Message::system(self.system.clone()));
        all.extend(self.history.iter().cloned());
        all
    }

    fn tokens(&self) -> usize {
        self.system.len() / 4 + self.history.iter().map(Message::approx_tokens).sum::<usize>()
    }

    /// Folds everything older than `keep_recent` into one summary message. Falls back to simply
    /// dropping the old turns if the summariser itself fails — the window must shrink either way.
    fn compact_if_needed(&mut self) -> bool {
        if self.tokens() <= self.config.context_tokens {
            return false;
        }
        let keep = self.config.keep_recent.min(self.history.len());
        let cut = self.history.len() - keep;
        if cut == 0 {
            return false;
        }
        let old: Vec<Message> = self.history.drain(..cut).collect();

        let mut ask = vec![Message::system(
            "Summarise the notes below into at most eight short lines: what the user has been \
             doing, what you already reacted to, and anything you wanted to remember. Write only \
             the summary.",
        )];
        ask.extend(old.iter().filter(|m| m.role != "tool").cloned());

        let summary = self
            .client
            .summarise(&ask)
            .unwrap_or_else(|_| "Earlier activity, since forgotten.".to_string());
        self.history.insert(0, Message::system(format!("Earlier:\n{summary}")));
        true
    }
}

/// Buckets the gate's output into quanta and decides when one is ready to send.
pub struct Quantiser {
    events: Vec<String>,
    last_sent: Instant,
    /// Somebody is waiting for an answer, so this slice closes on the next tick.
    urgent: bool,
    pub focused: Option<String>,
    pub workspace: Option<String>,
    pub present: bool,
    pub sensation: String,
}

impl Default for Quantiser {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            last_sent: Instant::now(),
            urgent: false,
            focused: None,
            workspace: None,
            present: true,
            sensation: String::new(),
        }
    }
}

impl Quantiser {
    const MAX_EVENTS: usize = 24;

    pub fn observe(&mut self, line: String) {
        if self.events.len() < Self::MAX_EVENTS {
            self.events.push(line);
        }
    }

    /// Cuts the current slice short. The one legitimate override of the pace: everything else may
    /// wait forty-five seconds because nothing is waiting on it, and a person who just typed is.
    pub fn urge(&mut self) {
        self.urgent = true;
    }

    pub fn pending(&self) -> usize {
        self.events.len()
    }

    pub fn since_last(&self) -> Duration {
        self.last_sent.elapsed()
    }

    /// Returns a quantum once its window has elapsed and there is something to say — or once it
    /// has been quiet long enough that having nothing to say is itself worth a thought.
    pub fn take(&mut self, config: &MindConfig, now: Instant) -> Option<Quantum> {
        let window = if self.present { config.quantum } else { config.idle_quantum };
        let since = now.duration_since(self.last_sent);
        if since < window && !self.urgent {
            return None;
        }
        let restless = !config.restless_after.is_zero()
            && self.present
            && since >= config.restless_after;
        if self.events.is_empty() && !restless {
            self.urgent = false;
            return None;
        }
        self.urgent = false;
        self.last_sent = now;
        Some(Quantum {
            events: std::mem::take(&mut self.events),
            focused: self.focused.clone(),
            workspace: self.workspace.clone(),
            present: self.present,
            since_last: since,
            sensation: self.sensation.clone(),
        })
    }
}

/// What a provider can actually do, answered before a surface is ever opened.
pub struct Probe {
    pub native_tools: bool,
    pub detail: String,
}

/// Reachability plus the only capability that matters: does this model return `tool_calls`, or
/// does it write calls into the message text? A model that cannot do the former is not usable.
pub fn probe(config: &Config) -> Result<Probe> {
    probe_provider(config.provider()?)
}

/// The same question asked of a provider that is not the configured one — what `lilguy provider
/// use` and `lilguy setup` need before they will write anything into a config file.
pub fn probe_provider(provider: &crate::config::Provider) -> Result<Probe> {
    probe_provider_n(provider, ATTEMPTS)
}

/// The same, with the patience named — setup tries several models and cannot spend three turns
/// on each of them.
pub fn probe_provider_n(provider: &crate::config::Provider, attempts: usize) -> Result<Probe> {
    let client = Client::new(provider.clone())?;
    let mut last = Probe { native_tools: false, detail: "never answered".into() };
    // A reasoning model sometimes spends a whole turn thinking and calls nothing, which says
    // nothing either way about whether it *can*. Ask again before condemning it.
    for attempt in 1..=attempts.max(1) {
        last = one_probe(&client)?;
        if last.native_tools {
            return Ok(last);
        }
        last.detail = format!("{} (attempt {attempt} of {attempts})", last.detail);
    }
    Ok(last)
}

/// Enough tries that a model which thinks its way past the call once is not written off for it.
const ATTEMPTS: usize = 3;

fn one_probe(client: &Client) -> Result<Probe> {
    let reply = client.chat(
        &[
            Message::system(
                "You are a creature on a desktop. Answer only by calling exactly one tool. \
                 Never reply with text.",
            ),
            Message::user("[45s] - the user started watching a long video about films"),
        ],
        &capability::schemas(),
    )?;

    let calls = reply.tool_calls.clone().unwrap_or_default();
    let text = reply.content.clone().unwrap_or_default();
    let spent = reply.completion_tokens;
    let cap = client.provider.max_tokens as u64;
    let detail = if !calls.is_empty() {
        calls.iter().map(|c| c.function.name.as_str()).collect::<Vec<_>>().join(", ")
    } else if reply.finish_reason.as_deref() == Some("length") {
        // A reasoning model can spend its whole budget thinking and never reach the call.
        format!(
            "spent its entire {cap}-token budget before answering. Raise [providers.*] max_tokens, \
             or switch thinking off — on ollama that is `extra = {{ think = false }}`"
        )
    } else if text.trim().is_empty() {
        format!("stopped after {spent} tokens having produced neither a call nor any text")
    } else {
        format!("replied with text: {}", crate::sensors::clip(text.trim(), 60))
    };
    Ok(Probe { native_tools: !calls.is_empty(), detail })
}
