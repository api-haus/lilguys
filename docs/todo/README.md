# Planned work

**Sequenced in [../roadmap.md](../roadmap.md)** — phases by what you can demonstrate at the end of
each, with sizes. This page is the index of the designs those phases implement.

Each page is a design settled far enough to act on, with its open questions named rather than
hidden, and each says at the top how much of it now exists. Read [philosophy.md](../philosophy.md)
first — it decides the ambiguous cases in all of them.

## Order

The dependencies are real, and roughly this:

```mermaid
flowchart TD
    SPLIT["soul / hologram split"] --> LUAU["luau scripting"]
    SPLIT --> AGENT["agent integration"]
    SPLIT --> THREE["three-process split"]
    SPLIT --> WORLD
    LUAU --> PKG["character packages"]
    LUAU --> WORLD["world scripting"]
    TIERS["model tiers"] --> WORLD
    PKG --> WORLD
    MANY["many guys"] --> WORLD
    TALK["talking to him"] --> AGENT
    CLI["lilguy CLI"] --> PKG
    TALK -.->|"`lilguy say` unblocks it"| CLI
    OBJ["object persistence"] -.->|"answer once, for both"| PKG
```

## Pages

| page | what | when |
|---|---|---|
| [soul-hologram-split](soul-hologram-split.md) | two processes, so the mind restarts without the character blinking out | first; most things wait on it |
| [talking-to-him](talking-to-him.md) | click him and type; the one thing allowed to jump the pace | the message path is built; the box is not |
| [lilguy-cli](lilguy-cli.md) | setup, doctor, provider, voice, plugin — built for an agent to drive | `doctor`, `start`, `stop`, `status`, `say` are built |
| [luau-scripting](luau-scripting.md) | sandboxed drives that compose through the event stream | after the split |
| [character-packages](character-packages.md) | look, voice, drives, entities and interaction, shipped as a directory | after scripting |
| [agent-integration](agent-integration.md) | be the face of hermes or openclaw; the original reason for all this | after the split and the message path |
| [object-persistence](object-persistence.md) | whether persistent things need representing at all | open question; may be answered by doing nothing |
| [taxonomy-and-telemetry](taxonomy-and-telemetry.md) | the vocabulary as a versioned contract, and cost you can attribute | with tiers; before any plugin exists |
| [model-tiers](model-tiers.md) | name what a call is worth, not what answers it; engine-enforced budgets | soon — it settles the vocabulary plugins are written against |
| [world-scripting](world-scripting.md) | a thin core; situations, arcs and a cast that comes and goes, authored in Luau | after tiers and the Luau host |
| [many-guys](many-guys.md) | a cast rather than a mascot; they sound different and answer by name | built, except knowing where each other are |
| [three-process-split](three-process-split.md) | pull the mind out of the soul | trigger-gated; not soon |

## The through-line

Every one of these is the same move: **add a way for something to happen, and a way for him to find
out that it happened.** A capability without its event is a puppet string. That is philosophy rule 5,
and it is the thing to check any of these designs against when they start to sprawl.

Which is also the argument for a thin core. Drives, entities, locations, weather, plot, and who is
even on screen this evening — all of them are that one shape. Built separately in Rust they are seven subsystems that do not compose; built as
one injection point in the core with everything above it in script, they are one subsystem that
composes with itself. See [world-scripting](world-scripting.md).
