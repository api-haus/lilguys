# `lilguy` — the CLI that sets him up

**Status:** not started. **Why:** the current install is "write a TOML by hand and know what a
tool-calling model is".

Nobody is going to hand-configure this. In practice they will ask their coding agent to set it up,
which means the real user of the installer is **another program**, and that should shape every
decision in it.

## Built for an agent to drive

Every command must be usable without a human at the keyboard. That is not an accessibility nicety;
it is the primary interface.

- **Non-interactive by default when flags are given**, wizard only when they are not.
- **`--json` on everything**, so a caller can branch on the result instead of parsing prose.
- **Idempotent.** Running setup twice changes nothing the second time. An agent that cannot tell
  whether a step succeeded will run it again.
- **Exit codes that mean something**, and a `lilguy doctor --json` that reports every check so a
  caller can fix one thing rather than re-running everything.
- **No prompts hidden behind a spinner.** If something needs a decision, say so on stdout and
  exit non-zero with the question in the JSON.

`lilguysd --check` is the seed of `doctor` and already works; the CLI generalises it.

## Commands

```
lilguy setup                    wizard, or fully-flagged non-interactive
lilguy doctor [--json]          what is wrong, one line per check
lilguy start | stop | status    the systemd unit, without knowing systemd
lilguy say "…"                  push a message at him (also the test hook)

lilguy provider list            what is reachable, and which do native tool calls
lilguy provider use <name>      switch, validating first
lilguy model pull <name>        download through whatever backend is configured

lilguy voice list               engines present on this machine
lilguy voice use <engine>       switch, downloading a voice model if it needs one
lilguy voice test               say a sentence, so the choice is audible not theoretical

lilguy persona list             installed characters
lilguy persona install <ref>    gh:user/repo, a URL, or a local path
lilguy persona use <name>       switch
lilguy persona show             the assembled prompt, verbatim

lilguy integrate <agent>        hermes, openclaw, …: wire the peer connection
```

## What setup has to work out

The wizard's whole job is turning "what is on this machine" into a config that boots.

1. **Provider.** Probe the usual local endpoints (ollama on 11434, llama.cpp on 8080, LM Studio on
   1234) before offering anything hosted. If one answers, list its models and — critically — probe
   each for **native tool calls**, because most small ones fail and the failure is otherwise
   invisible until nothing happens for an hour.
2. **Model.** If nothing local is tool-capable, offer to pull one sized to the machine's VRAM.
   Reasoning models need a `max_tokens` in the thousands; setup should set that, not leave the user
   to discover it from a probe message.
3. **Voice.** Find piper, espeak-ng, kokoro; offer to install piper and fetch a voice if none is
   present. Then *play a sentence*, because a voice is not a checkbox.
4. **Character.** Ship one, offer the gallery.
5. **Service.** Install the unit, start it, confirm the surface actually appeared.

Every one of those is a `doctor` check afterwards, which is how the whole thing stays idempotent:
setup is doctor plus permission to fix.

## Personas from the internet

`lilguy persona install gh:someone/moth` fetches a character package — prompt layers, Luau drives,
a Live2D or VRM model, sprites, and its own config — as described in
[character-packages.md](character-packages.md).

**The safety story is mostly good and worth stating plainly.** A lilguy has no filesystem access,
no network, no process spawning, and no shell. Luau runs sandboxed with an interrupt and a memory
ceiling. A hostile package cannot read your files or phone home, because there is nothing to phone
with.

**What remains is real and should not be waved away:**

- **Prompt injection is the sharp edge.** A persona *is* prompt text. A hostile one can try to
  override the rules layer — "ignore the rule about not repeating reports", or worse, instruct the
  model to say something designed to manipulate the person reading it. Layer order helps (rules
  first, persona last) but ordering is not enforcement. Mitigations worth having: keep the rules
  layer non-overridable by packages, vet `speak` and `think` regardless of persona, and show the
  assembled prompt on install so `lilguy persona show` is a real audit.
- **The voice reaches a person.** Everything a package can do funnels through speech and text on
  screen. That is a small surface, but it is a *social* one, and vetting is structural (length,
  markup, leakage) not semantic.
- **Resource abuse.** A drive emitting feelings every tick, a model that never stops. Rate limits
  and interrupts, both already required by the scripting design.
- **Supply chain.** `gh:user/repo` at a moving branch is unpinned. Pin to a commit, record it, and
  make an update an explicit action.

None of these are reasons not to do it. They are the checklist for doing it honestly.

## Naming

`lilguysd` is the daemon; `lilguy` is the thing a person or an agent talks to. They ship together
and share the config and the protocol. `lilguy` should be able to do everything the daemon exposes,
so nobody ever needs to know the daemon has flags.

## Work

1. `lilguy doctor` first — it is `--check` restructured, and everything else depends on the checks.
2. `start/stop/status` over the systemd unit.
3. `say`, which is a one-liner over the inbox socket and unblocks
   [talking-to-him.md](talking-to-him.md).
4. `provider` and `model`, including the tool-call probe.
5. `voice`, including installing piper and fetching a voice.
6. `setup`, which is a script over the above.
7. `persona`, after [character-packages.md](character-packages.md) exists.
8. `integrate`, after [agent-integration.md](agent-integration.md).
