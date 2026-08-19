# QA — graybox

Five checks, about three minutes. Run `./target/release/lilguysd` in a terminal you can see; every
check reads either the on-screen HUD or that terminal's stdout.

Stop it with `Ctrl-C`, or `pkill -x lilguysd`.

## 1. Click-through — 20 seconds

Click somewhere on your desktop that is **not** the character. The click must land on whatever is
underneath, exactly as if lilguysd were not running.

Then click the character. The terminal prints `clicked — this is where the hermes channel opens`.

Run with `LILGUYS_DEBUG_INPUT=1` to see the input rectangle printed as it moves. It should track
the character and stay about 125 × 250 px.

**Fails if:** a click away from him is swallowed, or clicking him does nothing.

## 2. Gaze direction across a facing flip — 40 seconds

Move the pointer slowly from the far left of the screen to the far right, passing above him.

- His eyes and the yellow gaze ray follow the pointer the whole way.
- After the pointer sits behind him for about a second, he turns to face it.
- The gaze must **not** invert at the moment he turns.

Watch the `gaze_x` bar in the HUD: it goes yellow (left) on the left half, white (right) on the
right half, and crosses zero once — never twice.

**Fails if:** the pupils or the ray jump to the opposite side when he flips.

## 3. Restraint — 60 seconds

Alt-tab between two windows six or seven times. The terminal shows:

```
[~] focus <app> — <title>     first sight of each window
[·] focus <app> — <title>     every repeat inside the 10-minute novelty window
```

The HUD's `gate` line should show `ignored` climbing much faster than `pending`.

**Fails if:** every alt-tab produces a `~`, or anything produces a `*` this early.

## 4. Media sense — 30 seconds

Start a YouTube video, or change track in any MPRIS player. Within a second or two:

```
[~] playing <artist> — <title>
```

It stays `~` for 60 seconds (the media dwell), then becomes `*` or `!` depending on the thought
budget. That delay is deliberate — a video you skipped past never gets a transcript fetched.

Force it without touching your own playback:

```bash
gdbus emit --session --object-path /org/mpris/MediaPlayer2 \
  --signal org.freedesktop.DBus.Properties.PropertiesChanged \
  "org.mpris.MediaPlayer2.Player" \
  "{'Metadata': <{'xesam:title': <'Test'>, 'xesam:url': <'https://www.youtube.com/watch?v=TEST'>, \
'xesam:artist': <['Tester']>}>, 'PlaybackStatus': <'Playing'>}" "@as []"
```

`LILGUYS_DEBUG_MPRIS=1` prints every signal, its keys, and the extracted fields.

**Fails if:** nothing appears. First check the player actually emits — `gdbus monitor --session
--dest org.mpris.MediaPlayer2.<player>` should show traffic. A player that reports `Playing` at
position `0.000003` and emits nothing is a stale registration, not a sensor bug.

## 5. Cost — 15 seconds

```bash
PID=$(pgrep -x lilguysd)
a=$(awk '{print $14+$15}' /proc/$PID/stat); sleep 8; b=$(awk '{print $14+$15}' /proc/$PID/stat)
python3 -c "print(f'{($b-$a)/100.0/8.0*100:.2f}% of one core')"
grep VmRSS /proc/$PID/status
```

Expect about **0.5 % of one core** and **105 MiB**. The `frame` line in the HUD should read
around **0.15 ms**, and `tick` should drop from 60 Hz to 8 Hz when the pointer is far away and he
has settled.

**Fails if:** CPU sits above ~2 %, or `tick` never leaves 60 Hz.
