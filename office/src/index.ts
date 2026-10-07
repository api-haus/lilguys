import { DurableObject } from "cloudflare:workers";
import page from "./floor.html";

interface Env {
  OFFICE: DurableObjectNamespace<Office>;
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
  cursor: number;
};

type Message = { id: number; at: number; room: string; sender: string; recipient: string | null; text: string };

const KITCHEN = "kitchen";
const BRIEFING_ENTRIES = 20;
const UNREAD_LIMIT = 50;

export class Office extends DurableObject<Env> {
  sql = this.ctx.storage.sql;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    ctx.blockConcurrencyWhile(async () => {
      this.sql.exec(`
        CREATE TABLE IF NOT EXISTS members (
          name TEXT PRIMARY KEY, owner TEXT NOT NULL, harness TEXT NOT NULL, room TEXT NOT NULL,
          state TEXT NOT NULL DEFAULT '', detail TEXT NOT NULL DEFAULT '',
          seen INTEGER NOT NULL, present INTEGER NOT NULL DEFAULT 0, cursor INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS messages (
          id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, room TEXT NOT NULL,
          sender TEXT NOT NULL, recipient TEXT, text TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS log (
          id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, name TEXT NOT NULL,
          kind TEXT NOT NULL CHECK (kind IN ('entry','pending','priority')), text TEXT NOT NULL);
      `);
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

  // A worker arrives through reception: it is handed the head of its own log and everything
  // addressed to it while it was away, then everybody present sees it enter.
  wakeup(name: string, owner: string, harness: string, room?: string) {
    const known = this.member(name);
    const at = Date.now();
    this.sql.exec(
      `INSERT INTO members (name, owner, harness, room, seen, present) VALUES (?, ?, ?, ?, ?, 1)
       ON CONFLICT(name) DO UPDATE SET owner = excluded.owner, harness = excluded.harness,
         room = COALESCE(?, members.room), seen = excluded.seen, present = 1, state = 'arrived', detail = ''`,
      name, owner, harness, room ?? owner, at, room ?? null,
    );
    const briefing = {
      returning: !!known,
      entries: this.sql
        .exec("SELECT at, text FROM log WHERE name = ? AND kind = 'entry' ORDER BY id DESC LIMIT ?", name, BRIEFING_ENTRIES)
        .toArray()
        .reverse(),
      pending: this.latest(name, "pending"),
      priority: this.latest(name, "priority"),
      unread: this.unread(name),
    };
    const me = this.member(name)!;
    this.broadcast({ t: "enter", member: me });
    return { briefing, me, present: this.present() };
  }

  latest(name: string, kind: "pending" | "priority") {
    return this.sql
      .exec<{ text: string; at: number }>("SELECT text, at FROM log WHERE name = ? AND kind = ? ORDER BY id DESC LIMIT 1", name, kind)
      .toArray()[0] ?? null;
  }

  // What reaches an identity: messages addressed to it, and unaddressed talk in its own room or the
  // kitchen. Everything else in the building is not its business.
  unread(name: string): Message[] {
    const me = this.member(name);
    if (!me) return [];
    const rows = this.sql
      .exec<Message>(
        `SELECT * FROM (SELECT * FROM messages WHERE id > ? AND sender != ?
           AND (recipient = ? OR (recipient IS NULL AND room IN (?, ?))) ORDER BY id DESC LIMIT ?) ORDER BY id`,
        me.cursor, name, name, me.room, KITCHEN, UNREAD_LIMIT,
      )
      .toArray();
    const top = this.sql.exec<{ id: number }>("SELECT COALESCE(MAX(id), 0) AS id FROM messages").one().id;
    this.sql.exec("UPDATE members SET cursor = ?, seen = ? WHERE name = ?", top, Date.now(), name);
    return rows;
  }

  say(sender: string, text: string, to?: string | null, room?: string | null) {
    const from = this.member(sender);
    const target = to ? this.member(to) : undefined;
    const where = room ?? target?.room ?? from?.room ?? KITCHEN;
    const msg = this.sql
      .exec<Message>(
        "INSERT INTO messages (at, room, sender, recipient, text) VALUES (?, ?, ?, ?, ?) RETURNING *",
        Date.now(), where, sender, to ?? null, text,
      )
      .one();
    this.broadcast({ t: "message", message: msg });
    return msg;
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

const json = (body: unknown, status = 200) => Response.json(body ?? { ok: true }, { status });

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/" && request.method === "GET") {
      return new Response(page, { headers: { "content-type": "text/html; charset=utf-8" } });
    }
    const token = request.headers.get("authorization")?.replace(/^Bearer /, "") ?? url.searchParams.get("token");
    if (token !== env.TOKEN) return json({ error: "unauthorized" }, 401);

    const office = env.OFFICE.getByName("main");
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
      case "POST /say":
        return missing("from", "text") ?? json(await office.say(b.from, b.text, b.to, b.room));
      case "GET /inbox":
        return missing("name") ?? json(await office.unread(b.name));
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
