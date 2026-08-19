# Plugins: casts, entities, and interaction

**Status:** not started. **Depends on:** [luau-scripting.md](luau-scripting.md),
[soul-hologram-split.md](soul-hologram-split.md).

A plugin is never just a persona. The smallest one is a single character — a look, a voice, drives,
things it carries — and the largest is an entire cast with the world scripts that run them. One
install unit, one identity for telemetry and budgets, one thing to enable or disable. `persona` is
the wrong word for it and is not used anywhere.

This is the format, and the interaction model underneath it.

## The package

```
moth/
  plugin.toml          identity, version, required taxonomy, tiers wanted
  characters/
    moth.toml          persona, palette, size, voice — one file per character
  prompt/
    self.md            optional: override the self-model layer for this cast
  drives/
    phototaxis.lua     drifts toward the brightest window
    hunger.lua
  world/
    night.lua          optional: a director, for a plugin that carries a situation
  avatar/
    moth.model3.json   Live2D, or moth.vrm, or sprites/
  entities/
    lamp.lua           a thing that exists alongside them
    lamp.png
```

The same layout scales from one creature to a cast of seven: more files under `characters/`, and
`world/` starts earning its place. Nothing about the format changes.

Installed by `lilguy plugin install gh:someone/moth`. Everything about a character lives here;
nothing about a character is code in this repository.

**The rules prompt layer is not part of a plugin.** Plugins supply `persona` per character, and may
supply `self` for the cast; `rules` stays with the daemon and is not overridable. See the safety section in
[lilguy-cli.md](lilguy-cli.md) — this is the one structural defence against a hostile persona.

## Entities

An entity is a thing that is not him: a lamp, a ball, a plant, a second creature. It has a sprite
or a model, a position, and a Luau script.

The reason to have them at all is that they turn interaction into something with *content*. A
person can click him and get a face; a person can drag a ball at him and get a reaction to the
ball. The second is a story and the first is a widget.

Entities are declared by the package and owned by the hologram, since they are drawn and dragged and
must keep working with no soul attached.

## Interaction primitives

The engine supplies the interactions; the package decides what they mean.

| primitive | what it is |
|---|---|
| click | a tap on him or an entity |
| drag and drop | pick a thing up, put it somewhere, or on him |
| context menu | right-click, entries declared by the package |
| hover | a pointer resting on something |
| proximity | two entities near each other, or an entity near him |

**Every one of these produces an interoceptive event, without exception.** Dropping a ball on him
emits `you feel bumped (entity: ball)`; opening his context menu emits `you found yourself watched`;
a script's response to any of it emits its own record. This is not a nicety — philosophy rule 5 says
a body action taken without the mind must produce a line saying it happened, and interaction is
exactly that.

The result is that a person playing with him is *legible* to the mind, minutes later, as a
sequence of things that happened to a body — which is the only way the mind can ever comment on
play without being wired into the pointer.

## What a script may do with an entity

The same restraint as drives: the engine supplies verbs, packages compose them.

| script calls | effect |
|---|---|
| `ctx.spawn{…}` / `ctx.despawn(id)` | an entity exists, or stops existing |
| `ctx.move(id, to)` | it goes somewhere, with the engine's easing |
| `ctx.attach(id, "hand")` | he carries it |
| `ctx.sprite(id, "lit")` | it changes appearance |
| `ctx.menu(id, entries)` | what right-clicking it offers |
| `ctx.feel{…}` | as ever, an interoceptive line |
| `on_interact(ctx, ev)` | click, drag, drop, hover, proximity |

**A script may not move him.** Locomotion answers to the body and the mind, not to a package. A
script that wants him somewhere emits a feeling and lets him decide — which is the same rule as
"intentions are requests, not commands", applied one level down.

## Reflexive instrumentation at entity level

The interesting case the engine should support: **a package declaring what he does automatically
around a thing.** Looks at the lamp when it lights. Drifts from a ball that moves. Flinches when
something is dropped on him.

These are reflexes, so they obey reflex law: immediate, free, never rationed, and each one records
itself. A package declares them, the hologram runs them with no soul attached, and the mind reads
about them afterwards like everything else.

```lua
-- entities/lamp.lua
function lamp.reflexes()
  return {
    { when = "sprite_changed", to = "lit", react = { tone = "curious", intensity = 0.5 } },
    { when = "dropped_on_buddy",        react = { tone = "surprised", intensity = 0.9 } },
  }
end
```

Declarative, so it can be checked and rate-limited rather than being arbitrary code on the render
path.

## The event vocabulary is the contract

Every interaction and every script verb must map to an observation the mind already understands, or
to a new one added deliberately. If a package can cause something to happen that produces no event,
the mind is blind to it and the character is a puppet again.

That constraint is worth stating as the acceptance test for this whole feature: **play with him for
five minutes, then read `events.jsonl` — the session should be legible from the log alone.**

## Work

Ordered so each step is usable before the next exists.

1. Package layout, `lilguy plugin install`, prompt layers from a package. No entities yet.
2. Avatar loading from a package (sprites first, then Live2D and VRM adapters).
3. Drives from a package, over the Luau host.
4. Entities: spawn, draw, drag, drop, and their observations.
5. Context menus and hover.
6. Declarative entity reflexes.

## Open questions

- Can two plugins be enabled at once — a cast plus a separate drive pack? Composable is nicer and
  much harder to make safe. The roster already supports guys from different sources, so the answer
  is probably yes with a namespacing rule.
- Do entities persist across restarts? A ball he was carrying should probably still be there. That
  needs entity state in the same persistence as script state.
- Should entities be sensed as *objects* rather than events?
  [object-persistence.md](object-persistence.md) is the same question and should be answered once,
  for both.
