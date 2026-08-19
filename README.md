# lilguys

An always-on desktop buddy for Hyprland. He watches, remembers, and reacts — sparingly.
He is the topmost surface of a [hermes](https://github.com/) agent that also answers on Telegram.

```bash
cargo build --release
./target/release/lilguysd --print-config > ~/.config/lilguys/lilguys.toml   # optional
./target/release/lilguysd --check                                          # validates and probes
./target/release/lilguysd
```

The model must support **native tool calls**; lilguys does not parse calls out of message text.
`--check` tells you whether yours does before you run anything.

- [Architecture](docs/architecture.md) — how the pieces fit, with diagrams: the three clocks, what
  quantised event flow means, the attention gate, the reactor, the four capabilities.
- [Design space](docs/design-space.md) — what Wayland actually exposes, measured; rendering
  adapters; the attention economy; how this attaches to hermes.
- [QA](docs/qa-graybox.md) — five manual checks, about three minutes.

Debug output: `LILGUYS_DEBUG_INPUT=1` prints the input rectangle, `LILGUYS_DEBUG_MPRIS=1` prints
every D-Bus media signal and the fields extracted from it.
