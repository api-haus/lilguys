# Talking to him

**Status:** built. Click him and a box opens where his thoughts appear; type, press Enter, and the
message reaches him at once. `lilguy say "…"` does the same thing without a pointer.

Click the guy, type something, he answers. The obvious feature, with one genuinely interesting
decision inside it.

## A message is an observation, but not an ordinary one

A typed message enters the stream like everything else — `Observation::Told { text }` — and reaches
the mind in the same account as a window focus and a hunger pang. That much follows from the
single-stream law and needs no argument.

**But it must not wait for the quantum.** Every other observation is allowed to sit in the bucket
for up to forty-five seconds because nothing is waiting on it. A person who has just typed at him
*is* waiting, and forty-five seconds of nothing is a bug, not restraint.

So: **a message closes the current slice immediately and sends it.** Not a side channel, not a
priority queue — the same slice, cut short. Everything accumulated so far goes with it, which is
correct: what he was in the middle of noticing is exactly the context for what you just said.

This is the one legitimate override of the pace, and it should stay the only one. Anything else
that wants to jump the queue should be argued for against this precedent.

## Getting the text

A layer surface has `keyboard-interactivity: none`, which is why he never steals your typing. Taking
keyboard focus means asking for `on_demand` while a box is open and giving it back after — a real
protocol dance with a real failure mode, which is that a bug leaves your keyboard captured.

Three routes were on the table, cheapest first. It went with 2, and 3 first as the design said:

1. **Borrow an existing prompt.** `fuzzel --dmenu`, `wofi --dmenu`, `rofi -dmenu`: spawn it, read a
   line from stdout, done. No keyboard handling in lilguys at all, no way to trap input, and it
   inherits whatever the user already themes. Ugly in that it is not *his* box, and it needs one of
   them installed.
2. **Own the input, carefully.** Keyboard interactivity on the surface only while the box is open,
   given back on Escape, on send, on focus loss and on a watchdog. This is the version that looks
   right, and it is the one that shipped — on the surface that already exists rather than a second
   one, because a second layer surface means a second wgpu context to draw one bubble in.

   **Exclusive, not on-demand.** On-demand focus is granted by a click, and the click that opens
   the box has already been delivered by the time the request reaches the compositor — so it would
   take a second click before a key arrived. Exclusive is what every dmenu-style launcher on
   Wayland asks for, and the ways out are Escape, Enter, losing focus, and 45 seconds of nothing.
3. **Take it from the inbox.** `lilguy say "…"` writes to the socket that already exists. Not a UI,
   but it is a one-line implementation and makes every other route testable before it is built.

Do 3 first because it costs nothing and unblocks the rest of the feature. Then 2, with 1 as a
documented fallback for compositors or setups where owning keyboard focus misbehaves.

## What he says back

Nothing new is needed. `speak` and `think` already exist, and a reply is just those tools called in
response to a slice that happened to contain a message.

Two things to get right:

- **A reply is not obligatory.** He may answer with a look. That must stay true, or the whole
  restraint collapses the moment someone talks to him.
- **He should not become a chatbot for the next ten minutes.** A message raises the odds of a reply
  for one slice, not a mode change. If a conversation is wanted, that is
  [agent-integration.md](agent-integration.md)'s job, not the reactor's.

## The bubble

Reuse the thought bubble geometry for the input box: same place above his head, same wrapping, same
flip-below-when-near-the-top. The typed text appears where his thoughts appear, which reads as
talking *to* him rather than opening a dialog.

## Work

1. ~~`Observation::Told { text }`, and a `lilguy say` that pushes one through the inbox.~~ Done, and
   it carries a `to`, so a cast can be addressed one at a time.
2. ~~Quantiser: a `Told` observation forces `take()` to return on the next tick.~~ Done — for the
   addressee. The rest of the room hears it at their own pace, which is right.
3. ~~Prompt: one line in `awareness` saying they can type at you and that an answer is optional.~~
   Done.
4. ~~Input surface (route 2), reusing the bubble renderer.~~ Done, on the existing surface.
5. ~~Click on him opens it; Escape closes it.~~ Done. A drag opens nothing — that was somebody
   moving him about, and telling the two apart is six pixels of pointer travel.

## Open questions

- Should a message he never answers still be visible to him later, or does it expire from the
  stream like anything else? Leaning expire — an unanswered message he brings up twenty minutes
  later is worse than one he let go.
- Does his reply go in a bubble, aloud, or both? Probably his choice, which means no decision here.
