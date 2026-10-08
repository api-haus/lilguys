// What every messenger bridge decides the same way about a message it heard.

export const ALLOW_TABLE = "CREATE TABLE IF NOT EXISTS allowed (name TEXT PRIMARY KEY)";

export function allowed(sql: SqlStorage) {
  return sql.exec<{ name: string }>("SELECT name FROM allowed ORDER BY name").toArray().map((r) => r.name);
}

export function isAllowed(sql: SqlStorage, names: (string | null | undefined)[]) {
  const said = names.filter(Boolean).map((n) => n!.toLowerCase());
  return allowed(sql).some((n) => said.includes(n));
}

// `!allow <name>` and `!disallow <name>`; Telegram's `/allow@bot` form too.
export function command(sql: SqlStorage, text: string): string | null {
  const m = text.match(/^[!/](allow|disallow)(?:@\w+)?(?:\s+@?(.+))?$/i);
  if (!m) return null;
  const name = m[2]?.trim().toLowerCase();
  if (name && m[1].toLowerCase() === "allow") sql.exec("INSERT OR IGNORE INTO allowed (name) VALUES (?)", name);
  if (name && m[1].toLowerCase() === "disallow") sql.exec("DELETE FROM allowed WHERE name = ?", name);
  const list = allowed(sql);
  return `the office listens to admins${list.length ? ` and ${list.join(", ")}` : " only"}`;
}

// Who a message is for: the agent whose post it replies to, or a member named at its start.
export function addressee(text: string, names: string[], repliedTo: string | null): [string | null, string] {
  if (repliedTo) return [repliedTo, text];
  const named = text.match(/^@?([\w.-]+)[:,]?\s+([\s\S]+)$/);
  return named && names.includes(named[1]) ? [named[1], named[2]] : [null, text];
}

export const isLogin = (text: string) => /^[!/](login|start)(?:@\w+)?$/i.test(text.trim());

export const ownerOf = (name: string) => name.toLowerCase().replace(/[^a-z0-9_.-]+/g, "-").slice(0, 40);

// What a person is handed in private when they log in: their own key and how to use it.
export function welcome(url: string, owner: string, token: string) {
  const config = JSON.stringify({ url, owner, token });
  return [
    `You are ${owner} in the lilguys office. This key is yours alone; asking again replaces it.`,
    `Put this in ~/.config/lilguys/office.json:`,
    config,
    `Then install the plugin: claude plugin marketplace add api-haus/lilguys && claude plugin install lilguys@lilguys`,
    `and in any Claude Code session run /lilguys:wakeup <identity> (in Codex: $wakeup <identity>).`,
  ];
}
