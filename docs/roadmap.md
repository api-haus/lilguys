# Roadmap

Sequenced by **what you can see at the end of each phase**, not by subsystem. Every phase ends with
something demonstrable; anything that cannot be demonstrated is in the wrong phase.

Design law is [philosophy.md](philosophy.md). What exists today is
[architecture.md](architecture.md). Each phase links the design it implements.

```mermaid
flowchart TD
    P0["0 · built<br/><i>a cast that reacts</i>"] --> P1["1 · usable<br/><i>somebody else can run it</i>"]
    P1 --> P2["2 · legible<br/><i>you can see what it costs</i>"]
    P2 --> P3["3 · split<br/><i>the mind restarts alone</i>"]
    P3 --> P4["4 · scriptable<br/><i>a drive is a file</i>"]
    P4 --> P5["5 · packaged<br/><i>install a plugin</i>"]
    P5 --> P6["6 · worlds<br/><i>a cast that comes and goes,<br/>in a situation nobody wrote</i>"]
    P3 --> P7["7 · agents<br/><i>the face of hermes</i>"]
```

---

## Phase 0 — built

Where it already is, for reference.

- Layer-shell overlay, wgpu renderer, SDF primitives, glyph atlas
- Graybox skin with a per-character palette; free-flight locomotion, no gravity
- Idle regiment and the reflex arc, each reflex recording itself
- Senses: foreign-toplevel, ext-workspace, idle-notify, MPRIS, inbox socket
- Attention gate — novelty, dwell, budget — and the quantiser
- Reactor: OpenAI-compatible client, auto-compacting context, five capabilities
- Layered prompt: `rules` · `awareness` · `self` · `persona`
- Voice as a command template; piper and espeak, one voice per character
- A roster of guys who witness each other, address each other by name, and share a floor on speech
- `turns.jsonl` and `events.jsonl`; `--print-config`, `--print-prompt`
- `lilguy setup · doctor · start · stop · status · say · provider · model · voice`, all with `--json`
- A message, which is the one thing that closes the current slice instead of waiting for it

---

## Phase 1 — usable by somebody who is not us

**Demo:** a person clones it, runs one command, answers three questions, and ends up with two
characters on their desktop that they can talk to.

Landed: `doctor`, `start/stop/status`, `say`, `Observation::Told` and the slice it cuts short,
per-guy voices, the shared floor on silence, `speak`'s optional `to`, the prompt tuning that makes
being addressed worth answering, `provider`/`model`/`voice`, `setup` — which is all of them in
order, with permission to fix what it finds — and the box that opens when you click one of them.

**Left to prove:** the phase demo itself, on a machine that is not this one. Everything in it is
built; nobody else has run it yet.

**Why first:** everything else is worth more if somebody else can run it, and `lilguy say` unblocked
the message path that Phase 7 needs.

---

## Phase 2 — legible

**Demo:** `lilguy status` prints what each character has cost you today, and `doctor` prints what a
schedule will commit you to tomorrow.

| work | design | size |
|---|---|---|
| Extract the closed sets into one module; every string references it | [taxonomy-and-telemetry](todo/taxonomy-and-telemetry.md) | 1 day |
| `taxonomy` version, in `--check`, in frames, exposed to scripts | [taxonomy-and-telemetry](todo/taxonomy-and-telemetry.md) | hours |
| Tiers: `reflex` · `mind` · `might` · `bulk`, falling back **down, never up** | [model-tiers](todo/model-tiers.md) | 2 days |
| Engine-enforced `per_hour` / `per_day`; refusal as a value with a reason | [model-tiers](todo/model-tiers.md) | 1 day |
| Counters with `{tier, guy, plugin}` attribution, at the existing decision points | [taxonomy-and-telemetry](todo/taxonomy-and-telemetry.md) | 2 days |
| `state.json` snapshot; `lilguy status [--json]` | [taxonomy-and-telemetry](todo/taxonomy-and-telemetry.md) | 1 day |

