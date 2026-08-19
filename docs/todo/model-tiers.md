# Model tiers

**Status:** not started. **Why:** a plugin cannot know what the person can afford, and must work
anyway.

Today code names a provider: `provider = "ollama"`. That is fine while the only caller is the
reactor and the only author is us. The moment a plugin ships with seven characters and an episode
generator, naming a provider is wrong — the author has no idea whether this machine has a 4B on
localhost or a Claude subscription.

**Tiers name what a call is worth, not what will answer it.** The person maps tiers to providers
once; every plugin is portable across every setup.

## The taxonomy

Four, fixed by the engine so a plugin can rely on them. A plugin may not invent a fifth, because a
name nobody has configured is a name that does nothing.

| tier | what it is for | expected rate |
|---|---|---|
| `reflex` | may run constantly; every guy, every quantum | hundreds per hour |
| `mind` | ordinary character thought — the default | tens per hour |
| `might` | rare and worth being slow: an episode arc, a summary that will be reused all day | a handful per hour |
| `bulk` | offline work nobody is waiting on: compaction, digests, backfill | whenever |

The distinction that matters is **who is waiting**. `reflex` and `mind` are in front of a person
watching a character; `might` is in front of a person who will notice it was good; `bulk` is in
front of nobody.

```toml
[tiers.reflex]
provider = "ollama-small"
per_hour = 400

[tiers.mind]
provider = "ollama"
per_hour = 60

# Optional. A plugin that wants this must work without it.
[tiers.might]
provider = "anthropic"
per_hour = 6
per_day = 40

[tiers.bulk]
provider = "ollama"
per_hour = 20
```

An unconfigured tier **falls back down**, never up: `might` unset resolves to `mind`, `mind` unset
resolves to `reflex`. Nothing ever silently escalates onto a paid endpoint. A plugin that needs to
know whether it really got what it asked for can ask.

## Hard limits, enforced by the engine

Budgets are the engine's job, not the plugin's, for the same reason the gate is not the model's
job: the thing being protected is the person's money and attention, and the thing spending it
cannot be the thing policing it.

- **`per_hour` and `per_day` ceilings per tier**, refilling continuously, as the existing turn
  budget already does.
- **A per-plugin share** of each tier, so one greedy episode generator cannot starve the
  characters' own thinking.
- **Refusal is a value, not an error.** `llm.ask("might", …)` returns nil and a reason. A plugin
  that cannot handle nil is a broken plugin, and the docs should say so in the first paragraph.
- **Every refusal is logged**, so "why did nothing happen" has an answer in `events.jsonl`.
- **A tier that is spent degrades**, it does not queue. Waiting twenty minutes to say something
  clever is worse than saying something ordinary now.

## What this changes today

The reactor stops naming `mind.provider` and starts asking for the `mind` tier. A guy that wants a
different model keeps its override, which becomes a tier override rather than a provider one. The
existing `turns_per_hour` becomes the `mind` tier's `per_hour`, which is what it always was.

That is a small change and worth making before any plugin exists, because it settles the vocabulary
plugins will be written against.

## Open questions

- Should a tier be able to name *several* providers with fallback on failure? Useful when a local
  endpoint is flaky. Adds a retry policy to a config file, which is where retry policies go to die.
- Is `bulk` real, or is it `mind` with a low priority? It differs only in that nobody is waiting,
  which may not deserve its own name.
- Per-plugin shares need a plugin identity. That arrives with
  [character-packages.md](character-packages.md) and probably should not be designed before it.
