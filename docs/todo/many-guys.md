# A cast, not a mascot

**Status:** the roster is built; the social layer is a first cut. This page is what is still
missing and what it implies.

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
```

## What is shared and what is not

| shared | per guy |
|---|---|
| the layer surface and the renderer | body, pose, avatar, palette |
| the sensors: wayland, MPRIS, the inbox | attention gate, novelty and dwell state |
| the compositor, the pointer, window geometry | quantiser, context window, token budget |
| the prompt layers except `persona` | the persona layer, and the model behind it |

The gate being per-guy is the interesting one. Two of them watching the same desktop rule on it
separately, so one may be mid-dwell on something the other discarded ten minutes ago. They diverge
without anything being written to make them diverge.

## They see each other

When one enacts an intent, the others receive it as an observation:

```
- SpongeBob said something — "I'm ready!"
- lil looks amused
```

It arrives on the same stream as a window focus and a hunger pang, because
[philosophy.md](../philosophy.md) says there is one stream. Nobody witnesses their own action —
they already know they did it.

A face is only witnessed above intensity 0.5. A flicker is nobody else's business, and reporting
every micro-expression would drown two guys in each other.

## What is missing

1. **Per-guy voices.** The `voice` field exists on a character and a guy and is not yet read. Two
   characters sharing one voice is the single most obvious thing wrong right now. It wants a voice
   *engine* choice per guy, not just a model, since a character may want espeak's flatness.
2. **They cannot address each other.** `speak` goes to the room. There is no way for SpongeBob to
   say something *to* lil, and no way for lil to know he was addressed. Probably a `to` field on
   `speak`, resolved by name, producing a differently-worded observation for the addressee.
3. **They do not know who else is here.** Nothing in the prompt says another creature exists. The
   `awareness` layer should name the others, or the first time one appears in the stream is
   genuinely confusing.
4. **No spatial awareness of each other.** They can overlap and drift through one another. Knowing
   another guy's position would let `focus` take a name, and would let a reflex fire on proximity —
   which is the cheapest possible source of social behaviour.
5. **Turn budgets are per guy, so cost is linear in cast size.** Five guys is five times the
   tokens. That is fine on a local model and expensive on a hosted one, and the config gives no way
   to say "the whole cast may spend N per hour".
6. **Conversation feedback loops.** Two guys who each react to the other reacting will ping-pong.
   The novelty window damps it, but nothing prevents it. A cheap guard: an observation about
   another guy cannot itself be witnessed by a third, so chatter does not amplify across a cast.

## Worth being careful about

**The pace was tuned for one.** A forty-five second quantum with one guy means someone speaks
rarely. With six guys on the same quantum, something happens every seven seconds on average, and
the restraint that makes a single creature good company becomes a room that will not shut up.

The cast should probably share a *floor* on silence rather than each keeping its own — a global
minimum gap between anybody speaking, independent of how many are thinking. That is a small change
and it should land before anyone tries the full Bikini Bottom.

## Why this is worth doing

A single creature reacting to your desktop is a toy with a ceiling. Two creatures reacting to your
desktop *and to each other* is a situation, and situations do not have that ceiling. The cost is
one more `[[guys]]` block and a second context window, which on a local model is nothing at all.
