import { DurableObject } from "cloudflare:workers";
import { type Arrival, type Bridge, type Frame, type Message, label, OFFICE_NAME } from "./bridge";
import { ALLOW_TABLE, addressee, allowed, command, isAllowed, isLogin, ownerOf, welcome } from "./heard";
import type { Office } from "./index";

export interface TelegramEnv {
  OFFICE: DurableObjectNamespace<Office>;
  TELEGRAM: DurableObjectNamespace<Telegram>;
  TELEGRAM_TOKEN?: string;
  TELEGRAM_CHAT?: string;
  OFFICE_URL: string;
}

const RECEPTION = "reception";
const TELEGRAM_LIMIT = 3500;
const ADMINS_TTL_MS = 5 * 60_000;
// Telegram serves every file as octet-stream; its path's extension is the only type it tells.
const MIME: Record<string, string> = {
  jpg: "image/jpeg", jpeg: "image/jpeg", png: "image/png", webp: "image/webp", gif: "image/gif",
  mp4: "video/mp4", webm: "video/webm", tgs: "application/x-tgsticker", oga: "audio/ogg", ogg: "audio/ogg", mp3: "audio/mpeg",
};

const roomOf = (name: string) => name.toLowerCase().replace(/[^a-z0-9_-]+/g, "-").slice(0, 100) || "office";
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

// The webhook's shared secret, derived from the bot token so there is nothing else to keep.
export async function webhookSecret(token: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`lilguys-office:${token}`));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

// A forum supergroup: a room is the topic of the same name, reception is General.
export class Telegram extends DurableObject<TelegramEnv> implements Bridge {
  sql = this.ctx.storage.sql;
  admins: { ids: Set<number>; at: number } | null = null;

  constructor(ctx: DurableObjectState, env: TelegramEnv) {
    super(ctx, env);
    ctx.blockConcurrencyWhile(async () => {
      this.sql.exec(`
        CREATE TABLE IF NOT EXISTS topics (room TEXT PRIMARY KEY, thread INTEGER NOT NULL UNIQUE);
        CREATE TABLE IF NOT EXISTS posted (message INTEGER PRIMARY KEY, sender TEXT NOT NULL, office INTEGER);
        CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT);
        ${ALLOW_TABLE};
      `);
      try { this.sql.exec("ALTER TABLE posted ADD COLUMN office INTEGER"); } catch {}
    });
  }

  get chat() {
    return this.env.TELEGRAM_CHAT!;
  }

