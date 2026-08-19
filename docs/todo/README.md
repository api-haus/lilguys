# Planned work

Nothing here is built. Each page is a design settled far enough to act on, with its open questions
named rather than hidden. Read [philosophy.md](../philosophy.md) first — it decides the ambiguous
cases in all of them.

## Order

The dependencies are real, and roughly this:

```mermaid
flowchart TD
    SPLIT["soul / hologram split"] --> LUAU["luau scripting"]
    SPLIT --> AGENT["agent integration"]
    SPLIT --> THREE["three-process split"]
    LUAU --> PKG["character packages"]
    TALK["talking to him"] --> AGENT
    CLI["lilguy CLI"] --> PKG
    TALK -.->|"`lilguy say` unblocks it"| CLI
    OBJ["object persistence"] -.->|"answer once, for both"| PKG
```

## Pages

| page | what | when |
|---|---|---|
| [soul-hologram-split](soul-hologram-split.md) | two processes, so the mind restarts without the character blinking out | first; most things wait on it |
| [talking-to-him](talking-to-him.md) | click him and type; the one thing allowed to jump the pace | anytime, mostly independent |
| [lilguy-cli](lilguy-cli.md) | setup, doctor, provider, voice, persona — built for an agent to drive | anytime; `doctor` is nearly free |
| [luau-scripting](luau-scripting.md) | sandboxed drives that compose through the event stream | after the split |
| [character-packages](character-packages.md) | look, voice, drives, entities and interaction, shipped as a directory | after scripting |
| [agent-integration](agent-integration.md) | be the face of hermes or openclaw; the original reason for all this | after the split and the message path |
| [object-persistence](object-persistence.md) | whether persistent things need representing at all | open question; may be answered by doing nothing |
| [three-process-split](three-process-split.md) | pull the mind out of the soul | trigger-gated; not soon |

## The through-line

Every one of these is the same move: **add a way for something to happen, and a way for him to find
out that it happened.** A capability without its event is a puppet string. That is philosophy rule 5,
and it is the thing to check any of these designs against when they start to sprawl.
