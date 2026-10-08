import { DurableObject } from "cloudflare:workers";
import page from "./floor.html";
import { type Bridge, type Frame, type Message, OFFICE_NAME, ownerIcon } from "./bridge";
import { Discord, type DiscordEnv } from "./discord";
import { Telegram, type TelegramEnv, webhookSecret } from "./telegram";

export { Discord, Telegram };

interface Env extends DiscordEnv, TelegramEnv {
  TOKEN: string;
  OFFICE_URL: string;
  MIRROR_PEOPLE: boolean;
}

type Member = {
  name: string;
  owner: string;
  harness: string;
  room: string;
  state: string;
  detail: string;
  seen: number;
  present: number;
};

type Mail = Message & { state: "queued" | "open" | "done"; touched: number; reminded: number };

const KITCHEN = "kitchen";
const BRIEFING_ENTRIES = 20;
// How many messages may sit open in an agent's attention at once. The rest wait in the mailbox and
// are delivered as earlier ones are closed.
const OPEN_LIMIT = 3;
const BRIEF_CHARS = 80;

const briefOf = (text: string) => {
  const line = text.trim().split("\n")[0];
  return line.length > BRIEF_CHARS ? `${line.slice(0, BRIEF_CHARS - 1)}…` : line;
};

export class Office extends DurableObject<Env> {
  sql = this.ctx.storage.sql;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    ctx.blockConcurrencyWhile(async () => {
      this.sql.exec(`
        CREATE TABLE IF NOT EXISTS members (
          name TEXT PRIMARY KEY, owner TEXT NOT NULL, harness TEXT NOT NULL, room TEXT NOT NULL,
          state TEXT NOT NULL DEFAULT '', detail TEXT NOT NULL DEFAULT '',
          seen INTEGER NOT NULL, present INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS messages (
          id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, room TEXT NOT NULL,
          sender TEXT NOT NULL, recipient TEXT, brief TEXT NOT NULL DEFAULT '', text TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS mail (
          name TEXT NOT NULL, message INTEGER NOT NULL REFERENCES messages(id),
          state TEXT NOT NULL DEFAULT 'queued' CHECK (state IN ('queued','open','done')),
          touched INTEGER NOT NULL DEFAULT 0, reminded INTEGER NOT NULL DEFAULT 0,
          PRIMARY KEY (name, message));
        CREATE TABLE IF NOT EXISTS log (
          id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, name TEXT NOT NULL,
          kind TEXT NOT NULL CHECK (kind IN ('entry','pending','priority')), text TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS reactions (
          message INTEGER NOT NULL REFERENCES messages(id), who TEXT NOT NULL, emoji TEXT NOT NULL, at INTEGER NOT NULL,
          PRIMARY KEY (message, who, emoji));
        CREATE TABLE IF NOT EXISTS rooms (name TEXT PRIMARY KEY, opener TEXT NOT NULL, at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS owners (owner TEXT PRIMARY KEY, icon TEXT NOT NULL UNIQUE);
        CREATE TABLE IF NOT EXISTS people (owner TEXT PRIMARY KEY, hash TEXT NOT NULL UNIQUE, issued INTEGER NOT NULL);
      `);
      // Databases created before the mailbox.
      try { this.sql.exec("ALTER TABLE messages ADD COLUMN brief TEXT NOT NULL DEFAULT ''"); } catch {}
      try { this.sql.exec("ALTER TABLE members DROP COLUMN cursor"); } catch {}
      for (const { owner } of this.sql.exec<{ owner: string }>("SELECT owner FROM members GROUP BY owner ORDER BY MIN(seen)").toArray()) this.icon(owner);
      this.sql.exec(`UPDATE messages SET brief = substr(text, 1, ${BRIEF_CHARS}) WHERE brief = ''`);
    });
  }

  // A person's own key: a fresh one revokes the last. Only its hash is kept.
  async issue(owner: string) {
    const token = [...crypto.getRandomValues(new Uint8Array(24))].map((b) => b.toString(16).padStart(2, "0")).join("");
    this.sql.exec(
      "INSERT INTO people (owner, hash, issued) VALUES (?, ?, ?) ON CONFLICT(owner) DO UPDATE SET hash = excluded.hash, issued = excluded.issued",
      owner, await sha256(token), Date.now(),
    );
    return token;
  }

  async whois(token: string) {
    return this.sql.exec<{ owner: string }>("SELECT owner FROM people WHERE hash = ?", await sha256(token)).toArray()[0]?.owner ?? null;
  }

  // An identity belongs to the person who first woke it; the person's own name is theirs to speak as.
  owns(owner: string, name: string, claim = false) {
    const m = this.member(name);
    return m ? m.owner === owner : claim || name === owner;
  }

  member(name: string): (Member & { icon: string }) | undefined {
    const m = this.sql.exec<Member>("SELECT * FROM members WHERE name = ?", name).toArray()[0];
    return m && { ...m, icon: this.icon(m.owner) };
  }

  icon(owner: string) {
    const known = this.sql.exec<{ icon: string }>("SELECT icon FROM owners WHERE owner = ?", owner).toArray()[0]?.icon;
    if (known) return known;
    const icon = ownerIcon(owner, this.sql.exec<{ icon: string }>("SELECT icon FROM owners").toArray().map((r) => r.icon));
    this.sql.exec("INSERT INTO owners (owner, icon) VALUES (?, ?)", owner, icon);
    return icon;
  }

  broadcast(frame: object) {
    const line = JSON.stringify({ v: 1, ...frame });
    for (const ws of this.ctx.getWebSockets()) {
      try {
        ws.send(line);
      } catch {}
    }
  }

  // A worker arrives through reception and everybody present sees it enter.
  wakeup(name: string, owner: string, harness: string, room?: string) {
    const returning = !!this.member(name);
    this.sql.exec(
      `INSERT INTO members (name, owner, harness, room, seen, present) VALUES (?, ?, ?, ?, ?, 1)
       ON CONFLICT(name) DO UPDATE SET owner = excluded.owner, harness = excluded.harness,
         room = COALESCE(?, members.room), seen = excluded.seen, present = 1, state = 'arrived', detail = ''`,
      name, owner, harness, room ?? owner, Date.now(), room ?? null,
    );
    const me = this.member(name)!;
    this.broadcast({ t: "enter", member: me });
    this.relay({ t: "enter", member: me });
    return { briefing: { returning, ...this.briefing(name) }, me, present: this.present() };
  }

  // The folder the receptionist hands over: the head of the identity's own log.
  briefing(name: string) {
    return {
      entries: this.sql
        .exec("SELECT at, text FROM log WHERE name = ? AND kind = 'entry' ORDER BY id DESC LIMIT ?", name, BRIEFING_ENTRIES)
        .toArray()
        .reverse(),
      pending: this.latest(name, "pending"),
      priority: this.latest(name, "priority"),
    };
  }

  latest(name: string, kind: "pending" | "priority") {
    return this.sql
      .exec<{ text: string; at: number }>("SELECT text, at FROM log WHERE name = ? AND kind = ? ORDER BY id DESC LIMIT 1", name, kind)
      .toArray()[0] ?? null;
  }

  // Every messenger the office is laid over, by the name it signs its own messages with.
  bridges(): [string, Bridge][] {
    const { DISCORD_TOKEN, DISCORD_GUILD, TELEGRAM_TOKEN, TELEGRAM_CHAT } = this.env;
    const all: [string, Bridge][] = [];
    if (DISCORD_TOKEN && DISCORD_GUILD) all.push(["discord", this.env.DISCORD.getByName(DISCORD_GUILD)]);
    if (TELEGRAM_TOKEN && TELEGRAM_CHAT) all.push(["telegram", this.env.TELEGRAM.getByName(TELEGRAM_CHAT)]);
    return all;
  }

  // What a person types in one messenger is mirrored into the others only when MIRROR_PEOPLE is on.
  relay(frame: Frame, origin?: string | null) {
    if (origin && !this.env.MIRROR_PEOPLE) return;
    for (const [name, bridge] of this.bridges()) {
      if (name !== origin) this.ctx.waitUntil(bridge.relay(frame).catch((e) => console.error(`${name} relay: ${e}`)));
    }
  }

  memberNames() {
    return this.sql.exec<{ name: string }>("SELECT name FROM members").toArray().map((r) => r.name);
  }

  // An agent named anywhere in a message hears it, whatever room it was said in.
  mentioned(text: string, sender: string) {
    const escape = (n: string) => n.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return this.memberNames().filter((n) => n !== sender && new RegExp(`(^|[^\\w])@?${escape(n)}(?![\\w])`, "i").test(text));
  }

  // A message is mailed to whoever it is addressed to, or, unaddressed, to everybody whose desk is
  // in the room it was said in, and to every agent it names. Other kitchen talk is mailed to nobody.
  say(sender: string, text: string, to?: string | null, room?: string | null, brief?: string | null, origin?: string | null) {
    const from = this.member(sender);
    const target = to ? this.member(to) : undefined;
    const where = room ?? target?.room ?? from?.room ?? KITCHEN;
    const msg = this.sql
      .exec<Message>(
        "INSERT INTO messages (at, room, sender, recipient, brief, text) VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
        Date.now(), where, sender, to ?? null, brief?.trim() || briefOf(text), text,
      )
      .one();
    const heard = to
      ? [to]
      : where === KITCHEN
        ? []
        : this.sql.exec<{ name: string }>("SELECT name FROM members WHERE room = ? AND name != ?", where, sender).toArray().map((r) => r.name);
    const recipients = [...new Set([...heard, ...this.mentioned(text, sender)])];
    for (const name of recipients) this.sql.exec("INSERT INTO mail (name, message) VALUES (?, ?)", name, msg.id);
    this.broadcast({ t: "message", message: msg });
    this.relay({ t: "message", message: msg, from: from ?? null }, origin);
    for (const name of recipients) this.broadcast({ t: "mail", name });
    return msg;
  }

  mail(name: string, state: Mail["state"], limit = -1): Mail[] {
    return this.sql
      .exec<Mail>(
        "SELECT m.*, mail.state, mail.touched, mail.reminded FROM mail JOIN messages m ON m.id = mail.message WHERE mail.name = ? AND mail.state = ? ORDER BY m.id LIMIT ?",
        name, state, limit,
      )
      .toArray();
  }

  // Exactly once: each message moves from queued to open in one place, here, and only open messages
  // up to OPEN_LIMIT. Whoever calls this owns showing what it returns.
  deliver(name: string) {
    const room = OPEN_LIMIT - this.mail(name, "open").length;
    const fresh = room > 0 ? this.mail(name, "queued", room) : [];
    for (const m of fresh) this.sql.exec("UPDATE mail SET state = 'open' WHERE name = ? AND message = ?", name, m.id);
    return { delivered: fresh, ...this.box(name) };
  }

  box(name: string) {
    return {
      open: this.mail(name, "open"),
      queued: this.sql.exec<{ n: number }>("SELECT COUNT(*) AS n FROM mail WHERE name = ? AND state = 'queued'", name).one().n,
    };
  }

  // Open messages the agent has neither read again nor closed since they were delivered, each
  // offered once at the next moment it could act on them.
  remind(name: string) {
    const due = this.mail(name, "open").filter((m) => !m.touched && !m.reminded);
    for (const m of due) this.sql.exec("UPDATE mail SET reminded = 1 WHERE name = ? AND message = ?", name, m.id);
    return due;
  }

  // Only what was mailed to this identity can be read or closed by it.
  read(name: string, id: number) {
    const hit = this.sql.exec("UPDATE mail SET touched = 1 WHERE name = ? AND message = ? RETURNING message", name, id).toArray();
    if (!hit.length) return null;
    const reactions = this.sql.exec<{ who: string; emoji: string }>("SELECT who, emoji FROM reactions WHERE message = ? ORDER BY at", id).toArray();
    return { ...this.sql.exec<Message>("SELECT * FROM messages WHERE id = ?", id).one(), reactions };
  }

  // A reaction in a messenger, kept on the message, and mailed to the agent it concerns: whoever
  // said it, or else whoever it was addressed to. It is not relayed; the messengers show their own.
  react(id: number, who: string, emoji: string, picture: string | null) {
    const m = this.sql.exec<Message>("SELECT * FROM messages WHERE id = ?", id).toArray()[0];
    if (!m) return;
    const fresh = this.sql.exec("INSERT OR IGNORE INTO reactions (message, who, emoji, at) VALUES (?, ?, ?, ?) RETURNING message", id, who, emoji, Date.now()).toArray();
    const agent = [m.sender, m.recipient].find((n) => n && n !== who && this.member(n));
    if (!fresh.length || !agent) return;
    const text = `reacted ${emoji} to #${id} "${m.brief}"${picture ? `\n${picture}` : ""}`;
    const note = this.sql
      .exec<Message>(
        "INSERT INTO messages (at, room, sender, recipient, brief, text) VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
        Date.now(), m.room, who, agent, briefOf(text), text,
      )
      .one();
    this.sql.exec("INSERT INTO mail (name, message) VALUES (?, ?)", agent, note.id);
    this.broadcast({ t: "message", message: note });
    this.broadcast({ t: "mail", name: agent });
  }

  close(name: string, ids: number[]) {
    const closed = ids.filter(
      (id) => this.sql.exec("UPDATE mail SET state = 'done' WHERE name = ? AND message = ? AND state != 'done' RETURNING message", name, id).toArray().length,
    );
    this.broadcast({ t: "mail", name });
    return { closed, ...this.box(name) };
  }

  activity(name: string, state: string, detail: string) {
    this.sql.exec("UPDATE members SET state = ?, detail = ?, seen = ? WHERE name = ?", state, detail, Date.now(), name);
    this.broadcast({ t: "activity", name, state, detail });
  }

  note(name: string, kind: "entry" | "pending" | "priority", text: string) {
    this.sql.exec("INSERT INTO log (at, name, kind, text) VALUES (?, ?, ?, ?)", Date.now(), name, kind, text);
    this.broadcast({ t: "log", name, kind, text });
  }

  sleep(name: string, entry?: string, pending?: string, priority?: string) {
    if (entry) this.note(name, "entry", entry);
    if (pending !== undefined) this.note(name, "pending", pending);
    if (priority !== undefined) this.note(name, "priority", priority);
    this.sql.exec("UPDATE members SET present = 0, state = 'left', detail = '', seen = ? WHERE name = ?", Date.now(), name);
    this.broadcast({ t: "leave", name });
    this.relay({ t: "leave", name });
  }

  // Locking in: the desk moves to a room, where only its talk and what names or is addressed to the
  // agent reach the mailbox; with no room it goes back to its owner's. A room nobody has used yet is opened by whoever locks into it first.
  move(name: string, to?: string | null) {
    const room = to || this.member(name)!.owner;
    const fresh = !this.sql.exec("SELECT 1 FROM members WHERE room = ? UNION SELECT 1 FROM messages WHERE room = ?", room, room).toArray().length;
    if (fresh) this.sql.exec("INSERT OR IGNORE INTO rooms (name, opener, at) VALUES (?, ?, ?)", room, name, Date.now());
    this.sql.exec("UPDATE members SET room = ?, seen = ? WHERE name = ?", room, Date.now(), name);
    const me = this.member(name)!;
    this.broadcast({ t: "enter", member: me });
    this.relay({ t: "move", member: me });
    return { me, opened: fresh };
  }

  // Only a room an agent opened can be closed, by any identity of the same owner. Whoever still has a
  // desk there goes back to their owner's room, and each messenger deletes its channel or topic.
  closeRoom(name: string, room: string) {
    const opener = this.sql.exec<{ opener: string }>("SELECT opener FROM rooms WHERE name = ?", room).toArray()[0]?.opener;
    if (!opener) return { error: `#${room} was not opened by an agent, so it stays` };
    if (this.member(opener)?.owner !== this.member(name)?.owner) return { error: `#${room} was opened by ${opener}, not one of yours` };
    const moved = this.sql.exec<{ name: string }>("UPDATE members SET room = owner WHERE room = ? RETURNING name", room).toArray().map((r) => r.name);
    this.sql.exec("DELETE FROM rooms WHERE name = ?", room);
    for (const n of moved) this.broadcast({ t: "enter", member: this.member(n) });
    this.relay({ t: "close", room });
    return { closed: room, moved };
  }

  present() {
    return this.sql.exec<Member>("SELECT * FROM members WHERE present = 1 ORDER BY room, name").toArray();
  }

  snapshot() {
    return {
      members: this.sql.exec<Member>("SELECT * FROM members ORDER BY room, name").toArray().map((m) => ({ ...m, icon: this.icon(m.owner) })),
      messages: this.sql.exec<Message>("SELECT * FROM (SELECT * FROM messages ORDER BY id DESC LIMIT 200) ORDER BY id").toArray(),
    };
  }

  async fetch(request: Request) {
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({ owner: request.headers.get("x-office-owner") });
    server.send(JSON.stringify({ v: 1, t: "snapshot", ...this.snapshot() }));
    return new Response(null, { status: 101, webSocket: client });
  }

  // A person in the floor view talks the same way an agent does. Holding the office's own key, they
  // may also give the bridges commands; agents may not.
  async webSocketMessage(ws: WebSocket, raw: string | ArrayBuffer) {
    if (typeof raw !== "string") return;
    const f = JSON.parse(raw);
    if (f.t !== "say" || typeof f.from !== "string" || typeof f.text !== "string") return;
    const { owner } = ws.deserializeAttachment() as { owner: string | null };
    if (owner) {
      if (this.owns(owner, f.from)) this.say(f.from, f.text, f.to, f.room);
      return;
    }
    const answers = await Promise.all(this.bridges().map(async ([name, bridge]) => [name, await bridge.command(f.text.trim())] as const));
    const said = answers.filter(([, a]) => a !== null);
    if (said.length) for (const [name, a] of said) this.say("office", `${name}: ${a}`, null, KITCHEN);
    else this.say(f.from, f.text, f.to, f.room);
  }
}

