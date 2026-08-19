//! The gate between noticing and thinking. Most observations must die here — see docs/design-space.md §5.

use crate::sensors::{Observation, Sensed};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Rules {
    /// Observations one source may contribute to a slice before the rest are dropped. Novelty is
    /// per key, so a source that mints a fresh key every time — a viewer whose "track" is a
    /// filename — sails straight through it and floods the slice.
    pub per_source_cap: usize,
    /// An observation whose key repeats inside this window says nothing new.
    pub novelty_window: Duration,
    /// A focus must hold this long before it is worth a thought.
    pub focus_dwell: Duration,
    /// A track must play this long before its transcript is worth fetching.
    pub media_dwell: Duration,
    /// Thoughts allowed per hour. The budget refills continuously.
    pub thoughts_per_hour: f32,
    /// Reactions the buddy shows without thinking. Cheap, so the ceiling is high.
    pub emotes_per_hour: f32,
}

impl From<&crate::config::Config> for Rules {
    fn from(c: &crate::config::Config) -> Self {
        Self {
            novelty_window: c.senses.novelty_window,
            focus_dwell: c.senses.focus_dwell,
            media_dwell: c.senses.media_dwell,
            thoughts_per_hour: c.mind.turns_per_hour,
            emotes_per_hour: 90.0,
            per_source_cap: c.senses.per_source_cap,
        }
    }
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            novelty_window: Duration::from_secs(600),
            focus_dwell: Duration::from_secs(90),
            media_dwell: Duration::from_secs(60),
            thoughts_per_hour: 12.0,
            emotes_per_hour: 90.0,
            per_source_cap: 3,
        }
    }
}

/// What the gate decided to do about one observation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Seen before, or too transient to matter. The common case, by design.
    Ignored,
    /// Waiting out a dwell timer. It may yet become a thought.
    Pending,
    /// Worth a flicker of expression, nothing more. No tokens spent.
    Emote,
    /// Worth waking the agent for. Rare, and budgeted.
    Think,
}

impl Verdict {
    pub fn tag(&self) -> &'static str {
        match self {
            Verdict::Ignored => "·",
            Verdict::Pending => "~",
            Verdict::Emote => "!",
            Verdict::Think => "*",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub at: Instant,
    pub verdict: Verdict,
    pub text: String,
}

/// A refilling allowance. Spending is only possible when a whole unit has accrued.
struct Budget {
    per_hour: f32,
    credit: f32,
    last: Instant,
}

impl Budget {
    /// Starts full. A buddy that has just woken up should not be at its stingiest.
    fn new(per_hour: f32) -> Self {
        Self { per_hour, credit: (per_hour * 0.25).max(1.0), last: Instant::now() }
    }

    fn refill(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last).as_secs_f32();
        self.last = now;
        self.credit = (self.credit + dt * self.per_hour / 3600.0).min(self.per_hour * 0.25);
    }

    fn spend(&mut self, now: Instant) -> bool {
        self.refill(now);
        if self.credit >= 1.0 {
            self.credit -= 1.0;
            return true;
        }
        false
    }
}

pub struct Attention {
    pub rules: Rules,
    seen: HashMap<String, Instant>,
    /// Observations serving a dwell before they count.
    waiting: HashMap<String, (Instant, Observation)>,
    thoughts: Budget,
    emotes: Budget,
    pub log: VecDeque<Entry>,
    pub counts: [u32; 4],
    fresh: usize,
    /// How much each source has contributed inside the current novelty window.
    flood: HashMap<String, (Instant, usize)>,
}

impl Attention {
    const LOG_CAP: usize = 12;

    pub fn new(rules: Rules) -> Self {
        Self {
            thoughts: Budget::new(rules.thoughts_per_hour),
            emotes: Budget::new(rules.emotes_per_hour),
            rules,
            seen: HashMap::new(),
            waiting: HashMap::new(),
            log: VecDeque::new(),
            counts: [0; 4],
            fresh: 0,
            flood: HashMap::new(),
        }
    }

