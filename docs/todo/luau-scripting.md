# Sandboxed Luau scripting

**Status:** not started. **Why:** a feeding mechanic should be a file someone drops in, not a fork.

The inbox socket already lets any process give a lilguy a feeling, and that stays the floor: an
external drive needs nothing from this document. What it does not give you is a *drive that lives
with the character* — starts with him, stops with him, keeps state across a restart, and can react
to what he does rather than only to a clock.

## Why Luau rather than Lua

Luau is Roblox's Lua 5.1 fork, and its sandbox is the entire reason to prefer it: it was built for
running untrusted user scripts in a hostile setting, which is exactly the position a downloaded
personality script puts us in. It also brings interrupts — a script that spins forever can be
killed on a deadline instead of hanging the process.

The Rust binding is `mlua` with the `luau` feature, which is well-trodden and does not need a
system Lua.

Plain Lua would work and is smaller, but sandboxing it correctly means removing pieces of the
standard library by hand and getting every one right. That is a security posture nobody should
hand-roll for a thing that runs on someone's desktop all day.

## What a script may touch

Everything a script can do maps onto an interface that already exists. **No new capability is
invented for scripts** — if a script can do it, the mind can already ask for it or a drive can
already push it.

```lua
-- drives/hunger.lua
local hunger = {}

function hunger.init(ctx)
  ctx.state.fullness = ctx.state.fullness or 1.0   -- persisted across restarts
end

function hunger.tick(ctx, dt)
  ctx.state.fullness = ctx.state.fullness - dt / 21600   -- empty in six hours
  if ctx.state.fullness < 0.15 then
    ctx.feel{ state = "starving", detail = "nothing since this morning",
              tone = "concerned", intensity = 0.9, hold = 40 }
  end
end

-- Scripts observe the same stream the mind does. Nothing privileged.
function hunger.on_observation(ctx, obs)
  if obs.kind == "feeling" and obs.source == "pet" and obs.state == "fed" then
    ctx.state.fullness = 1.0
    ctx.feel{ state = "fed", tone = "pleased", intensity = 0.9, hold = 25 }
  end
end

return hunger
```

| script calls | goes to |
|---|---|
| `ctx.feel{…}` | an interoceptive `Feeling`, same as the inbox |
| `ctx.state` | a table persisted to disk per script |
| `ctx.config` | that script's own `[scripts.<name>]` table |
| `ctx.log(msg)` | `events.jsonl` |
| `on_observation(ctx, obs)` | every observation, after the gate |
| `tick(ctx, dt)` | a slow timer, seconds not frames |

**A script may not** open sockets, read files, spawn processes, touch the surface, call the model,
or emit an intent. Those are the daemon's, and a script that needs one of them is asking for a
capability that should be argued for in philosophy.md's terms.

The observation callback is what makes drives composable toward each other: a pet script emits
`fed`, the hunger script sees it in the same stream everything else arrives on, and neither knows
the other exists. That is the whole plugin story and it needs no registry, no dependency
declarations, and no load order.

## Personalities as scripts

The prompt is already layered and any layer can be a file. A personality package is therefore just
a directory:

```
~/.config/lilguys/characters/moth/
  persona.md        # the persona prompt layer
  self.md           # optional: override the self-model layer too
  drives/
    phototaxis.lua  # drifts toward the brightest window
  moth.toml         # layer wiring plus [scripts.phototaxis] config
```

Selecting a character is one line in `lilguys.toml`. Nothing about a character is code in this
repo, which is the point.

## Safety

1. **Sandbox on, always.** `mlua`'s Luau sandbox, no `io`, no `os` beyond `os.time` and
   `os.clock`, no `require` outside the character's own directory.
2. **Interrupt on a deadline.** A tick that overruns is killed and the script is disabled with a
   line in `events.jsonl`. One bad script must not stall the body.
3. **Memory ceiling per script**, enforced by the Luau allocator.
4. **Scripts run in the soul, not the hologram.** They are policy, they can misbehave, and the
   hologram is the process that must never stop. See
   [soul-hologram-split.md](soul-hologram-split.md) — that split should land first.
5. **Rate-limit `ctx.feel`.** A script emitting a feeling every tick would flood the stream and
   drown the desktop senses. A per-script ceiling, with the drops logged.

## Work

Do this after the soul/hologram split, so scripts land on the side that can be restarted.

1. `mlua` with `luau` and `vendored`; one `ScriptHost` owning the VM and the script table.
2. `ctx` implementation: `feel`, `state`, `config`, `log`. Nothing else in the first cut.
3. Persistence: one JSON file per script beside the logs, written on change with a debounce.
4. `tick` on a slow timer, `on_observation` after the gate.
5. Interrupt, memory ceiling, and `feel` rate limit — all three before any script ships.
6. Character directories and the `[character]` config key.
7. Ship `hunger.lua` as the worked example, and cite it in the docs rather than describing it.

## Open questions

- Should a script be able to veto an observation before the gate sees it? Powerful, and a good way
  for one bad script to blind the buddy entirely. Leaning no.
- Should a script be able to *suggest* an intent for the mind to consider, rather than emit one
  directly? That would keep the mind sovereign while letting a drive lobby. Needs a design.
- Hot reload on file change is obvious and cheap; the question is what happens to `ctx.state`
  across a reload. Probably keep it, and let the script version its own schema.
