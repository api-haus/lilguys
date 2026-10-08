import { DurableObject } from "cloudflare:workers";
import { type Arrival, type Bridge, type Frame, type Message, label, OFFICE_NAME } from "./bridge";
import { ALLOW_TABLE, addressee, allowed, command, isAllowed, isLogin, ownerOf, welcome } from "./heard";
import type { Office } from "./index";

export interface DiscordEnv {
  OFFICE: DurableObjectNamespace<Office>;
  DISCORD: DurableObjectNamespace<Discord>;
  DISCORD_TOKEN?: string;
  DISCORD_GUILD?: string;
  OFFICE_URL: string;
}

const API = "https://discord.com/api/v10";
const GATEWAY = "https://gateway.discord.gg/?v=10&encoding=json";
// GUILDS, GUILD_MESSAGES, GUILD_MESSAGE_REACTIONS, DIRECT_MESSAGES, MESSAGE_CONTENT
const INTENTS = (1 << 0) | (1 << 9) | (1 << 10) | (1 << 12) | (1 << 15);
const ADMINISTRATOR = 1n << 3n;
const WATCHDOG_MS = 60_000;
const DISCORD_LIMIT = 2000;
const CATEGORY = "lilguys office";
const RECEPTION = "reception";

type Channel = { id: string; name: string; type: number; parent_id?: string | null };
type Guild = { owner_id: string; roles: { id: string; permissions: string }[] };

// A room is the text channel of the same name; agents post into it through a webhook as themselves.
export class Discord extends DurableObject<DiscordEnv> implements Bridge {
  sql = this.ctx.storage.sql;
  ws: WebSocket | null = null;
  beat: ReturnType<typeof setInterval> | null = null;
  channels: Map<string, Channel> | null = null;
  admins: { owner: string; roles: Set<string> } | null = null;

  constructor(ctx: DurableObjectState, env: DiscordEnv) {
    super(ctx, env);
    ctx.blockConcurrencyWhile(async () => {
      this.sql.exec(`
        CREATE TABLE IF NOT EXISTS gateway (k TEXT PRIMARY KEY, v TEXT);
        CREATE TABLE IF NOT EXISTS webhooks (channel TEXT PRIMARY KEY, id TEXT NOT NULL, token TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS posted (discord TEXT PRIMARY KEY, sender TEXT NOT NULL, office INTEGER);
        ${ALLOW_TABLE};
      `);
      try { this.sql.exec("ALTER TABLE posted ADD COLUMN office INTEGER"); } catch {}
    });
  }

  get guild() {
    return this.env.DISCORD_GUILD!;
  }

  kv(k: string): string | null {
    return this.sql.exec<{ v: string }>("SELECT v FROM gateway WHERE k = ?", k).toArray()[0]?.v ?? null;
  }
  setKv(k: string, v: string | null) {
    if (v === null) this.sql.exec("DELETE FROM gateway WHERE k = ?", k);
    else this.sql.exec("INSERT INTO gateway (k, v) VALUES (?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v", k, v);
  }

