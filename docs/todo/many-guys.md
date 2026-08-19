# A cast, not a mascot

**Status:** the roster is built, they have their own voices, they can address each other by name,
and one floor holds the whole cast to a minimum silence. What is left is spatial: they still drift
through one another and cannot see where anybody is.

Any number of guys share one surface. Each has its own body, gate, quantiser, context window, voice
queue and — if you want — its own model. Adding one is a `[[guys]]` block. Nothing about the design
privileges the first one, and there is no "switch character" operation because there was never one
character.

```toml
[[guys]]
character = "graybox"

[[guys]]
character = "spongebob"
provider = "haiku"        # this one may think with a different model entirely
voice = "espeak:en-us+f5" # and sound nothing like the other one
```

## What is shared and what is not

| shared | per guy |
|---|---|
| the layer surface and the renderer | body, pose, avatar, palette |
| the sensors: wayland, MPRIS, the inbox | attention gate, novelty and dwell state |
| the compositor, the pointer, window geometry | quantiser, context window, token budget |
| the prompt layers except `persona` | the persona layer, the model, and the voice |

The gate being per-guy is the interesting one. Two of them watching the same desktop rule on it
separately, so one may be mid-dwell on something the other discarded ten minutes ago. They diverge
without anything being written to make them diverge.

## They see each other

When one enacts an intent, the others receive it as an observation:

```
- SpongeBob said to you — "I made Krabby Patties and caught jellyfish today!"
- lil looks amused
```

It arrives on the same stream as a window focus and a hunger pang, because
[philosophy.md](../philosophy.md) says there is one stream. Nobody witnesses their own action —
they already know they did it — and nobody witnesses an action that was held back, either.

A face is only witnessed above intensity 0.5. A flicker is nobody else's business, and reporting
every micro-expression would drown two guys in each other.

## Why they were silent

Worth recording, because none of it was the models' fault and all of it looked like it was.

They *do* witness each other — `lil: you feel SpongeBob made a gesture — wave` was in the log the
whole time. Three things stopped it becoming conversation:

1. **Silence was self-sustaining.** An empty slice was never sent, so nothing produced nothing:
   nobody acted, so nobody had anything to react to, forever. `restless_after` breaks the loop — a
   guy who has been quiet that long gets a slice saying so, and a character with a personality does
   something in character with nothing. **This is what makes idle chatter possible at all.**
2. **Nothing told them anybody else existed.** A line saying another creature did something arrived
   with no idea who that was. The `awareness` layer now names the others and says they are company,
   not scenery.
3. **The prompt drives hard toward silence** — react over gesture over speak, and "calling none at
   all is the most common correct response". Correct for one guy watching a desktop; still probably
   too strong for a cast, and the next thing to tune.

## What is missing

1. **No spatial awareness of each other.** They overlap and drift through one another. Knowing
   another guy's position would let `focus` take a name, and would let a reflex fire on proximity —
   which is the cheapest possible source of social behaviour.
2. **Turn budgets are per guy, so cost is linear in cast size.** Five guys is five times the
   tokens. That is fine on a local model and expensive on a hosted one, and the config gives no way
   to say "the whole cast may spend N per hour". `speech_floor` bounds how often anybody *speaks*;
   nothing yet bounds how often everybody *thinks*.
3. **Conversation feedback loops are damped, not prevented.** Two guys who each react to the other
   reacting will ping-pong. `per_source_cap` is what holds it: a witnessed action carries the actor
   as its source, so one guy monologuing is throttled without silencing the rest. That is a damper
   and not a guard, and a cast of six has not been run against it.
4. **The roster is fixed at startup.** Characters cannot yet arrive or leave while it runs, which
   is what a world needs — see [world-scripting.md](world-scripting.md).

## What landed, and what it took

Worth recording, because none of it was the models' fault and all of it looked like it was.

1. **Silence was self-sustaining.** An empty slice was never sent, so nothing produced nothing:
   nobody acted, so nobody had anything to react to, forever. `restless_after` breaks the loop — a
   guy who has been quiet that long gets a slice saying so, and a character with a personality does
   something in character with nothing.
2. **Nothing told them anybody else existed.** A line saying another creature did something arrived
   with no idea who that was. The `awareness` strand now names the others and says they are company,
   not scenery.
3. **Being addressed was indistinguishable from overhearing.** `speak` now takes an optional `to`,
   and the same sentence reaches the one named as *said to you* and everybody else as *said to lil*.
   The prompt says that being addressed by name is the one thing that usually deserves a voice back
   — and that line carries `{others}`, so it vanishes for a character who is alone.
4. **Two of them shared one voice.** A voice now belongs to a character: `"engine:voice"`, an
   engine, or a voice, resolved guy → character → `[voice]`. SpongeBob is on espeak because alba
   cannot be him.
5. **The pace was tuned for one.** `speech_floor` is a floor under the whole cast rather than one
   guy, so six of them on a forty-five second quantum is not a room that will not shut up. Faces,
   thoughts and movement are free and are never held back by it.

Witnessing is exteroception, incidentally, and modelling it as a feeling was a category error: a
creature acting in front of you is the world happening, not something you feel. It reads as
`SpongeBob said to you — "…"` rather than `you feel SpongeBob said something`.

## Why this is worth doing

A single creature reacting to your desktop is a toy with a ceiling. Two creatures reacting to your
desktop *and to each other* is a situation, and situations do not have that ceiling. The cost is
one more `[[guys]]` block and a second context window, which on a local model is nothing at all.
