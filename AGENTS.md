# lilguys — working on this repo

Read [docs/philosophy.md](docs/philosophy.md) **before** changing behaviour. It is design law, not
background: it decides ambiguous cases, and its eight prohibitions apply whether or not a change
compiles and works.

- [docs/architecture.md](docs/architecture.md) — how the current build fits together, with diagrams
- [docs/roadmap.md](docs/roadmap.md) — phases from here on, each ending in something demonstrable
- [docs/todo/](docs/todo/) — the twelve designs those phases implement
- [docs/design-space.md](docs/design-space.md) — what Wayland exposes, **measured on real hardware**
- [docs/telemetry.md](docs/telemetry.md) — what is counted, and the development Grafana stack
- [docs/qa-graybox.md](docs/qa-graybox.md) — five manual checks, about three minutes
- [docs/office.md](docs/office.md) — the pivot: guys live in any place, seen through any view; the office laid over a messenger

## The shape in one paragraph

`lilguysd` draws **any number of characters** on a wlr-layer-shell overlay. Each **body** runs at
60 Hz with no language and no model: it drifts, blinks, watches the pointer, and answers what it
senses immediately and for free. Each **mind** is a language model reading a slow, quantised account
of what its body has been through, answering with five kinds of intention. Between them sits an
attention gate whose job is to discard most of what it sees. Guys witness each other's actions as
ordinary observations.

## Hard rules

1. **The mind never reads a sensor; the body never waits for the mind.** Everything in
   philosophy.md §"What this forbids" holds. Consequence is not perception — a capability may answer
   the character who used it.
2. **Native tool calls only.** A model that writes `react(pleased)` into message text is
   misconfigured. Report it; never parse around it.
3. **Never let system text reach a person.** `speak` and `think` are vetted in
   `mind/capability.rs::vet_leakage`. Adding a path from prompt or schema text to the voice or a
   bubble is a defect, not a feature.
4. **The sutra is the only thing that persists, so it must not move.** The prompt is a thread, not
   a document — the body forgets on restart and the context window is compacted away, and the thread
   is what makes this the same character tomorrow. The engine weaves in `{name}`, `{others}`,
   `{capabilities}` and friends, and a line whose placeholder is empty is dropped whole. Anything
   that *changes* goes in the event stream instead: a thread that moves is not a thread.
5. **Config names nothing internal.** `lilguys.default.toml` is the whole surface and describes
   behaviour, not types. Every field must be present with its default — `Config::default()` parses
   that file, so a missing field is a startup failure.
6. **There is never only one guy.** The roster is a `Vec`; nothing may assume a single character.
   It is fixed at startup today and will not stay that way — a world spawns and releases characters
   — so anything newly written against a stable position is work somebody has to undo.
7. **Every reflex records itself**, and a reflex record never triggers another reflex.
8. **Whatever authors the world may not author a character.** Events in and verbs offered; never
   intents.
9. **Grayboxing aids off by default** (`[debug]`). The graybox skin is the shipping look, not
   scaffolding.
10. **Secrets are named, never stored.** Providers reference an environment variable.

## Commands

```bash
cargo build --release
cargo test --release
cargo lilguy doctor                       # in this tree; the alias takes any subcommand below
./target/release/lilguy setup             # doctor, plus permission to fix what it finds
./target/release/lilguy doctor            # every check: config, wayland, NATIVE TOOL CALLS, voice
./target/release/lilguy start | stop      # the unit if installed, a detached process if not
./target/release/lilguy restart           # config is read once at startup; this is how it lands
./target/release/lilguy say "…"           # a message reaches them at once, not at the next quantum
./target/release/lilguy status --json     # who is running, and every counter with its attribution
./target/release/lilguysd --print-config  # every setting with its default
./target/release/lilguysd --print-prompt  # the assembled system prompt, verbatim
```

`lilguy start` puts the daemon in its own process group, so a tool-call timeout cannot take it
down with the caller; running `./target/release/lilguysd` in the foreground does not, and a
timeout there kills the whole group. Every subcommand takes `--json`, exits non-zero when
something is wrong, and is safe to run twice — the caller is usually another program.

For a real always-on install see `packaging/lilguys.service` and the README.

## Diagnose from the logs, never from a theory

`~/.local/state/lilguys/` — read these **first**, every time.

- **`turns.jsonl`** — per model turn: the slice sent verbatim, raw `content`, raw `tool_calls`,
  intents parsed, calls **rejected and why**, tokens, compaction, error, round-trip ms.
- **`events.jsonl`** — every gate ruling, with its verdict and which guy made it.

```bash
python3 -c "
import json, pathlib, os
p = pathlib.Path(os.path.expanduser('~/.local/state/lilguys/turns.jsonl'))
for l in p.read_text().splitlines()[-10:]:
    d = json.loads(l); c = d['tool_calls'] or []
    print([x['function']['name'] + x['function']['arguments'][:60] for x in c] or '(none)',
          d['error'] or '')"
```

Every diagnosis this project has needed came out of those two files, and none were guessable:

- *"It repeats itself"* — every tool result is the literal `"ok"`, a null signal, so a small model
  simply continues its own strongest pattern.
- *"It's always in shock"* — `apply_emotion` returned early once a hold expired, so nothing eased
  the face back and the last expression froze on permanently.
- *"They're silent"* — the slice was twenty-four lines of one image viewer announcing every PNG it
  opened. The models were correctly saying nothing about noise.