export async function sha256(text: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

const json = (body: unknown, status = 200) => Response.json(body === undefined ? { ok: true } : body, { status });

export default {
  async scheduled(_event: ScheduledController, env: Env) {
    if (env.DISCORD_TOKEN && env.DISCORD_GUILD) await env.DISCORD.getByName(env.DISCORD_GUILD).ensure();
  },

  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/" && request.method === "GET") {
      return new Response(page, { headers: { "content-type": "text/html; charset=utf-8" } });
    }
    if (url.pathname === "/telegram" && request.method === "POST") {
      if (!env.TELEGRAM_TOKEN || !env.TELEGRAM_CHAT) return json({ error: "no Telegram group configured" }, 404);
      if (request.headers.get("x-telegram-bot-api-secret-token") !== (await webhookSecret(env.TELEGRAM_TOKEN))) {
        return json({ error: "unauthorized" }, 401);
      }
      await env.TELEGRAM.getByName(env.TELEGRAM_CHAT).update(await request.json());
      return json(undefined);
    }
    const token = request.headers.get("authorization")?.replace(/^Bearer /, "") ?? url.searchParams.get("token");
    const office = env.OFFICE.getByName(OFFICE_NAME);
    // The office's own key speaks for anyone; a person's key only for that person and the identities they woke.
    const owner = token === env.TOKEN ? null : token ? await office.whois(token) : null;
    if (token !== env.TOKEN && !owner) return json({ error: "unauthorized" }, 401);

    if (url.pathname === "/ws") {
      if (request.headers.get("upgrade") !== "websocket") return json({ error: "expected websocket" }, 426);
      const headers = new Headers(request.headers);
      if (owner) headers.set("x-office-owner", owner);
      return office.fetch(new Request(request, { headers }));
    }
    const b: any = request.method === "POST" ? await request.json() : Object.fromEntries(url.searchParams);
    const need = (...keys: string[]) => keys.filter((k) => typeof b[k] !== "string" || !b[k]);
    const missing = (...keys: string[]) => {
      const m = need(...keys);
      return m.length ? json({ error: `missing ${m.join(", ")}` }, 400) : null;
    };

    const route = `${request.method} ${url.pathname}`;
    const forbidden = async (name: unknown, claim = false) =>
      owner && typeof name === "string" && !(await office.owns(owner, name, claim))
        ? json({ error: `${name} is not ${owner}'s` }, 403)
        : null;
    if (owner && ["GET /discord", "GET /telegram", "POST /people"].includes(route)) return json({ error: "the office's own key only" }, 403);
    const denied = (await forbidden(b.name, route === "POST /wakeup")) ?? (await forbidden(b.from));
    if (denied) return denied;

    switch (route) {
      case "POST /people":
        return missing("owner") ?? json({ owner: b.owner, token: await office.issue(b.owner) });
      case "POST /wakeup":
        return missing("name", "harness") ?? json(await office.wakeup(b.name, owner ?? b.owner ?? "someone", b.harness, b.room));
      case "POST /say": {
        const err = missing("from", "text");
        if (err) return err;
        if (b.to === b.from) return json({ error: "a message to yourself is a log entry" }, 400);
        const msg = await office.say(b.from, b.text, b.to, b.room, b.brief);
        if (Number.isInteger(b.re)) await office.close(b.from, [b.re]);
        return json(msg);
      }
      case "GET /briefing":
        return missing("name") ?? json(await office.briefing(b.name));
      case "POST /deliver":
        return missing("name") ?? json(await office.deliver(b.name));
      case "GET /mail":
        return missing("name") ?? json(await office.box(b.name));
      case "POST /remind":
        return missing("name") ?? json(await office.remind(b.name));
      case "POST /read":
        if (!Number.isInteger(b.id)) return json({ error: "id must be a message number" }, 400);
        return missing("name") ?? json(await office.read(b.name, b.id));
      case "POST /done":
        if (!Array.isArray(b.ids) || !b.ids.every(Number.isInteger)) return json({ error: "ids must be message numbers" }, 400);
        return missing("name") ?? json(await office.close(b.name, b.ids));
      case "POST /activity":
        return missing("name", "state") ?? json(await office.activity(b.name, b.state, b.detail ?? ""));
      case "POST /log":
        if (!["entry", "pending", "priority"].includes(b.kind)) return json({ error: "kind is entry, pending or priority" }, 400);
        return missing("name", "text") ?? json(await office.note(b.name, b.kind, b.text));
      case "POST /room":
        return missing("name") ?? json(await office.move(b.name, b.room));
      case "POST /room/close": {
        const err = missing("name", "room");
        if (err) return err;
        const out = await office.closeRoom(b.name, b.room);
        return json(out, "error" in out ? 403 : 200);
      }
      case "POST /sleep":
        return missing("name") ?? json(await office.sleep(b.name, b.entry, b.pending, b.priority));
      case "GET /discord":
        if (!env.DISCORD_TOKEN || !env.DISCORD_GUILD) return json({ error: "no Discord server configured" }, 404);
        const bridge = env.DISCORD.getByName(env.DISCORD_GUILD);
        const failed = await bridge.ensure().then(() => null, (e) => String(e));
        return json({ ...(await bridge.report()), ensure: failed });
      case "GET /telegram":
        if (!env.TELEGRAM_TOKEN || !env.TELEGRAM_CHAT) return json({ error: "no Telegram group configured" }, 404);
        return json(await env.TELEGRAM.getByName(env.TELEGRAM_CHAT).hook(`${url.origin}/telegram`));
      case "GET /state":
        return json(await office.snapshot());
    }
    const file = url.pathname.match(/^\/files\/telegram\/([^/]+)$/);
    if (file && request.method === "GET" && env.TELEGRAM_TOKEN && env.TELEGRAM_CHAT) {
      return env.TELEGRAM.getByName(env.TELEGRAM_CHAT).file(decodeURIComponent(file[1]));
    }
    return json({ error: "not found" }, 404);
  },
};