**Why here:** tiers settle the vocabulary plugins are written against, and attribution has to exist
*before* there are plugins to attribute to, or it never gets retrofitted honestly.

---

## Phase 3 — split

**Demo:** `systemctl --user restart lilguys-soul` and the characters do not flicker.

| work | design | size |
|---|---|---|
| `lilguys-proto`: frame types, version on every frame, serde | [soul-hologram-split](todo/soul-hologram-split.md) | 1 day |
| Split the binary; hologram listens, everything else connects | [soul-hologram-split](todo/soul-hologram-split.md) | 3 days |
| ndjson over WebSocket, both directions | [soul-hologram-split](todo/soul-hologram-split.md) | 1 day |
| Bounded replay buffer, dropping oldest and **emitting a reflective record when it drops** | [soul-hologram-split](todo/soul-hologram-split.md) | 1 day |
| Two systemd units; soul wants hologram, is not required by it | [soul-hologram-split](todo/soul-hologram-split.md) | hours |
| A browser debug view — a static HTML file on the same socket | [soul-hologram-split](todo/soul-hologram-split.md) | 1 day |

**Acceptance:** soul absent is a supported state. Reflexes fire, feelings record, the cast is alive
and simply has nothing to reflect with.

---

## Phase 4 — scriptable

**Demo:** a feeding mechanic is a `.lua` file you drop in, and the character gets hungry.

| work | design | size |
|---|---|---|
| `mlua` with Luau; one host owning the VM, sandbox, interrupt, memory ceiling | [luau-scripting](todo/luau-scripting.md) | 3 days |
| `ctx`: `feel`, `state`, `config`, `log` — nothing else in the first cut | [luau-scripting](todo/luau-scripting.md) | 2 days |
| Stream API: `where` · `map` · `filter` · `dedupe` · `debounce` · `throttle` · `buffer` · `to` | [world-scripting](todo/world-scripting.md) | 3 days |
| `ctx.every`, then `ctx.cron` / `daily` / `weekly` with persisted last-run and catch-up | [world-scripting](todo/world-scripting.md) | 2 days |
| Reserved spend: sum the schedules, report against the tier budget | [model-tiers](todo/model-tiers.md) | 1 day |
| Per-script persistence, debounced | [luau-scripting](todo/luau-scripting.md) | 1 day |
| Rate limits on `ctx.feel`; cycle depth cap on streams | [world-scripting](todo/world-scripting.md) | 1 day |
| Ship `hunger.lua` as the worked example | [luau-scripting](todo/luau-scripting.md) | hours |

**Why after the split:** scripts are policy, they misbehave, and they belong on the side that can
be restarted without killing anyone.

---

## Phase 5 — packaged

**Demo:** `lilguy plugin install gh:someone/moth` and a different creature is on your desktop.

| work | design | size |
|---|---|---|
| Package layout; `lilguy plugin install/list/use/show` | [character-packages](todo/character-packages.md) | 3 days |
| Prompt layers from a plugin — with `rules` **not** overridable | [character-packages](todo/character-packages.md) | 1 day |
| Plugin identity, so telemetry attribution has a subject | [taxonomy-and-telemetry](todo/taxonomy-and-telemetry.md) | 1 day |
| Sprite adapter behind `Avatar`, reading Shimeji sets | [design-space §4](design-space.md) | 3 days |
| Live2D adapter — Cubism Native over FFI | [design-space §4](design-space.md) | 2 weeks |
| VRM adapter — glTF plus morph targets on wgpu | [design-space §4](design-space.md) | 2 weeks |
| Entities: spawn, draw, drag, drop, and the observations each produces | [character-packages](todo/character-packages.md) | 1 week |
| Context menus, hover, declarative entity reflexes | [character-packages](todo/character-packages.md) | 3 days |
| Pinned refs, and `lilguy plugin show` as a real audit against prompt injection | [lilguy-cli](todo/lilguy-cli.md) | 2 days |

