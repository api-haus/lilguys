# lilguys

An always-on desktop buddy for Hyprland. He watches, remembers, and reacts — sparingly.
He is the topmost surface of a [hermes](https://github.com/) agent that also answers on Telegram.

```bash
cargo build --release
./target/release/lilguysd --print-config > ~/.config/lilguys/lilguys.toml   # optional
./target/release/lilguysd --check                                          # validates and probes
./target/release/lilguysd --print-prompt                                   # the assembled prompt
./target/release/lilguysd
```

The model must support **native tool calls**; lilguys does not parse calls out of message text.
`--check` tells you whether yours does before you run anything.

- [Philosophy](docs/philosophy.md) — the mind/body split, what it forbids, and how to add a drive.
  Design law; read it before changing behaviour.
- [Architecture](docs/architecture.md) — how the pieces fit, with diagrams: the three clocks, what
  quantised event flow means, the attention gate, the reactor, the four capabilities.
- [Design space](docs/design-space.md) — what Wayland actually exposes, measured; rendering
  adapters; the attention economy; how this attaches to hermes.
- [Planned work](docs/todo/) — the soul/hologram process split, sandboxed Luau drives.
- [QA](docs/qa-graybox.md) — five manual checks, about three minutes.

Logs land in `~/.local/state/lilguys/`: `turns.jsonl` is every model turn verbatim — slice sent,
raw reply, intents parsed, calls rejected and why — and `events.jsonl` is every gate ruling.

Grayboxing aids are off by default; turn them on per-kind under `[debug]`. `LILGUYS_DEBUG_INPUT=1`
prints the input rectangle and `LILGUYS_DEBUG_MPRIS=1` prints every D-Bus media signal.
