import { DurableObject } from "cloudflare:workers";
import page from "./floor.html";
import { type Bridge, type Frame, type Message, OFFICE_NAME } from "./bridge";
import { Discord, type DiscordEnv } from "./discord";

export { Discord };

interface Env extends DiscordEnv {
  TOKEN: string;
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
      `);
      // Databases created before the mailbox.
      try { this.sql.exec("ALTER TABLE messages ADD COLUMN brief TEXT NOT NULL DEFAULT ''"); } catch {}
      try { this.sql.exec("ALTER TABLE members DROP COLUMN cursor"); } catch {}
      this.sql.exec(`UPDATE messages SET brief = substr(text, 1, ${BRIEF_CHARS}) WHERE brief = ''`);
    });
  }

  member(name: string): Member | undefined {
    return this.sql.exec<Member>("SELECT * FROM members WHERE name = ?", name).toArray()[0];
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

  // A message is mailed to whoever it is addressed to, or, unaddressed, to everybody whose desk is
  // in the room it was said in. Kitchen talk is ambient and mailed to nobody.
  // Every messenger the office is laid over, by the name it signs its own messages with.
  bridges(): [string, Bridge][] {
    return this.env.DISCORD_TOKEN && this.env.DISCORD_GUILD ? [["discord", this.env.DISCORD.getByName(this.env.DISCORD_GUILD)]] : [];
  }

  relay(frame: Frame, origin?: string | null) {
    for (const [name, bridge] of this.bridges()) {
      if (name !== origin) this.ctx.waitUntil(bridge.relay(frame).catch((e) => console.error(`${name} relay: ${e}`)));
    }
  }

  memberNames() {
    return this.sql.exec<{ name: string }>("SELECT name FROM members").toArray().map((r) => r.name);
  }

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
    const recipients = to
      ? [to]
      : where === KITCHEN
        ? []
        : this.sql.exec<{ name: string }>("SELECT name FROM members WHERE room = ? AND name != ?", where, sender).toArray().map((r) => r.name);
    for (const name of recipients) this.sql.exec("INSERT INTO mail (name, message) VALUES (?, ?)", name, msg.id);
    this.broadcast({ t: "message", message: msg });
    this.relay({ t: "message", message: msg }, origin);
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
    return hit.length ? this.sql.exec<Message>("SELECT * FROM messages WHERE id = ?", id).one() : null;
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

  present() {
    return this.sql.exec<Member>("SELECT * FROM members WHERE present = 1 ORDER BY room, name").toArray();
  }

  snapshot() {
    return {
      members: this.sql.exec<Member>("SELECT * FROM members ORDER BY room, name").toArray(),
      messages: this.sql.exec<Message>("SELECT * FROM (SELECT * FROM messages ORDER BY id DESC LIMIT 200) ORDER BY id").toArray(),
    };
  }

  async fetch(request: Request) {
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    server.send(JSON.stringify({ v: 1, t: "snapshot", ...this.snapshot() }));
    return new Response(null, { status: 101, webSocket: client });
  }

  // A person in the floor view talks the same way an agent does.
  async webSocketMessage(_ws: WebSocket, raw: string | ArrayBuffer) {
    if (typeof raw !== "string") return;
    const f = JSON.parse(raw);
    if (f.t === "say" && typeof f.from === "string" && typeof f.text === "string") this.say(f.from, f.text, f.to, f.room);
  }
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
    const token = request.headers.get("authorization")?.replace(/^Bearer /, "") ?? url.searchParams.get("token");
    if (token !== env.TOKEN) return json({ error: "unauthorized" }, 401);

    const office = env.OFFICE.getByName(OFFICE_NAME);
    if (url.pathname === "/ws") {
      if (request.headers.get("upgrade") !== "websocket") return json({ error: "expected websocket" }, 426);
      return office.fetch(request);
    }
    const b: any = request.method === "POST" ? await request.json() : Object.fromEntries(url.searchParams);
    const need = (...keys: string[]) => keys.filter((k) => typeof b[k] !== "string" || !b[k]);
    const missing = (...keys: string[]) => {
      const m = need(...keys);
      return m.length ? json({ error: `missing ${m.join(", ")}` }, 400) : null;
    };

    switch (`${request.method} ${url.pathname}`) {
      case "POST /wakeup":
        return missing("name", "owner", "harness") ?? json(await office.wakeup(b.name, b.owner, b.harness, b.room));
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
      case "POST /sleep":
        return missing("name") ?? json(await office.sleep(b.name, b.entry, b.pending, b.priority));
      case "GET /state":
        return json(await office.snapshot());
    }
    return json({ error: "not found" }, 404);
  },
};
