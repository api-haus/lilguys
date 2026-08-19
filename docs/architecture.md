# lilguys — architecture

One daemon, `lilguysd`. It draws a character on a Wayland overlay, senses what happens on the
desktop, and lets a language model decide what the character does about it.

Everything below describes what the code does today. **[philosophy.md](philosophy.md) says what it
is held to** — the mind/body split, the two kinds of sensing, and the six things a change may not
break. Measurements behind these choices are in [design-space.md](design-space.md); the manual
checks are in [qa-graybox.md](qa-graybox.md).

## 1. The shape of it

```mermaid
flowchart LR
    subgraph world["the desktop"]
        WL["Wayland<br/>compositor"]
        DB["D-Bus<br/>session bus"]
        IPC["Hyprland<br/>IPC socket"]
    end

    subgraph daemon["lilguysd"]
        SEN["sensors<br/><i>push, free</i>"]
        GATE["attention gate<br/><i>local, no model</i>"]
        QUANT["quantiser<br/><i>time slices</i>"]
        MIND["reactor<br/><i>one model turn</i>"]
        BODY["body<br/><i>steering</i>"]
        AV["avatar<br/><i>the puppet</i>"]
        VOX["voicebox"]
    end

    WL --> SEN
    DB --> SEN
    IDLE["idle regiment<br/><i>needs no events</i>"]
    SEN --> GATE
    GATE -->|survivors| QUANT
    GATE -.->|reflex arc: face only| BODY
    IDLE --> BODY
    QUANT -->|one slice| MIND
    MIND -->|intents| BODY
    MIND -->|speak| VOX
    BODY --> AV
    IPC -->|pointer, window rects| BODY
    AV -->|quads| WL
    VOX -->|audio| world
```

Two properties shape every decision:

- **Cheap things happen often, expensive things happen rarely.** The sensors are push-based and
  cost nothing. The gate is local arithmetic. Only the reactor spends tokens, and it is fed on a
  slow clock with a hard ceiling.
- **The mind never touches pixels or protocols.** It emits four kinds of intent. Everything about
  how those look is below the `Avatar` trait, and everything about where the character goes is in
  the body.

## 2. Three clocks

Nothing in lilguys runs on one loop. Three run at very different rates, and each only escalates to
the next when it has to.

| clock | rate | cost | what it does |
|---|---|---|---|
| reflex | 60 Hz awake, 8 Hz idle | none | drift, gaze, blink, breathing, click, drag |
| notice | per event | none | novelty and dwell filtering, the reflex arc |
| think | one slice, at most | tokens | the model decides what to actually do |

The reflex clock is the render tick. It drops to 8 Hz whenever the character is settled and nothing
is animating, which is most of the time.

Two distinct things run without a model, and they are easy to confuse:

- **The idle regiment** needs no events at all. Breathing, blinking on an interval deliberately
  irrational against the breath cycle, the figure-of-eight float, gaze tracking with eyes leading
  head leading body, turning to face a pointer that stays behind him, and `wander_per_minute`. All
  of it lives in `locomotion.rs` and runs on every tick. This is what keeps him alive when nothing
  whatsoever is happening.
- **The reflex arc** is event-driven: the gate hands an observation straight to the body, skipping
  the model entirely. See §4.1.

## 3. What "quantised event flow" means

**Fixed-length time slices.** Events do not stream to the model. They accumulate in a bucket for a
whole quantum — 45 seconds by default, 10 minutes once the user is away — and the bucket is then
rendered as one short report. One slice is at most one model turn.

Three rules make it economical:

1. **An empty slice is never sent.** If nothing survived the gate during the window, no request is
   made at all. Silence costs nothing and is the common case.
2. **The window widens when the user leaves.** `idle_quantum` replaces `quantum` the moment the
   idle notifier fires, so an unattended machine costs almost nothing.
3. **A slice can still be refused.** `turns_per_hour` is a refilling budget on top of the clock; a
   busy hour cannot spend more than its allowance no matter how many slices fill up.

```mermaid
sequenceDiagram
    autonumber
    participant S as sensors
    participant G as gate
    participant Q as quantiser
    participant M as reactor
    participant B as body

    S->>G: focus zen, a video essay
    G->>G: unseen, start 90s dwell
    S->>G: focus ghostty
    G->>G: seen 20s ago, discard
    S->>G: playing Like Stories of Old
    G->>G: unseen, start 60s dwell

    Note over G: dwell elapses, both survive
    G->>Q: append 2 lines
    G->>B: reflex arc, curious
    Note over Q: quantum still open, nothing sent

    Note over Q,M: 45s boundary, bucket non-empty
    Q->>M: one slice plus framing state
    M->>M: compact if over context_tokens
    M->>M: one chat completion, four tools offered
    M-->>B: react curious 0.6, then focus window zen
    Note over M,B: no speak, silence is the default
```