**Acceptance:** play with a character for five minutes, then read `events.jsonl` — the session
should be legible from the log alone.

---

## Phase 6 — worlds

**Demo:** install one plugin, get seven characters and a situation that changes daily.

| work | design | size |
|---|---|---|
| Director as stream middleware: filter, rewrite, broadcast, address, author | [world-scripting](todo/world-scripting.md) | 1 week |
| A cast that comes and goes: `world.spawn` / `world.release`, stable ids, arrival and departure as events | [world-scripting](todo/world-scripting.md) | 1 week |
| Global addressing: `plugin/character` as identity, the short name as the address | [character-packages](todo/character-packages.md) | 2 days |
| `llm.run(tier, {tools = …})` — a director model holding world verbs, bounded by a call budget | [world-scripting](todo/world-scripting.md) | 4 days |
| `llm.ask(tier, …)` from Luau, over coroutines; nil on refusal | [world-scripting](todo/world-scripting.md) | 3 days |
| `llm.stream` as a sink that is also a source | [world-scripting](todo/world-scripting.md) | 2 days |
| `world.capability{…}` — per-character availability, schema cap, bounded `on_call` | [world-scripting](todo/world-scripting.md) | 1 week |
| Meaningful tool results, replacing the literal `"ok"` | [world-scripting](todo/world-scripting.md) | 1 day |
| A budget on authored events, so a director cannot narrate constantly | [world-scripting](todo/world-scripting.md) | 1 day |
| Ship a worked cast as the reference plugin | [world-scripting](todo/world-scripting.md) | 1 week |

**The law that governs the whole phase:** a world authors events and offers verbs. It never authors
a character's action. Philosophy rule 7.

**What it is for:** a model holding `spawn`, `release`, `tell` and `broadcast` can stage an evening
— bring Squidward in, make the grill smell wrong, put the lights out — and seven independent minds
answer it in their own voices. That is an episode nobody wrote, including the director, and it is
the whole reason for running language models instead of dialogue trees.

---

## Phase 7 — agents

**Demo:** click a character, ask something, and the conversation continues in Telegram.

Can start once Phase 3 lands; the message path from Phase 1 is the other prerequisite.

| work | design | size |
|---|---|---|
| A minimal peer client as a worked example | [agent-integration](todo/agent-integration.md) | 2 days |
| hermes: a channel adapter for messages | [agent-integration](todo/agent-integration.md) | 3 days |
| hermes: an ambient plugin taking a digest **into memory, never a live context** | [agent-integration](todo/agent-integration.md) | 3 days |
| Routing: which messages go to the reactor and which to the agent | [agent-integration](todo/agent-integration.md) | 2 days |
| `lilguy integrate hermes` | [lilguy-cli](todo/lilguy-cli.md) | 1 day |

**The constraint that decides the design:** hermes treats per-conversation prompt caching as sacred.
Ambient observations go to memory, read at conversation start — never spliced into a running
context.

---

## Not scheduled

Deliberately, and each for its own reason.

- **[object-persistence](todo/object-persistence.md)** — an open question that may be answered by
  doing nothing. Collect the cases where a character wanted to refer to something and could not,
  before building an inventory that undoes the gate.
- **[three-process-split](todo/three-process-split.md)** — trigger-gated. Build it when identity
  outlives the model, or when somebody wants two minds on one soul.
- **Long-term memory** — properly stated, the question of what gets promoted out of the context
  window and **woven into the sutra**, which is the only thing that already persists. A product
  question, not a storage one. The reactor's private notes are the seed and nothing yet reads them
  back.

## How to read the sizes

Days of focused work, not calendar time, and they are the optimistic number. The two-week entries —
Live2D and VRM — are the only ones where the risk is not in the design but in somebody else's SDK.
