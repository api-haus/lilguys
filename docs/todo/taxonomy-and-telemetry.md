# Taxonomy and telemetry

**Status:** telemetry built, taxonomy started. **Why:** plugins compose through a vocabulary, and
nobody can see what anything costs.

The counters, `state.json`, `lilguy status` and the closed-set accessors behind their attributes
are in — [telemetry.md](../telemetry.md) is the reference. Naming an OTLP collector also pushes
them to the development Grafana stack, which the shape below did not anticipate and which changes
nothing about it: the local file stays the product, and nothing is sent anywhere the person did
not start themselves. What is left is the taxonomy as a *contract* — the version, the plugin
declaration, and `plugin` as the third leg of attribution.

Two problems that look separate and are the same problem: **what things are called**, and **what
they are doing**. Both are currently implicit — kinds are ad-hoc strings scattered through the
code, and the only visibility is a log you have to read by eye.

Neither matters while we are the only authors. Both become load-bearing the moment somebody else
ships a plugin.

## The taxonomy is a contract

A plugin author writes `:where{ kind = "focus" }` and has to be right about it a year from now.
That makes the vocabulary an interface with a version, not a set of strings that happen to be in
use.

### Closed sets, owned by the core

These may only change with a taxonomy version bump, and a plugin may declare a minimum.

| set | members |
|---|---|
| observation kinds | `focus` · `title` · `workspace` · `presence` · `media` · `feeling` · `told` |
| verdicts | `ignored` · `pending` · `emote` · `think` |
| body capabilities | `react` · `gesture` · `think` · `speak` · `focus` |
| emotions | `neutral` · `curious` · `pleased` · `amused` · `surprised` · `concerned` · `bored` · `sleepy` |
| gestures | `wave` · `nod` · `shake` · `shrug` · `point` · `stretch` · `bounce` · `slump` |
| tiers | `reflex` · `mind` · `might` · `bulk` |
| pose parameters | the twenty Live2D-shaped names in `avatar::Param` |

### Open sets, where extension lives

Deliberately unbounded, so a plugin never needs the core to change:

- **`feeling.source`** — `body`, `reflex`, a drive's name, a plugin's name.
- **`feeling.state`** — `starving`, `picked up`, anything a script invents.
- **scripted capabilities** — namespaced by plugin (`bikini-bottom.cook`) so two packages cannot
  collide, and so a tool name in a log says who is responsible for it.

That split is the whole design: **the core owns the shape, plugins own the content.** A feeling is
always `{source, state, detail, tone, intensity, hold}`; what goes in those fields is nobody's
business but the author's.

### Versioning

`taxonomy = 1` in the core, exposed to scripts and named in every protocol frame. A plugin declares
`requires_taxonomy = 1` and is refused at install time if the core is older. Additive changes bump
the minor; removing or renaming a closed-set member bumps the major and is expected to be rare
enough to be embarrassing.

## Telemetry

The logs already record everything that happens. What is missing is anything that answers a
question without reading them.

### The questions worth answering

1. **What is this costing me?** Per tier, per character, per plugin, per hour and per day. Nobody
   else can attribute cost to a *character*, and with a cast that is the number people will care
   about most.
2. **Why did nothing happen?** A budget was spent, a dwell had not elapsed, a tier was unconfigured,
   a call was refused. Every one of those is already a decision the code makes and discards.
3. **Is it behaving?** Gate throughput by verdict, refused calls by reason, buffer drops, turn
   latency, how often a model is asked and how often it declines to act.
4. **Which plugin is responsible?** Every spend, every authored event, every refusal, attributed.

### Shape

Counters and histograms in memory, snapshotted to `state.json` beside the logs, and readable as
`lilguy status --json`. The volume is dozens of events a minute and the audience is one person and
one CLI, so nothing here needs a daemon of its own.

A development collector is the one exception, and it is opt-in: `[telemetry] otlp` points at a
localhost OTLP endpoint and the same counters go up as OTLP/JSON over the `ureq` already in the
tree. Off unless named, and it adds no dependency — a real OpenTelemetry SDK measured at forty
crates, tokio and hyper among them, for a daemon with no async runtime at all.

```json
{
  "uptime_s": 41230,
  "guys": {
    "SpongeBob": { "turns": 88, "tokens": 41200, "spoke": 3, "thought": 71, "ignored": 1420 },
    "lil":       { "turns": 84, "tokens": 39100, "spoke": 0, "thought": 66, "ignored": 1455 }
  },
  "tiers": {
    "mind":  { "used_hour": 41, "limit_hour": 60, "used_day": 172 },
    "might": { "used_hour": 0,  "limit_hour": 6,  "used_day": 2, "reserved_day": 1 }
  },
  "refusals": { "over budget": 12, "speak vetted": 3, "tier unconfigured": 0 },
  "gate": { "ignored": 2875, "pending": 190, "emote": 141, "think": 172 },
  "drops": { "sensor buffer": 0, "voice queue": 4 }
}
```

### Rules

- **Never sent anywhere.** This is a local file and a local command. There is no opt-in analytics
  question to answer because there is nothing to opt into.
- **Attribution is mandatory.** A spend without a `{tier, guy, plugin}` triple is a bug. Anonymous
  cost is cost you cannot govern.
- **Refusals are counted, not just logged.** "It did nothing for an hour" must be answerable in one
  command.
- **Cheap enough to always be on.** Counters, not traces.

## Work

1. ~~Counter registry with attribution, incremented where decisions already happen — the gate,
   the budget, the vetting, the buffers.~~
2. ~~`state.json` snapshot on a slow timer, and the cost lines in `lilguy status [--json]`.~~
3. ~~The closed sets each behind one accessor — `Observation::kind`, `Verdict::name`,
   `Intent::kind`, `Drift::name`, `Emotion::name`.~~ Still typed out as bare strings elsewhere.
4. Extract those accessors into one module that owns the vocabulary, rather than one per type.
5. `taxonomy` constant, exposed in `--check`, in protocol frames, and to scripts.
6. `tier` as an attribute, which needs [model-tiers.md](model-tiers.md); `provider` stands in for
   it today.
7. Plugin identity, so attribution has a third leg — arrives with
   [character-packages.md](character-packages.md).
8. Snapshot at shutdown, not only on the timer.

## Open questions

- Should the taxonomy be *data* — a file the core ships and scripts read — rather than compiled in?
  It would let a plugin see the whole vocabulary reflectively, which is nice for tooling and one
  more thing to keep in sync.
- Per-token cost estimates need a price table per provider, which is a maintenance burden and goes
  stale. Counting calls and tokens is honest; converting to money is a promise. Probably count, and
  let the CLI multiply by a figure the person supplies.
