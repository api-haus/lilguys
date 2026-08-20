# lilguys

An always-on desktop buddy for Hyprland. He watches, remembers, and reacts — sparingly.
He is the topmost surface of a [hermes](https://github.com/) agent that also answers on Telegram.

## Running it

`lilguysd` is the daemon and `lilguy` is what you talk to. Everything below is `lilguy`, so
nobody has to know the daemon has flags:

```bash
cargo build --release
./target/release/lilguy setup           # find a model, find a voice, put them on screen
./target/release/lilguy doctor          # every check, one line each, with the fix for what is wrong
./target/release/lilguy status          # who is running, who is on screen
./target/release/lilguy say "hello"     # talk to them; an answer is optional
./target/release/lilguy start | stop | restart
```

`setup` is `doctor` with permission to fix things: it probes the local endpoints for a model that
can do **native tool calls**, offers to pull one if nothing can, finds or fetches a voice, and
installs the systemd unit. Running it twice changes nothing the second time.

Every command takes `--json` and exits non-zero when something is wrong, because in practice the
thing setting this up is somebody's coding agent rather than somebody's afternoon. Nothing is ever
downloaded or installed without being asked; `--yes` is how a caller answers in advance.

The config is read once, when the daemon starts. Anything that writes it — `setup`, `provider use`,
`model use`, `voice use` — reaches a running daemon only after `lilguy restart`.

```bash
./target/release/lilguy provider list   # what answers on this machine, and what it can run
./target/release/lilguy model use qwen3:4b
./target/release/lilguy voice list      # engines present, voices downloaded
./target/release/lilguy voice test      # hear it, rather than believe it
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

The daemon's own flags, for when you are working on it:

```bash
cargo lilguy doctor                     # `cargo lilguy` is any lilguy subcommand, from this tree
cargo run --release
cargo run --release -- --check          # the same report `lilguy doctor` prints
cargo run --release -- --print-config   # every setting with its default
cargo run --release -- --print-prompt   # the assembled system prompt, verbatim
```

The model must support **native tool calls**; lilguys does not parse calls out of message text.
`lilguy doctor` tells you whether yours does before you run anything.

- [Philosophy](docs/philosophy.md) — the mind/body split, what it forbids, and how to add a drive.
  Design law; read it before changing behaviour.
- [Architecture](docs/architecture.md) — how the pieces fit, with diagrams: the three clocks, what
  quantised event flow means, the attention gate, the reactor, the four capabilities.
- [Design space](docs/design-space.md) — what Wayland actually exposes, measured; rendering
  adapters; the attention economy; how this attaches to hermes.
- [Roadmap](docs/roadmap.md) — phases from here to a scripted cast, each ending in something you
  can see, with sizes.
- [Planned work](docs/todo/) — the twelve designs those phases implement.
- [QA](docs/qa-graybox.md) — five manual checks, about three minutes.

Logs land in `~/.local/state/lilguys/`: `turns.jsonl` is every model turn verbatim — slice sent,
raw reply, intents parsed, calls rejected and why — and `events.jsonl` is every gate ruling.

Grayboxing aids are off by default; turn them on per-kind under `[debug]`. `overhead = 4` floats
the last few gate rulings above his head — the debugging instrument that moves with him instead of
covering the screen. `LILGUYS_DEBUG_HTTP=1` prints provider requests and replies. `LILGUYS_DEBUG_INPUT=1`
prints the input rectangle and `LILGUYS_DEBUG_MPRIS=1` prints every D-Bus media signal.
