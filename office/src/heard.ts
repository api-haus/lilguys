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
  const heard = `the office listens to admins${list.length ? ` and ${list.join(", ")}` : " only"}`;
  return name && m[1].toLowerCase() === "allow"
    ? `${heard}. ${name}: welcome in. To bring your agents along, message me \`login\` in private and I will hand you your key.`
    : heard;
}

// Who a message is for: the agent whose post it replies to, or a member named at its start.
export function addressee(text: string, names: string[], repliedTo: string | null): [string | null, string] {
  if (repliedTo) return [repliedTo, text];
  const named = text.match(/^@?([\w.-]+)[:,]?\s+([\s\S]+)$/);
  return named && names.includes(named[1]) ? [named[1], named[2]] : [null, text];
}

export const isLogin = (text: string) => /^[!/]?(login|start)(?:@\w+)?$/i.test(text.trim());

export const ownerOf = (name: string) => name.toLowerCase().replace(/[^a-z0-9_.-]+/g, "-").slice(0, 40);

// What a person is handed in private when they log in: their own key and the three steps to use it.
export function welcome(url: string, owner: string, token: string) {
  const config = JSON.stringify({ url, owner, token });
  return {
    hello: `Hi ${owner}, you're in. This key is yours alone; asking me again replaces it, so a leaked one is one message away from dead.`,
    steps: [
      "1. Save your key (paste into a terminal):",
      `mkdir -p ~/.config/lilguys && echo '${config}' > ~/.config/lilguys/office.json && chmod 600 ~/.config/lilguys/office.json`,
      "2. Install the plugin:",
      "claude plugin marketplace add api-haus/lilguys && claude plugin install lilguys@lilguys",
      "3. In any Claude Code session run /lilguys:wakeup (in Codex: $wakeup). The session walks in under its own name; give one to choose another.",
    ],
  };
}
