# lilguys — working on this repo

Read [docs/philosophy.md](docs/philosophy.md) **before** changing behaviour. It is design law, not
background: it decides ambiguous cases, and its six prohibitions apply whether or not a change
compiles and works. [docs/architecture.md](docs/architecture.md) says how the current build fits
together, with diagrams.

## The shape in one paragraph

`lilguysd` draws a character on a wlr-layer-shell overlay. The **body** runs at 60 Hz with no
language and no model: it drifts, blinks, watches the pointer, and answers what it senses
immediately and for free. The **mind** is a language model that reads a slow, quantised account of
what the body has been through and answers with four kinds of intention. Between them sits an
attention gate whose job is to discard most of what it sees.

## Hard rules

1. **The mind never perceives directly, and the body never waits for it.** Everything in
   philosophy.md §"What this forbids" holds.
2. **Native tool calls only.** A model that writes `react(pleased)` into message text is
   misconfigured. Report it; never parse around it. A reasoning model needs a `max_tokens` in the
   thousands or it stops mid-thought having called nothing — `--check` names that case.
   `LILGUYS_DEBUG_HTTP=1` prints the request body and reply when a provider misbehaves.
3. **Never speak system text.** `speak` is the one capability vetted, in
   `mind/capability.rs::vet_speech`. Adding a way for prompt or schema text to reach the voice is a
   defect, not a feature.
4. **Config names nothing internal.** `lilguys.default.toml` is the whole surface and it describes
   behaviour, not types. Every field must be there with its default — `Config::default()` parses
   that file, so a missing field is a startup failure.
5. **Grayboxing aids stay off by default.** `[debug]` gates them. The graybox skin itself is the
   shipping default look, not scaffolding.
6. **Secrets are named, never stored.** Providers reference an environment variable.

## Working on it

```bash
cargo build --release
./target/release/lilguysd --check        # config, provider, native tool calls, voice binary
./target/release/lilguysd --print-config # every setting with its default
./target/release/lilguysd --print-prompt # the assembled system prompt, verbatim
cargo test --release
```

Logs are in `~/.local/state/lilguys/`. `turns.jsonl` records every model turn verbatim — slice
sent, raw reply, intents parsed, calls rejected and why. `events.jsonl` records every gate ruling.
**Read these before theorising about misbehaviour.** Manual checks are in
[docs/qa-graybox.md](docs/qa-graybox.md).

Run the daemon detached (`setsid nohup … &`) — a timeout on the launching shell kills the whole
process group otherwise. `pkill -x lilguysd` stops it. For a real always-on install see
`packaging/lilguys.service` and the README.

## Where things live

| path | what |
|---|---|
| `src/app.rs` | wiring: surface, tick, input region, reflex arc, intent enactment |
| `src/locomotion.rs` | the body — steering, idle regiment, its own feelings |
| `src/avatar/` | the `Avatar` seam, the `Pose`/`Drive` vocabulary, the graybox skin |
| `src/sensors/` | exteroception (wayland, mpris) and interoception (inbox socket) |
| `src/attention.rs` | the gate: novelty, dwell, budget |
| `src/mind/` | the reactor, provider client, the four capabilities |
| `src/voice.rs` | TTS as a command template |
| `src/config.rs` | the whole configurable surface |

## Planned, not built

- [docs/todo/soul-hologram-split.md](docs/todo/soul-hologram-split.md) — two processes, so the mind
  can restart without the character blinking out. Do this first; the scripting work lands on top.
- [docs/todo/luau-scripting.md](docs/todo/luau-scripting.md) — sandboxed Luau drives and character
  packages, composing through the observation stream rather than through each other.
- [docs/todo/three-process-split.md](docs/todo/three-process-split.md) — pulling the mind out of the
  soul so the part iterated on constantly is the cheapest to discard. Trigger-gated; not soon.

## Adding things

- **A sense** pushes `Observation` onto the bus and adds a `[senses]` switch. It must be push-based
  or near-free to poll; see design-space.md for the introspection ladder and its measurements.
- **A drive** does not go in this repo. It writes JSON lines to `$XDG_RUNTIME_DIR/lilguys.sock`.
- **A skin** implements `Avatar` over the same `Pose` and `Drive`. It must not need new parameters
  before exhausting the twenty that exist.
- **A capability** is the expensive one. Four is a deliberate number; adding a fifth means arguing
  in philosophy.md's terms why the existing four cannot express it.