Every slice opens with two framing lines carrying **state rather than events** — how long the
window was, whether the user is present, the active workspace, the focused window, and how the body
currently is:

```
[45s elapsed · they are here · workspace 2 · they are looking at zen — a video essay]
[you are settled by zen, feeling curious 60%]
```

The second line is why interoception needs more than an event stream: a feeling that never changes
is still true, and only a framing line can say so. It also lets the model tell "nothing happened
because they left" from "nothing happened because they are concentrating".

## 4. The attention gate

The gate is the reason the reactor is affordable. It runs locally, spends nothing, and discards
most of what it sees.

```mermaid
stateDiagram-v2
    [*] --> Judge: observation arrives
    Judge --> Ignored: key seen inside novelty_window
    Judge --> Emote: ambient (workspace, presence)
    Judge --> Pending: could matter (focus, title, media)
    Pending --> Pending: dwell not elapsed
    Pending --> Think: dwell elapsed, budget allows
    Pending --> Emote: dwell elapsed, budget spent
    Ignored --> [*]
    Emote --> [*]: face only, never a token
    Think --> [*]: appended to the current slice
```

Every observation carries a **key** — `focus:zen:<title>`, `media:<url>:<title>` — and a repeat of
the same key inside `novelty_window` is dropped outright. Alt-tabbing between two windows twenty
times produces two observations, not forty.

### 4.1 The reflex arc

Two paths reach `Verdict::Emote`, and both bypass the model:

1. **A feeling** — the body's own, or a drive's through the inbox. Always answered, never rationed:
   budget protects tokens, and an expression costs none.
2. **Ambient observations** — a workspace switch or a presence change. These colour the mood but are
   never worth a token on their own. They can flood, so unlike feelings they are rationed.
3. **A demoted thought** — a dwell matured, but `turns_per_hour` was already spent. Rather than
   discard it, the verdict drops from Think to Emote so something visible still happens.

The answer is a fixed mapping from observation to expression, applied directly to the body:

| observation | expression | hold |
|---|---|---|
| user came back | pleased 0.55 | 6 s |
| user went away | sleepy 0.70 | 30 s |
| workspace switched | curious 0.30 | 3 s |
| focus changed | curious 0.45 | 5 s |
| window retitled | curious 0.22 | 2.5 s |
| playback started | amused 0.45 | 8 s |
| playback stopped | neutral 0.20 | 3 s |
| any feeling | whatever it named | whatever it asked |

**Facial only, on purpose.** A gesture is a deliberate intention and stays the mind's to decide, so
the worst this layer can do with the model unreachable is pull a face. It never moves him, never
speaks, and never spends anything.

Dwell is what stops the buddy reacting to things you passed through. A window must hold focus for
`focus_dwell`; a track must play for `media_dwell`. Skipping a video means its transcript is never
worth fetching, and the gate is where that is decided.

## 5. Senses

Each sensor is one source, and each is independently switchable in `[senses]`. None of them poll,
and none of them look at pixels.

Sensing divides in two, and the division is load-bearing rather than tidy — see
[philosophy.md](philosophy.md). **Exteroception** is the world; **interoception** is the body
reporting on itself. Both push onto the same bus and reach the mind in the same account.

| sensor | kind | source | gives |
|---|---|---|---|
| windows | extero | `zwlr_foreign_toplevel_manager_v1` | focus changes, app ids, live titles |
| workspaces | extero | `ext_workspace_manager_v1` | which workspace is active, switch events |
| idle | extero | `ext_idle_notifier_v1` | user went away, user came back |
| media | extero | MPRIS on D-Bus | title, artist, and **the URL** of whatever is playing |
| pointer | extero | Hyprland IPC | cursor position, 14.6 µs per read |
| geometry | extero | Hyprland IPC | window rectangles, for `focus(window)` |
| body | intero | `locomotion.rs` | picked up, set down, arrived, out of sight |
| inbox | intero | `$XDG_RUNTIME_DIR/lilguys.sock` | any feeling any process cares to push |

### 5.1 The inbox

One JSON object per line, from anything that can open a unix socket:

```json
{"source":"hunger","state":"starving","detail":"nothing since this morning",
 "tone":"concerned","intensity":0.9,"hold":40}
```

`tone`, `intensity` and `hold` set the immediate expression, because only the drive's author knows
what its own signal means. A feeling is answered by the body at once and **never rationed** — it
costs no tokens — and it still rides into the next slice for the mind to reflect on later.

A feeding mechanic is therefore a script with a timer, outside this repo entirely.

The Wayland sensors ride the same connection as the surface — no second socket, no second thread.
MPRIS gets its own thread because D-Bus is blocking and a stalled bus must never stall rendering.

Media is the sense that matters most and costs least. A `PropertiesChanged` signal carries
`xesam:url`, so a YouTube watch id arrives as a free push event and a transcript is one HTTP call
away. No screen capture, no OCR, no browser extension.

## 6. The reactor

Its own thread, blocking HTTP, one turn per slice.

```mermaid
flowchart TD
    Q["Quantum arrives"] --> P{"user present<br/>or think_while_away?"}
    P -->|no| DROP["drop, no request"]
    P -->|yes| BUD{"turns_per_hour<br/>budget?"}
    BUD -->|spent| DROP2["drop, note the reason"]
    BUD -->|allows| CMP{"over<br/>context_tokens?"}
    CMP -->|yes| SUM["summarise everything<br/>older than keep_recent<br/>into one system message"]
    CMP -->|no| CHAT
    SUM --> CHAT["chat completion<br/>4 tools offered"]
    CHAT --> TC{"tool_calls<br/>returned?"}
    TC -->|yes| INT["parse to intents"]
    TC -->|no, but text| ERR["report: model cannot<br/>do native tool calls"]
    TC -->|no, silent| NONE["do nothing — valid<br/>and common"]
    INT --> HIST["append reply plus one<br/>tool result per call"]
```

**Context is a plain message list that compacts in place.** Above `context_tokens`, everything
older than `keep_recent` turns is sent back to the same model with a summarise instruction, and the
result replaces it as a single system message. If summarising itself fails, the old turns are
dropped anyway — the window has to shrink either way.

**Tool calls must be native.** If a model writes `react(pleased)` into the message text instead of
using the tool channel, lilguys reports that as an error rather than parsing around it.
`lilguysd --check` answers this before you ever run the daemon.

## 7. The four capabilities

The model has exactly four tools, ordered from cheap and quiet to loud and interrupting. The system
prompt says to prefer them in that order, and that calling nothing at all is a normal response.

```mermaid
flowchart LR
    R["react<br/><i>face only</i>"] --> G["gesture<br/><i>whole body</i>"]
    G --> F["focus<br/><i>go somewhere</i>"]
    F --> S["speak<br/><i>interrupts</i>"]
    R -.-> RD["emotion + intensity + hold<br/>eases onto Pose params"]
    G -.-> GD["one of eight, each with<br/>a duration and an envelope"]
    F -.-> FD["window · pointer · place · away<br/>becomes a steering target"]
    S -.-> SD["one short sentence<br/>into the voicebox queue"]
```

`focus` is the only one that needs the outside world: `focus(window, match: "zen")` looks the
rectangle up through Hyprland IPC, because no Wayland protocol will tell one client where another
client's window is. `focus(away)` sends the character off the edge of the screen on purpose.

## 8. Body and avatar

The body is steering. There is no gravity, no ground, and no jump — a lilguy floats, drifts, and
may leave the screen.

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Noticing: pointer within notice_radius
    Noticing --> Idle: pointer beyond forget_radius
    Idle --> Seeking: wander, or focus() from the mind
    Noticing --> Seeking: focus() from the mind
    Seeking --> Lingering: arrived
    Lingering --> Idle: linger elapsed
    Idle --> Leaving: focus(away)
    Leaving --> Idle: offscreen and nothing to do
    Idle --> Held: clicked and dragged
    Held --> Idle: released
```

Movement is seek-with-arrival plus a slow figure-of-eight float, so he is never perfectly still.
`offscreen_margin` decides how far past the edge he may drift; only `Leaving` ignores it.

The seam below the body is one trait:

```mermaid
classDiagram
    class Avatar {
        <<trait>>
        +advance(pose, drive, dt)
        +draw(painter, origin, facing, scale)
        +bounds() Bounds
        +hit(local) bool
    }
    class Pose {
        +f32 head_yaw
        +f32 gaze_x
        +f32 eye_open_l
        +f32 mouth_open
        +f32 breath
        +f32 joy
        20 parameters in total
    }
    class Drive {
        +f32 speed
        +heading
        +f32 bob_phase
        +gesture kind and progress
        +bool speaking
    }
    Avatar <|.. Graybox
    Avatar <|.. Live2D : planned
    Avatar <|.. VRM : planned
    Avatar ..> Pose
    Avatar ..> Drive
