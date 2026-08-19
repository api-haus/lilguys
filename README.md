# lilguys

An always-on desktop buddy for Hyprland. He watches, remembers, and reacts — sparingly.
He is the topmost surface of a [hermes](https://github.com/) agent that also answers on Telegram.

```bash
cargo build --release && ./target/release/lilguysd
```

- [Design space](docs/design-space.md) — what Wayland actually exposes, measured; rendering
  adapters; the attention economy; how this attaches to hermes.
- [QA](docs/qa-graybox.md) — five manual checks, about three minutes.

Debug output: `LILGUYS_DEBUG_INPUT=1` prints the input rectangle, `LILGUYS_DEBUG_MPRIS=1` prints
every D-Bus media signal and the fields extracted from it.