    /// Feeds new observations in and matures anything whose dwell has elapsed.
    pub fn consider(&mut self, incoming: Vec<Sensed>, now: Instant) -> Vec<(Verdict, Observation)> {
        let mut out = Vec::new();
        self.fresh = 0;
        for s in incoming {
            // Judge by when it happened, not when it was drained. A buffered burst must not all
            // look simultaneous, and a replayed one must not look fresh.
            let v = self.judge(&s.what, s.at);
            self.record(v, &s.what, s.at);
            if matches!(v, Verdict::Emote | Verdict::Think) {
                out.push((v, s.what));
            }
        }
        for (verdict, what) in self.mature(now) {
            self.record(verdict, &what, now);
            out.push((verdict, what));
        }
        out
    }

    fn judge(&mut self, what: &Observation, now: Instant) -> Verdict {
        // A source that talks constantly is damped whatever it says. Without this, one image
        // viewer announcing every file it opens drowns out the entire desktop.
        let source = what.source();
        let seen = self.flood.entry(source).or_insert((now, 0));
        if now.duration_since(seen.0) > self.rules.novelty_window {
            *seen = (now, 0);
        }
        seen.1 += 1;
        if seen.1 > self.rules.per_source_cap {
            return Verdict::Ignored;
        }

        let key = what.key();
        if let Some(prev) = self.seen.get(&key) {
            if now.duration_since(*prev) < self.rules.novelty_window {
                return Verdict::Ignored;
            }
        }
        self.seen.insert(key.clone(), now);

        match what {
            // Presence and workspace changes are ambient: they colour the mood, they are never
            // worth a token on their own.
            // A feeling is the body's own immediate feedback and costs nothing to answer. Budget
            // exists to protect tokens; rationing a free response only makes the body feel dead.
            Observation::Feeling(_) => Verdict::Emote,
            // Ambient changes colour the mood too, but they arrive from outside and can flood, so
            // they are rationed. Either way both ride along in the next slice for the mind.
            Observation::Presence { .. } | Observation::Workspace { .. } => {
                if self.emotes.spend(now) {
                    Verdict::Emote
                } else {
                    Verdict::Ignored
                }
            }
            // Anything that might deserve a thought must first survive its dwell.
            Observation::Focus { .. } | Observation::Title { .. } | Observation::Media { .. } => {
                self.waiting.insert(key, (now, what.clone()));
                Verdict::Pending
            }
        }
    }

    /// Promotes anything that outlasted its dwell, and drops anything superseded meanwhile.
    fn mature(&mut self, now: Instant) -> Vec<(Verdict, Observation)> {
        let rules = self.rules.clone();
        let ready: Vec<String> = self
            .waiting
            .iter()
            .filter(|(_, (started, what))| {
                let dwell = match what {
                    Observation::Media { .. } => rules.media_dwell,
                    _ => rules.focus_dwell,
                };
                now.saturating_duration_since(*started) >= dwell
            })
            .map(|(k, _)| k.clone())
            .collect();

        ready
            .into_iter()
            .filter_map(|k| self.waiting.remove(&k))
            .map(|(_, what)| {
                let v = if self.thoughts.spend(now) { Verdict::Think } else { Verdict::Emote };
                (v, what)
            })
            .collect()
    }

    fn record(&mut self, verdict: Verdict, what: &Observation, at: Instant) {
        self.counts[match verdict {
            Verdict::Ignored => 0,
            Verdict::Pending => 1,
            Verdict::Emote => 2,
            Verdict::Think => 3,
        }] += 1;
        self.log.push_front(Entry { at, verdict, text: what.summary() });
        self.log.truncate(Self::LOG_CAP);
        self.fresh = (self.fresh + 1).min(Self::LOG_CAP);
    }

    /// Whole thoughts currently affordable — the HUD reads this to show restraint accruing.
    pub fn thought_credit(&self) -> f32 {
        self.thoughts.credit
    }

    /// Entries added by the most recent `consider`, newest first.
    pub fn fresh(&self) -> usize {
        self.fresh
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}