  async api<T = any>(method: string, body: unknown): Promise<T> {
    for (;;) {
      const res = await fetch(`https://api.telegram.org/bot${this.env.TELEGRAM_TOKEN}/${method}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      const r = (await res.json()) as { ok: boolean; result: T; description?: string; parameters?: { retry_after?: number } };
      if (res.status === 429) {
        await new Promise((ok) => setTimeout(ok, (r.parameters?.retry_after ?? 1) * 1000));
        continue;
      }
      if (!r.ok) throw new Error(`telegram ${method}: ${res.status} ${r.description}`);
      return r.result;
    }
  }

  setKv(k: string, v: string) {
    this.sql.exec("INSERT INTO kv (k, v) VALUES (?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v", k, v);
  }

  // Points the bot's webhook at this Worker and reports what Telegram knows of the bot and the hook.
  async hook(url: string) {
    await this.api("setWebhook", { url, secret_token: await webhookSecret(this.env.TELEGRAM_TOKEN!), allowed_updates: ["message", "message_reaction"] });
    const [me, info] = await Promise.all([this.api("getMe", {}), this.api("getWebhookInfo", {})]);
    const kv = Object.fromEntries(this.sql.exec<{ k: string; v: string }>("SELECT k, v FROM kv").toArray().map((r) => [r.k, r.v]));
    return {
      bot: me.username,
      reads_all_group_messages: me.can_read_all_group_messages,
      pending_updates: info.pending_update_count,
      last_error: info.last_error_message ?? null,
      allowed: allowed(this.sql),
      topics: this.sql.exec("SELECT room, thread FROM topics").toArray(),
      ...kv,
    };
  }

  // ---- Telegram → office ----

  async update(u: any) {
    if (u.message_reaction) return this.reacted(u.message_reaction).catch((e) => this.setKv("error", `${new Date().toISOString()} ${e}`));
    const msg = u.message;
    if (!msg || msg.from?.is_bot) return;
    if (msg.chat?.type === "private") return this.private(msg).catch((e) => this.setKv("error", `${new Date().toISOString()} ${e}`));
    if (String(msg.chat?.id) !== this.chat) return;
    await this.heard(msg).catch((e) => this.setKv("error", `${new Date().toISOString()} ${e}`));
  }

  async heard(msg: any) {
    const thread: number | null = msg.is_topic_message ? msg.message_thread_id : null;
    const created = msg.forum_topic_created?.name ?? msg.forum_topic_edited?.name;
    if (created && thread) return this.learn(created, thread);
    const text = (msg.text ?? msg.caption ?? "").trim();
    const files = [...this.attachments(msg), ...(await this.customEmoji(msg))];
    if (!text && !files.length) return;

    const admin = String(msg.sender_chat?.id) === this.chat || (await this.isAdmin(msg.from?.id));
    const answer = admin ? command(this.sql, text) : null;
    if (answer !== null) return this.send(esc(answer), thread);
    const from = msg.from ?? {};
    const full = [from.first_name, from.last_name].filter(Boolean).join(" ");
    if (!admin && !isAllowed(this.sql, [from.username, from.first_name, full])) return;

    const room = thread ? this.roomFor(thread, msg.reply_to_message?.forum_topic_created?.name) : RECEPTION;
    const replied = msg.reply_to_message?.message_id;
    const poster = replied ? this.sql.exec<{ sender: string }>("SELECT sender FROM posted WHERE message = ?", replied).toArray()[0]?.sender : null;
    const office = this.env.OFFICE.getByName(OFFICE_NAME);
    const [to, body] = addressee([text, ...files].filter(Boolean).join("\n"), await office.memberNames(), poster ?? null);
    const said = await office.say(full || from.username || "someone", body, to, room, null, "telegram");
    this.sql.exec("INSERT OR REPLACE INTO posted (message, sender, office) VALUES (?, ?, ?)", msg.message_id, full || from.username || "someone", said.id);
  }

  // Only reactions added by someone the office hears, on a message it knows.
  async reacted(r: any) {
    if (String(r.chat?.id) !== this.chat) return;
    const from = r.user;
    const full = [from?.first_name, from?.last_name].filter(Boolean).join(" ");
    const heard = r.actor_chat ? String(r.actor_chat.id) === this.chat : !!from && ((await this.isAdmin(from.id)) || isAllowed(this.sql, [from.username, from.first_name, full]));
    const id = this.sql.exec<{ office: number | null }>("SELECT office FROM posted WHERE message = ?", r.message_id).toArray()[0]?.office;
    if (!heard || !id) return;
    const before = new Set((r.old_reaction ?? []).map((x: any) => JSON.stringify(x)));
    const added = (r.new_reaction ?? []).filter((x: any) => !before.has(JSON.stringify(x)));
    const office = this.env.OFFICE.getByName(OFFICE_NAME);
    const who = r.actor_chat?.title ?? (full || from.username);
    for (const x of added) {
      if (x.type === "emoji") await office.react(id, who, x.emoji, null);
      if (x.type === "custom_emoji") {
        const [st] = await this.api<any[]>("getCustomEmojiStickers", { custom_emoji_ids: [x.custom_emoji_id] }).catch(() => []);
        const picture = st ? this.picture(`custom emoji ${st.emoji ?? ""}`.trim(), st) : null;
        await office.react(id, who, st?.emoji ? `${st.emoji} (custom)` : "custom emoji", picture);
      }
    }
  }

  // Each file a message carries, as a line naming its kind and where the office serves it to keys.
  // What moves is offered by its still, Telegram's own thumbnail, with the clip itself as `clip:`;
  // an animated sticker or emoji is Lottie or webm and only its still can be looked at.
  attachments(msg: any) {
    const out: string[] = [];
    if (msg.photo?.length) out.push(`[photo] ${this.fileUrl(msg.photo.at(-1).file_id)}`);
    for (const kind of ["animation", "video", "video_note"]) {
      const f = msg[kind];
      if (!f) continue;
      const clip = `clip: ${this.fileUrl(f.file_id)}`;
      out.push(`[${kind}${f.file_name ? ` ${f.file_name}` : ""}] ${f.thumbnail ? `${this.fileUrl(f.thumbnail.file_id)} (${clip})` : clip}`);
    }
    if (msg.sticker) out.push(this.picture(`sticker ${[msg.sticker.emoji, msg.sticker.set_name].filter(Boolean).join(" ")}`, msg.sticker));
    for (const kind of ["document", "audio", "voice"]) {
      const f = msg[kind];
      if (f && !(kind === "document" && msg.animation)) out.push(`[${kind}${f.file_name ? ` ${f.file_name}` : ""}] ${this.fileUrl(f.file_id)}`);
    }
    return out;
  }

  fileUrl(id: string) {
    return `${this.env.OFFICE_URL}/files/telegram/${encodeURIComponent(id)}`;
  }

  picture(kind: string, sticker: any) {
    const moving = sticker.is_animated || sticker.is_video;
    if (!moving) return `[${kind}] ${this.fileUrl(sticker.file_id)}`;
    return sticker.thumbnail ? `[${kind}, still] ${this.fileUrl(sticker.thumbnail.file_id)}` : `[${kind}] clip: ${this.fileUrl(sticker.file_id)}`;
  }

  // Custom emoji are stickers underneath; each distinct one in the text or caption becomes a picture line.
  async customEmoji(msg: any) {
    const entities = [...(msg.entities ?? []), ...(msg.caption_entities ?? [])];
    const ids = [...new Set<string>(entities.filter((e: any) => e.type === "custom_emoji").map((e: any) => e.custom_emoji_id))];
    if (!ids.length) return [];
    const stickers = await this.api<any[]>("getCustomEmojiStickers", { custom_emoji_ids: ids }).catch(() => []);
    return stickers.map((st) => this.picture(`custom emoji ${st.emoji ?? ""}`.trim(), st));
  }

  // Streams a file the bot can see, so its token never leaves the Worker.
  async file(id: string) {
    const f = await this.api<{ file_path?: string }>("getFile", { file_id: id }).catch(() => null);
    if (!f?.file_path) return new Response("not available", { status: 404 });
    const res = await fetch(`https://api.telegram.org/file/bot${this.env.TELEGRAM_TOKEN}/${f.file_path}`);
    const ext = f.file_path.split(".").pop()!.toLowerCase();
    const type = MIME[ext] ?? res.headers.get("content-type") ?? "application/octet-stream";
    return new Response(res.body, { status: res.status, headers: { "content-type": type } });
  }

  learn(name: string, thread: number) {
    this.sql.exec("DELETE FROM topics WHERE thread = ?", thread);
    this.sql.exec("INSERT OR REPLACE INTO topics (room, thread) VALUES (?, ?)", roomOf(name), thread);
  }

  // A topic made before the bot joined is learned from the creation message every top-level post replies to.
  roomFor(thread: number, created?: string) {
    const known = this.sql.exec<{ room: string }>("SELECT room FROM topics WHERE thread = ?", thread).toArray()[0]?.room;
    if (known) return known;
    if (created) this.learn(created, thread);
    return created ? roomOf(created) : `topic-${thread}`;
  }

  // The group's owner and administrators.
  async isAdmin(user: number | undefined) {
    if (!this.admins || Date.now() - this.admins.at > ADMINS_TTL_MS) {
      const list = await this.api<{ user: { id: number } }[]>("getChatAdministrators", { chat_id: this.chat });
      this.admins = { ids: new Set(list.map((m) => m.user.id)), at: Date.now() };
    }
    return user !== undefined && this.admins.ids.has(user);
  }

  // A private chat with the bot: `/login` (or `/start`) hands a person who is heard in the group their own office key.
  async private(msg: any) {
    const reply = (html: string) => this.api("sendMessage", { chat_id: msg.chat.id, text: html, parse_mode: "HTML" });
    if (!isLogin(msg.text ?? "")) return reply("I'm the lilguys office. Send me /login to get your key; talk to the agents in the group's topics.");
    const from = msg.from ?? {};
    const full = [from.first_name, from.last_name].filter(Boolean).join(" ");
    if (!(await this.isAdmin(from.id)) && !isAllowed(this.sql, [from.username, from.first_name, full])) {
      return reply("Hi! I'm the lilguys office. I don't know you yet: ask an admin of the group to say <code>!allow your-name</code> there, then send me /login again.");
    }
    const owner = ownerOf(from.username ?? full);
    const token = await this.env.OFFICE.getByName(OFFICE_NAME).issue(owner);
    const { hello, steps } = welcome(this.env.OFFICE_URL, owner, token);
    await reply([esc(hello), esc(steps[0]), `<pre>${esc(steps[1])}</pre>`, esc(steps[2]), `<pre>${esc(steps[3])}</pre>`, esc(steps[4])].join("\n"));
  }

  command(text: string) {
    return command(this.sql, text);
  }

  // ---- office → Telegram ----

  async relay(frame: Frame) {
    if (!this.env.TELEGRAM_TOKEN || !this.env.TELEGRAM_CHAT) return;
    if (frame.t === "message") return this.post(frame.message, frame.from);
    const line =
      frame.t === "enter"
        ? `🚪 <b>${esc(label(frame.member.name, frame.member))}</b> walked in through reception, desk in ${esc(frame.member.room)}`
        : `👋 <b>${esc(frame.name)}</b> walked out`;
    await this.send(line, null);
  }

  async post(m: Message, from: Arrival | null) {
    const thread = await this.thread(m.room);
    const head = `<b>${esc(label(m.sender, from))}</b>${m.recipient ? ` → <b>${esc(m.recipient)}</b>` : ""}: `;
    for (let i = 0; i < m.text.length; i += TELEGRAM_LIMIT) {
      const sent = await this.send(head + esc(m.text.slice(i, i + TELEGRAM_LIMIT)), thread);
      this.sql.exec("INSERT OR REPLACE INTO posted (message, sender, office) VALUES (?, ?, ?)", sent.message_id, m.sender, m.id);
    }
  }

  send(html: string, thread: number | null) {
    return this.api<{ message_id: number }>("sendMessage", {
      chat_id: this.chat,
      ...(thread ? { message_thread_id: thread } : {}),
      text: html,
      parse_mode: "HTML",
      link_preview_options: { is_disabled: true },
    });
  }

  // The topic named after a room, created if the group has none; reception is General.
  async thread(room: string): Promise<number | null> {
    const name = roomOf(room);
    if (name === RECEPTION) return null;
    const known = this.sql.exec<{ thread: number }>("SELECT thread FROM topics WHERE room = ?", name).toArray()[0]?.thread;
    if (known) return known;
    const made = await this.api<{ message_thread_id: number }>("createForumTopic", { chat_id: this.chat, name });
    this.learn(name, made.message_thread_id);
    return made.message_thread_id;
  }
}
