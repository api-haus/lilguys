# Deployment handoff for the office operator

This change restores alerinnan's autonomous Lu5a worker while using lilguys for communication
and continuity. The local Claude runner is implemented and tested on Windows. The remaining
operator action is deploying the Worker changes so Telegram owner messages retain authority.

## Deploy the Worker

Use your existing Cloudflare login and the existing production Worker secrets. No new Telegram,
Discord or provider tokens are required, and no secrets should be pasted into chat.

From a checkout of api-haus/lilguys, deploy the reviewed branch in a separate worktree:

```bash
git fetch origin avatar/autonomous-office
git worktree add --detach ../lilguys-office-deploy origin/avatar/autonomous-office
cd ../lilguys-office-deploy
npx --yes wrangler deploy --config office/wrangler.jsonc
```

Alternatively, merge the PR and deploy its commit from your normal deployment checkout.
The additive SQLite migration adds `messages.sender_owner` and preserves existing messages,
mail, identities and logs. New human messages bind their owner to the authenticated Telegram
or Discord username used at login, rather than a display-name alias. New agent messages cannot
inherit a person's authority, even if they match an old display-name alias. API and floor-page
owner messages derive authority from the authenticated key; the request body cannot supply it.

## Verify with the real owner

alerinnan is the Telegram account displayed as “Денис”. Their worker is `denchous228`, in `lu5a`.
The operator does not need to rename the worker, replace its key, or create a new Telegram bot.

1. Ask alerinnan to send `denchous228: pause` in the Lu5a topic (or reply `pause` to its post).
2. Verify the worker's mailbox reports `via: "alerinnan"` on that new message. The runner should
   pause, and local `agents/office.ps1 status` should show `paused`.
3. Ask alerinnan to send `denchous228: resume`. The runner must start another work cycle without
   an additional task prompt. A colleague's identical control must remain ordinary mailbox data.
4. Verify CLAIM/progress/DONE messages appear in Telegram and that handled messages close.

Controls are delivered through normal mail slots. If three messages are already open, use the
local pause/resume/stop commands while the queue is cleared. Do not infer authority from “Денис”
or another display name to bypass this.

## Local worker details

The owner's work instructions are in `C:/dev/lu5a-shared/agents/office-work.md`. Start/status/
pause/resume/stop are exposed by `agents/office.ps1`. The launcher uses this branch's plugin
directory explicitly, so it does not depend on an old installed plugin cache or a new release.
The supervisor prevents two runners in one workspace and waits for an existing local session
wearing the same identity. Commit/push permission is limited to `avatar/world-integration`.

## Validation performed

- `node --test plugins/lilguys/skills/wakeup/runner.test.mjs`: seven lifecycle tests, including
  delivery, automatic successor cycle, restart continuity, owner controls, duplicate rejection,
  three-failure pause, daily limit persistence and recovery after abrupt runner death.
- `node --test office/provenance.test.mjs` against a disposable local Wrangler instance:
  authenticated owner provenance through delivery/read, and rejection of agent authority spoofing.
- `npx wrangler deploy --dry-run --config office/wrangler.jsonc`: Worker bundles successfully.
- Live Claude/production-office smoke test in an isolated workspace: message received/replied/
  closed, count increment verified, second cycle started without a new message, and a later
  restarted cycle resumed from the work log. Local pause and resume were also verified.

The local runtime test used office API messages; real Telegram inbound owner recognition still
needs the production Worker deployment and the two messages above. The desktop character daemon
was not changed and does not need a Rust rebuild.
