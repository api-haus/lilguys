# Persistent objects, and whether they need representing at all

**Status:** open question, deliberately unresolved. **Do not build until the case is made.**

An event stream is made of changes. The world is made of things. A window that has been open for
six hours generates no events and therefore, to the mind, does not exist. Is that a bug?

## The tempting answer, and why it is probably wrong

The obvious move is an inventory: put the current window layout, the open applications, whatever
else persists, into the framing line of every slice. Nothing goes stale, the mind always knows what
is there.

**Persistence is not salience, and an inventory conflates them.** A window open for six hours is
maximally persistent and almost perfectly unsalient. Restating it every forty-five seconds teaches
the model, by sheer repetition, that it matters — and the model will eventually say something about
it. The entire gate exists to make silence cheap; an inventory quietly undoes that at the framing
line, where the gate cannot reach.

It also costs tokens on every turn forever, for information that is almost never the reason
anything happens.

## The asymmetry that may dissolve the problem

The **body** needs to know what exists — it must find a window to drift toward. The **mind** mostly
does not, and `focus` already demonstrates why:

```
mind:  focus(window, match: "zen")
body:  resolves "zen" against live compositor state, finds the rect, goes there
```

The mind named a thing loosely and was understood. It did not need an inventory, a handle, or a
correct name. **The mind refers; the body resolves.** If that pattern holds for everything the mind
might want to point at, persistent objects never need representing at all — they need only to be
*referrable*.

That is worth testing before building anything: collect the cases where the mind wanted to refer to
something and could not, and see whether they are about persistence or about vocabulary.

## Options, in the order they should be tried

1. **Nothing.** Things enter attention when they change and leave when they stop mattering, which
   is what attention is. Test how often this is actually wrong before assuming it is.
2. **Widen what can be referred to.** If `focus` can name a window loosely, so can other verbs.
   Adding referents is cheaper than adding state.
3. **Ask, do not tell.** A `look_around` capability that returns the current layout *when the mind
   wants it* costs nothing on the turns it is not called. This keeps the inventory out of the
   framing line and puts the decision where it belongs. The cost is a second round trip on the
   turns it is used, which at this cadence is free.
4. **A bounded framing inventory.** Last resort. If it happens, it must be small, stable in
   wording, and capped — and the prompt must say explicitly that it is furniture, not news.

Option 3 is the interesting one, because it inverts the cost: nothing is spent until curiosity
spends it, which is the same shape as every other decision in this design.

## Memory, for the record

**Short-term memory is the context window, and that is the whole of it.** Whatever a model can hold
across a conversation is what it gets. The compaction pass already summarises what falls out. There
is no work to do here beyond keeping the window well-fed and honestly compacted.

**Long-term memory is the question of what gets woven into the sutra.** Not a store bolted to the
side — the thread already persists, and it is already the only thing that does. Something learned
lives in attention until it is either forgotten at the next compaction or promoted into what the
character *is*. That promotion is the whole design problem; the destination is not in doubt. See
[philosophy.md](../philosophy.md) §"The sutra".

It should still emerge from working on interactivity — from finding out what a character actually
needs to remember to be good company — rather than from deciding up front. The reactor's private
notes are the seed: already his own words, and nothing yet reads them back.

Deferred on purpose. Revisit when a specific, felt absence names itself.
