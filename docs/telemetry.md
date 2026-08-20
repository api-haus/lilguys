# Telemetry

The logs record what happened. Telemetry answers a question without reading them.

Two readers, and they read the same counters:

- **`lilguy status`** — always available, no stack, no container. The cost lines come from
  `state.json`, which the daemon rewrites every thirty seconds beside the logs.
- **Grafana** — only while you are developing, only if you started the stack, and only over the
  loopback.

Nothing is ever sent anywhere. There is no opt-in analytics question to answer because there is
nothing to opt into: the file is local, and the collector is one you start yourself and can stop.

## What it costs, in one command

```console
$ lilguy status
ok    daemon     pid 395124 · unit active · enabled at login
ok    roster     lil, SpongeBob
ok    frames     1760 at 60 Hz · render p99 0.5 ms · gap p99 25.0 ms
ok    lil        turns 84 · tokens 39100 · spoke 3 · ignored 1455
ok    gate       ignored 2875 · pending 190 · emote 141 · think 172
ok    refusals   over budget 12 · speech vetted 3
ok    uptime     11h 27m
```

`lilguy status --json` returns the whole snapshot, including a `series` object with every counter
under its own attributed name. Anything the curated lines above do not answer is in there.

## What is counted

Every spend carries `who`, and a model spend also carries `provider`. A number nobody can pin on a
character is a number nobody can govern, so anonymous counters are a defect.

| metric | kind | attributes | what it says |
|---|---|---|---|
| `lilguys.frames` | counter | — | frames submitted |
| `lilguys.frame.render` | histogram, ms | — | GPU submit, per frame |
| `lilguys.frame.interval` | histogram, ms | — | the gap between ticks, **before** the clamp |
| `lilguys.tick.rate` | gauge, Hz | — | what the loop asked for |
| `lilguys.on_screen` | gauge | — | characters inside the edges |
| `lilguys.observations` | counter | `who`, `kind` | what reached the gate |
| `lilguys.gate` | counter | `who`, `verdict` | what the gate did with it |
| `lilguys.reflexes` | counter | `who`, `emotion` | faces made without a model |
| `lilguys.drift` | counter | `who`, `state` | one per tick, so a rate reads as a time share |
| `lilguys.turns` | counter | `who`, `provider`, `outcome` | model turns asked for |
| `lilguys.turn.duration` | histogram, ms | `who`, `provider`, `outcome` | provider round trip |
| `lilguys.tokens` | counter | `who`, `provider` | window sent, summed over turns |
| `lilguys.compactions` | counter | `who` | how often the window was folded |
| `lilguys.intents` | counter | `who`, `kind` | capabilities actually used |
| `lilguys.refusals` | counter | `who`, `reason` | every silence chosen on purpose |
| `lilguys.speech` | counter | `who` | said aloud, past the shared floor |
| `lilguys.drops` | counter | `where` | anything a full buffer threw away |

Refusals are the ones worth watching. "It did nothing for an hour" has to be answerable, and every
reason — `over budget`, `user away`, `speech floor`, `speech vetted`, `call rejected` — is already
a decision the code makes and would otherwise discard.

`lilguys.frame.interval` records the raw gap rather than the clamped one, because a clamped gap
cannot show a rate that has quietly halved. That is the instrument for the strobe class of bug:
p99 climbing while p50 holds means some frames arrive late, not that every frame is slow.

## The development stack

One container holds Grafana, Prometheus, Tempo and Loki, with OTLP receivers.

```bash
cd packaging/observability && docker compose up -d
```

Then name the collector in `~/.config/lilguys/lilguys.toml` and restart:

```toml
[telemetry]
otlp = "http://127.0.0.1:4318"
```

```bash
lilguy restart
```

Grafana is on <http://127.0.0.1:3000>, no login, dark by default. The **lilguys** dashboard is
provisioned from `packaging/observability/dashboards/lilguys.json` and covers frames, mind and
attention. Model turns also arrive as spans in Tempo, so a slow provider is visible beside what
the turn produced.

Everything binds to the loopback. Stop it with `docker compose down`; the daemon carries on
counting and writing `state.json` with nobody listening, and says once that the collector is
unreachable rather than filling the journal.

`push = "0s"` counts without exporting. `snapshot = "0s"` never writes the file. `enabled = false`
turns the whole meter off, and every call site becomes a no-op.

## Rules

- **Never sent anywhere.** A local file and a local command, plus a collector you started.
- **Attribution is mandatory.** A spend without a `who` is a bug.
- **Refusals are counted, not just logged.**
- **Cheap enough to always be on.** Counters and one background thread; the push is a JSON POST
  over the same `ureq` the provider client uses, and pulls in no async runtime.
- **Telemetry never blocks a tick.** Every call is fire-and-forget, the export runs on its own
  thread, and a dead collector costs the buddy its numbers rather than its life.

## Related

- [architecture.md](architecture.md) — the three clocks these counters measure
- [todo/taxonomy-and-telemetry.md](todo/taxonomy-and-telemetry.md) — the closed sets these
  attributes come from, and what is still open