`state.json` in the same directory answers what anything cost without reading either — see
[docs/telemetry.md](docs/telemetry.md). Read it through `lilguy status --json` rather than by eye.

Environment switches: `LILGUYS_DEBUG_HTTP` (provider request and reply), `LILGUYS_DEBUG_MPRIS`
(every D-Bus media signal and its extracted fields), `LILGUYS_DEBUG_INPUT` (input rectangles),
`LILGUYS_CONFIG` (an alternate config, for experiments that must not touch the live one).

## Traps this project has already hit

Each cost real time. None are visible from the code.

- **`include_str!` bakes `lilguys.default.toml` at compile time.** Edit it and cargo may not notice;
  `touch src/config.rs` before rebuilding or you will debug a stale default.
- **`Config::default()` parses that TOML**, so a struct-level `#[serde(default)]` recurses into a
  stack overflow. Field-level defaults only, and the file must be complete.
- **`#[serde(flatten)]` and `deny_unknown_fields` are incompatible.** `Prompt` needs flatten, so it
  cannot deny.
- **An unset input region is INFINITE**, swallowing every click on the desktop. Set an empty one
  before the first commit of the surface.
- **Reasoning models spend the whole budget thinking.** qwen3:4b needs `max_tokens` in the thousands
  or it stops mid-thought having called nothing. `--check` names this case exactly.
- **`OwnedValue` derefs to `Value`; downcasting it yields nothing** and silently empties every MPRIS
  field.
- **Expressions must decay.** Any layer that eases parameters has to run every tick, not only while
  it has something to say, or the face freezes where it was left.
- **Novelty is per key, so a source minting a fresh key each time sails straight through it.** Hence
  `per_source_cap`.
- **A semicolon inside a mermaid `Note` terminates the statement.** Scan diagrams before committing.
- **The tool-call probe is flaky by nature.** A reasoning model sometimes thinks its way past the
  call and answers nothing, which says nothing about whether it *can*. `probe_provider` asks three
  times before condemning a model; a single failed probe is not evidence.
- **`toml_edit` indexing panics by value and inserts by reference.** `node[key].is_none()` on a
  missing key panics; `&mut node[key]` creates it. Writing a config the other way loses the file.
- **A GPU-queue job may be holding tickets.** `processqueue gpu --info` before blaming a timeout.

## Where things live

| path | what |
|---|---|
| `src/bin/lilguy.rs` | the command a person or their agent drives; `src/main.rs` is the daemon |
| `src/doctor.rs` | every check, shared by `lilguy doctor` and `lilguysd --check` |
| `src/service.rs` | finding, starting and stopping the daemon, with or without systemd |
| `src/setup.rs` | what this machine has, and the wizard that turns it into a config that boots |
| `src/app.rs` | wiring: surface, tick, input region, painting, intent enactment, the reflex map |
| `src/guy.rs` | one roster member — body, gate, quantiser, mind, voice, cross-perception |
| `src/locomotion.rs` | the body — steering, idle regiment, its own feelings |
| `src/avatar/` | the `Avatar` seam, the `Pose`/`Drive` vocabulary, the graybox skin |
| `src/sensors/` | exteroception (wayland, mpris) and interoception (inbox socket) |
| `src/attention.rs` | the gate: novelty, dwell, budget, flood damping |
| `src/mind/` | reactor, provider client, the five capabilities |
| `src/voice.rs` | TTS as a command template |
| `src/config.rs` | the whole configurable surface |
| `src/log.rs` | the two JSONL sinks |
| `src/telemetry.rs` | attributed counters, `state.json`, and the OTLP push |
| `characters/` | shipped characters: persona, palette, size |
| `packaging/` | the systemd user unit, and the development observability stack |
| `.cargo/config.toml` | the `cargo lilguy` alias, so the CLI runs without a path or an install |

## Adding things

- **A sense** pushes an `Observation` onto a guy's bus and adds a `[senses]` switch. Push-based, or
  near-free to poll — design-space.md has the ladder with measurements.
- **A drive** does not go in this repo. It writes JSON lines to `$XDG_RUNTIME_DIR/lilguys.sock`.
- **A character** is a file in `characters/`: persona, palette, size. A `[[guys]]` block puts them
  on screen. Several may share one character file.
- **A skin** implements `Avatar` over the same `Pose` and `Drive`, and must not need new parameters
  before exhausting the twenty that exist.
- **A capability** is the expensive one — five is deliberate, and every schema costs tokens on every
  turn for every guy. A sixth needs an argument in philosophy.md's terms.
- **A plugin** is never a persona. It is the install unit and may carry a whole cast, its drives and
  its world scripts. `lilguy plugin install`, never `lilguy persona install`.

## Verifying behaviour, not just code

`cargo test` covers the speech vetting and little else, because most of this is a live system. The
honest checks:

1. `--check` green on every line, including native tool calls.
2. [docs/qa-graybox.md](docs/qa-graybox.md) — click-through, gaze across a facing flip, restraint,
   media, cost.
3. Run for ten minutes with `[debug] overhead = 4` and read `turns.jsonl`. **Long slices with absent
   calls mean the gate is leaking, not the model failing.**
4. Measure, do not assert: `/proc/$(pgrep -x lilguysd)/stat` for CPU, `VmRSS` for memory. Current
   figures are in design-space.md and belong updated when they move.
