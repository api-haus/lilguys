# lilguys

An always-on desktop buddy for Hyprland. He watches, remembers, and reacts — sparingly.
He is the topmost surface of a [hermes](https://github.com/) agent that also answers on Telegram.

## Running it

In the foreground, which is the normal loop — `Ctrl-C` stops it:

```bash
cargo run --release
cargo run --release -- --check          # validate config, probe the model, find the voice binary
cargo run --release -- --print-config   # every setting with its default
cargo run --release -- --print-prompt   # the assembled system prompt, verbatim
```

Always on, which is the point of it:

```bash
cargo install --path .
install -Dm644 packaging/lilguys.service ~/.config/systemd/user/lilguys.service
systemctl --user daemon-reload
systemctl --user enable --now lilguys
journalctl --user -u lilguys -f
```

The unit is `PartOf=graphical-session.target`, so it comes and goes with your session. On Hyprland
that target needs the compositor's environment imported — `uwsm` does it, and without it add
`systemctl --user import-environment WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE` to your config.

Detached without systemd, which survives the shell that started it:

```bash
setsid nohup ./target/release/lilguysd >/tmp/lilguys.log 2>&1 </dev/null &
pkill -x lilguysd    # stop it
```

The model must support **native tool calls**; lilguys does not parse calls out of message text.
`--check` tells you whether yours does before you run anything.

- [Philosophy](docs/philosophy.md) — the mind/body split, what it forbids, and how to add a drive.
  Design law; read it before changing behaviour.
- [Architecture](docs/architecture.md) — how the pieces fit, with diagrams: the three clocks, what
  quantised event flow means, the attention gate, the reactor, the four capabilities.
- [Design space](docs/design-space.md) — what Wayland actually exposes, measured; rendering
  adapters; the attention economy; how this attaches to hermes.
- [Planned work](docs/todo/) — the process split, a setup CLI, sandboxed Luau drives, character
  packages, agent integration.
- [QA](docs/qa-graybox.md) — five manual checks, about three minutes.

Logs land in `~/.local/state/lilguys/`: `turns.jsonl` is every model turn verbatim — slice sent,
raw reply, intents parsed, calls rejected and why — and `events.jsonl` is every gate ruling.

Grayboxing aids are off by default; turn them on per-kind under `[debug]`. `overhead = 4` floats
the last few gate rulings above his head — the debugging instrument that moves with him instead of
covering the screen. `LILGUYS_DEBUG_HTTP=1` prints provider requests and replies. `LILGUYS_DEBUG_INPUT=1`
prints the input rectangle and `LILGUYS_DEBUG_MPRIS=1` prints every D-Bus media signal.
