# lilguys

## My input devices are mine
Never move my cursor, synthesise a click, or send keystrokes to my session — no `hyprctl dispatch
movecursor`, no `ydotool`, no virtual pointer — not even to test the thing I asked you to build, and
not even if you plan to put it back. I am using this machine while you work. Verify by reading
state, by a log line, by a screenshot of where things already are, or by handing me a QA case to
run. If a check needs input only I can make, write the steps and give them to me: `docs/qa-graybox.md`.

## The graybox skin is the default skin
The flat cartoony look in `src/avatar/graybox.rs` is not scaffolding to be replaced. It ships as
lilguys' iconic default, alongside the Live2D and VRM adapters. Keep it buildable and keep it
pretty; `annotate: false` is its shipping mode.