```

`Pose` names follow Live2D's standard parameter set, which makes the whole VTuber model corpus an
asset library and maps cleanly onto glTF morph targets. `Drive` carries what is not a facial
parameter: travel, gesture progress, and whether the mouth should be moving.

`hit()` is load-bearing beyond drawing — it produces the input region, and everything outside that
rectangle passes clicks through to whatever is underneath.

## 9. Voice

A TTS pipeline is a command line. That is the whole abstraction.

```toml
[voice.engines.piper]
synth = ["piper", "--model", "{voice}", "--length-scale", "{speed}", "--output_file", "{out}"]
play  = ["pw-play", "{out}"]
```

`{text}` `{voice}` `{speed}` `{out}` are substituted; text goes on stdin when `{text}` does not
appear in the arguments. Piper, espeak-ng, kokoro, XTTS and any OpenAI-compatible `/v1/audio/speech`
endpoint reached through `curl` are therefore the same thing to lilguys, and adding one is a config
entry rather than a code change.

Synthesis runs on its own thread with a short queue. Beyond `queue_limit` an utterance is refused
rather than queued — a buddy talking over itself is worse than one that missed a line. While it
speaks, `Drive::speaking` moves the mouth and blocks the expression layer from fighting it.

## 10. Threads

```mermaid
flowchart TD
    MAIN["main thread — calloop<br/>wayland events, render tick,<br/>sensors, gate, quantiser, body"]
    MPRIS["mpris thread<br/>blocking D-Bus"]
    MIND["mind thread<br/>blocking HTTP"]
    VOICE["voice thread<br/>synth and playback"]
    MPRIS -->|Observation| MAIN
    MIND -->|Reaction| MAIN
    VOICE -->|Speaking / Silent| MAIN
    MAIN -->|Quantum| MIND
    MAIN -->|text| VOICE
```

Every thread that can block owns a channel into the calloop event loop and nothing else. The main
thread never waits on D-Bus, HTTP, or a subprocess, which is why a dead endpoint or a missing TTS
binary degrades one sense instead of freezing the character.

## 11. Logs

`$XDG_STATE_HOME/lilguys/` holds two append-only JSONL files, on by default.

- **`turns.jsonl`** — one object per model turn: the slice sent verbatim, the raw `content`, the raw
  `tool_calls`, the intents parsed out, the calls **rejected** and why, token estimate, whether the
  window compacted, the error if any, and the round-trip in milliseconds.
- **`events.jsonl`** — one object per gate ruling, with its verdict, so what was discarded is as
  visible as what survived.

Everything the model says is recorded before it is acted on, which is what makes misbehaviour
diagnosable rather than anecdotal.

### 11.1 Vetting speech

`speak` is the only capability that reaches the user directly, so it is the only one vetted. An
utterance is refused when it runs past 25 words, contains markup characters, or carries the tells
of a leaked prompt. Small models do leak their tool-calling preamble into `speak` — a real one
began "Given the following functions, please respond with a JSON…" and was spoken aloud before this
existed. A refusal is logged with its reason and shown as `[x] refused` rather than swallowed.

## 12. Configuration

One TOML file, `~/.config/lilguys/lilguys.toml`, naming nothing internal. A partial file is merged
onto the bundled defaults, so it need only contain what differs.

- `lilguysd --print-config` writes a fully commented starting point.
- `lilguysd --check` validates it, resolves the provider, probes the model for **native tool-call
  support**, and reports whether the TTS binary is on `PATH` — all without opening a surface.

Providers are interchangeable because every one speaks the OpenAI chat-completions wire format:
llama.cpp's server, ollama, vLLM, LM Studio, OpenRouter, OpenAI. Switching between local and hosted
is a `provider = ` line. API keys are named by environment variable and never live in the file.

## 13. Not yet built

- **Memory.** Deferred by decision. The reactor's private notes are the seed for it.
- **hermes link.** Clicking the character opens a channel adapter; ambient observations become a
  plugin writing to memory rather than into a live conversation.
- **Live2D and VRM adapters**, over the same `Pose` and `Drive`.
- **Clipboard and AT-SPI senses**, both free and both push-based.
- **Per-window capture** — the only planned sense that costs anything.