  async rest<T = any>(method: string, route: string, body?: unknown): Promise<T> {
    for (;;) {
      const res = await fetch(API + route, {
        method,
        headers: { authorization: `Bot ${this.env.DISCORD_TOKEN}`, "content-type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      if (res.status === 429) {
        const { retry_after = 1 } = (await res.json()) as { retry_after?: number };
        await new Promise((r) => setTimeout(r, retry_after * 1000));
        continue;
      }
      if (!res.ok) throw new Error(`discord ${method} ${route}: ${res.status} ${await res.text()}`);
      return (res.status === 204 ? null : await res.json()) as T;
    }
  }

  // ---- the gateway: Discord → office ----

  // Called by the alarm, the cron and every relay. Connects if not connected; the alarm keeps it so.
  async ensure() {
    if ((await this.ctx.storage.getAlarm()) === null) await this.ctx.storage.setAlarm(Date.now() + WATCHDOG_MS);
    if (this.ws && this.kv("intents") !== String(INTENTS)) {
      const old = this.ws;
      if (this.beat) clearInterval(this.beat);
      this.ws = this.beat = null;
      old.close(4000, "intents changed");
    }
    if (this.ws) return;
    // A resumed session keeps the intents it was identified with.
    const resume = this.kv("session") && this.kv("resume_url") && this.kv("intents") === String(INTENTS);
    // Workers open a WebSocket through fetch, which takes https:// where Discord hands out wss://.
    const url = resume ? `${this.kv("resume_url")!.replace(/^wss:/, "https:")}/?v=10&encoding=json` : GATEWAY;
    const res = await fetch(url, { headers: { upgrade: "websocket" } });
    const ws = res.webSocket;
    if (!ws) {
      for (const k of ["session", "resume_url", "seq"]) this.setKv(k, null);
      throw new Error(`gateway refused the upgrade: ${res.status}`);
    }
    ws.accept();
    this.ws = ws;
    ws.addEventListener("message", (e) => this.onGateway(ws, JSON.parse(e.data as string), !!resume));
    ws.addEventListener("close", (e) => this.drop(ws, e.code));
    ws.addEventListener("error", () => this.drop(ws, 0));
  }

  async alarm() {
    await this.ctx.storage.setAlarm(Date.now() + WATCHDOG_MS);
    await this.ensure();
  }

  drop(ws: WebSocket, code: number) {
    if (this.ws !== ws) return;
    if (this.beat) clearInterval(this.beat);
    this.ws = this.beat = null;
    // 4004 bad token, 4013/4014 bad or unapproved intents: reconnecting cannot help.
    this.setKv("closed", `${code} at ${new Date().toISOString()}`);
    if ([4004, 4013, 4014].includes(code)) console.error(`discord gateway closed for good: ${code}`);
    else this.ctx.waitUntil(this.ensure());
  }

  async onGateway(ws: WebSocket, p: { op: number; d: any; s: number | null; t: string | null }, resuming: boolean) {
    if (p.s !== null) this.setKv("seq", String(p.s));
    const send = (op: number, d: unknown) => ws.send(JSON.stringify({ op, d }));
    const seq = () => (this.kv("seq") ? Number(this.kv("seq")) : null);
    switch (p.op) {
      case 10:
        this.beat = setInterval(() => send(1, seq()), p.d.heartbeat_interval);
        if (resuming) send(6, { token: this.env.DISCORD_TOKEN, session_id: this.kv("session"), seq: seq() });
        else {
          this.setKv("intents", String(INTENTS));
          send(2, { token: this.env.DISCORD_TOKEN, intents: INTENTS, properties: { os: "linux", browser: "lilguys", device: "lilguys" } });
        }
        return;
      case 1:
        return send(1, seq());
      case 7:
        return ws.close(4000, "reconnect requested");
      case 9:
        if (!p.d) for (const k of ["session", "resume_url", "seq"]) this.setKv(k, null);
        return ws.close(4000, "invalid session");
      case 0:
        if (p.t === "READY") {
          this.setKv("session", p.d.session_id);
          this.setKv("resume_url", p.d.resume_gateway_url);
          this.setKv("me", p.d.user.id);
        } else if (p.t === "MESSAGE_CREATE" && p.d.guild_id === this.guild) {
          await this.heard(p.d).catch((e) => this.setKv("error", `${new Date().toISOString()} ${e}`));
        } else if (p.t === "MESSAGE_CREATE" && !p.d.guild_id && p.d.author?.id !== this.kv("me")) {
          await this.private(p.d).catch((e) => this.setKv("error", `${new Date().toISOString()} ${e}`));
        } else if (p.t?.startsWith("CHANNEL_")) {
          this.channels = null;
        } else if (p.t === "MESSAGE_REACTION_ADD" && p.d.guild_id === this.guild) {
          await this.reacted(p.d).catch((e) => this.setKv("error", `${new Date().toISOString()} ${e}`));
        } else if (p.t === "GUILD_UPDATE" || p.t?.startsWith("GUILD_ROLE_")) {
          this.admins = null;
        }
    }
  }

  async report() {
    return {
      connected: !!this.ws,
      alarm: await this.ctx.storage.getAlarm(),
      ...Object.fromEntries(["session", "seq", "closed", "error", "me"].map((k) => [k, this.kv(k)])),
      allowed: allowed(this.sql),
    };
  }

  async heard(d: any) {
    if (d.author?.id === this.kv("me")) return;
    if (d.webhook_id && this.sql.exec("SELECT 1 FROM webhooks WHERE id = ?", d.webhook_id).toArray().length) return;
    const text = (d.content ?? "").trim();
    const files = [
      ...(d.attachments ?? []).map((a: any) => `[${a.content_type ?? "file"} ${a.filename}] ${/^(video|audio)\//.test(a.content_type ?? "") ? "clip: " : ""}${a.url}`),
      ...(d.sticker_items ?? []).map((st: any) => `[sticker ${st.name}] https://media.discordapp.net/stickers/${st.id}.png`),
      ...[...new Set<string>(text.match(/<a?:\w+:\d+>/g) ?? [])].map((e) => {
        const [, animated, name, id] = e.match(/<(a?):(\w+):(\d+)>/)!;
        return `[custom emoji :${name}:] https://cdn.discordapp.com/emojis/${id}.${animated ? "gif" : "png"}`;
      }),
    ];
    if (!text && !files.length) return;
    const channel = (await this.channelMap()).get(d.channel_id);
    if (!channel) return;
    const admin = await this.isAdmin(d.author.id, d.member?.roles ?? []);
    const answer = admin ? command(this.sql, text) : null;
    if (answer !== null) {
      await this.rest("POST", `/channels/${channel.id}/messages`, { content: answer, allowed_mentions: { parse: [] } });
      return;
    }
    if (!admin && !isAllowed(this.sql, [d.author.username, d.author.global_name, d.member?.nick, `<@${d.author.id}>`])) return;
    const office = this.env.OFFICE.getByName(OFFICE_NAME);
    const names = await office.memberNames();
    const sender = d.member?.nick ?? d.author.global_name ?? d.author.username;

    const replied = d.message_reference?.message_id;
    const poster = replied ? this.sql.exec<{ sender: string }>("SELECT sender FROM posted WHERE discord = ?", replied).toArray()[0]?.sender : null;
    const [to, body] = addressee([text, ...files].filter(Boolean).join("\n"), names, poster ?? null);
    const said = await office.say(sender, body, to, channel.name, null, "discord");
    this.sql.exec("INSERT OR REPLACE INTO posted (discord, sender, office) VALUES (?, ?, ?)", d.id, sender, said.id);
  }

  async reacted(d: any) {
    const user = d.member?.user;
    if (!user || d.user_id === this.kv("me") || user.bot) return;
    const heard = (await this.isAdmin(d.user_id, d.member.roles ?? [])) || isAllowed(this.sql, [user.username, user.global_name, d.member.nick, `<@${d.user_id}>`]);
    const id = this.sql.exec<{ office: number | null }>("SELECT office FROM posted WHERE discord = ?", d.message_id).toArray()[0]?.office;
    if (!heard || !id) return;
    const custom = d.emoji.id ? `https://cdn.discordapp.com/emojis/${d.emoji.id}.${d.emoji.animated ? "gif" : "png"}` : null;
    const who = d.member.nick ?? user.global_name ?? user.username;
    await this.env.OFFICE.getByName(OFFICE_NAME).react(id, who, custom ? `:${d.emoji.name}:` : d.emoji.name, custom && `[custom emoji :${d.emoji.name}:] ${custom}`);
  }

  // ---- who the office listens to: server admins, and the people they allow ----

  async isAdmin(user: string, memberRoles: string[]) {
    if (!this.admins) {
      const g = await this.rest<Guild>("GET", `/guilds/${this.guild}`);
      const roles = g.roles.filter((r) => (BigInt(r.permissions) & ADMINISTRATOR) !== 0n).map((r) => r.id);
      this.admins = { owner: g.owner_id, roles: new Set(roles) };
    }
    const { owner, roles } = this.admins;
    return user === owner || roles.has(this.guild) || memberRoles.some((r) => roles.has(r));
  }

  // A direct message: `!login` hands a person who is heard on the server their own office key.
  async private(d: any) {
    const reply = (content: string) => this.rest("POST", `/channels/${d.channel_id}/messages`, { content, allowed_mentions: { parse: [] } });
    if (!isLogin(d.content ?? "")) return reply("I'm the lilguys office. Send me `login` to get your key; talk to the agents in the server's channels.");
    const member = await this.rest<{ nick?: string; roles: string[] }>("GET", `/guilds/${this.guild}/members/${d.author.id}`).catch(() => null);
    const heard = member && ((await this.isAdmin(d.author.id, member.roles)) || isAllowed(this.sql, [d.author.username, d.author.global_name, member.nick, `<@${d.author.id}>`]));
    if (!heard) return reply("Hi! I'm the lilguys office. I don't know you yet: ask an admin of the server to say `!allow <your name>` there, then message me `login` again.");
    const owner = ownerOf(d.author.username);
    const token = await this.env.OFFICE.getByName(OFFICE_NAME).issue(owner);
    const { hello, steps } = welcome(this.env.OFFICE_URL, owner, token);
    await reply([hello, steps[0], "```sh\n" + steps[1] + "\n```", steps[2], "```sh\n" + steps[3] + "\n```", steps[4]].join("\n"));
  }

  command(text: string) {
    return command(this.sql, text);
  }

  // ---- posting: office → Discord ----

  async relay(frame: Frame) {
    if (!this.env.DISCORD_TOKEN || !this.env.DISCORD_GUILD) return;
    await this.ensure();
    if (frame.t === "message") return this.post(frame.message, frame.from);
    const line =
      frame.t === "enter"
        ? `🚪 **${label(frame.member.name, frame.member)}** walked in through reception, desk in #${frame.member.room}`
        : `👋 **${frame.name}** walked out`;
    const channel = await this.channel(RECEPTION);
    await this.rest("POST", `/channels/${channel.id}/messages`, { content: line, allowed_mentions: { parse: [] } });
  }

  async post(m: Message, from: Arrival | null) {
    const channel = await this.channel(m.room);
    const hook = await this.webhook(channel.id);
    const text = m.recipient ? `→ **${m.recipient}**: ${m.text}` : m.text;
    for (let i = 0; i < text.length; i += DISCORD_LIMIT) {
      const sent = await this.rest<{ id: string }>("POST", `/webhooks/${hook.id}/${hook.token}?wait=true`, {
        content: text.slice(i, i + DISCORD_LIMIT),
        username: label(m.sender, from).slice(0, 80),
        allowed_mentions: { parse: [] },
      });
      this.sql.exec("INSERT OR REPLACE INTO posted (discord, sender, office) VALUES (?, ?, ?)", sent.id, m.sender, m.id);
    }
  }

  async channelMap() {
    if (!this.channels) {
      const all = await this.rest<Channel[]>("GET", `/guilds/${this.guild}/channels`);
      this.channels = new Map(all.map((c) => [c.id, c]));
    }
    return this.channels;
  }

  // The text channel named after a room, created under the office's category if the server has none.
  async channel(room: string): Promise<Channel> {
    const name = room.toLowerCase().replace(/[^a-z0-9_-]+/g, "-").slice(0, 100) || "office";
    const all = [...(await this.channelMap()).values()];
    const found = all.find((c) => c.type === 0 && c.name === name);
    if (found) return found;
    let category = all.find((c) => c.type === 4 && c.name.toLowerCase() === CATEGORY);
    if (!category) category = await this.rest<Channel>("POST", `/guilds/${this.guild}/channels`, { name: CATEGORY, type: 4 });
    const made = await this.rest<Channel>("POST", `/guilds/${this.guild}/channels`, { name, type: 0, parent_id: category.id });
    this.channels = null;
    return made;
  }

  async webhook(channel: string) {
    const known = this.sql.exec<{ id: string; token: string }>("SELECT id, token FROM webhooks WHERE channel = ?", channel).toArray()[0];
    if (known) return known;
    const made = await this.rest<{ id: string; token: string }>("POST", `/channels/${channel}/webhooks`, { name: "lilguys" });
    this.sql.exec("INSERT INTO webhooks (channel, id, token) VALUES (?, ?, ?)", channel, made.id, made.token);
    return made;
  }
}
