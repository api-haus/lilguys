import type { Office } from "./index";
import { MACHINES } from "./machines";

export const COINS = {
  murkoin: { name: "муркоін", icon: "🪙", aliases: ["murkoin", "murk", "муркоін", "муркоин", "мурк", "🪙"] },
  hrukoin: { name: "хрюкоін", icon: "🐽", aliases: ["hrukoin", "hruk", "хрюкоін", "хрюкоин", "хрюк", "🐽"] },
} as const;

export type Coin = keyof typeof COINS;

export const coinOf = (word: string): Coin | null =>
  (Object.keys(COINS) as Coin[]).find((c) => (COINS[c].aliases as readonly string[]).includes(word.toLowerCase())) ?? null;

export const show = (coin: Coin, amount: number) => `${amount} ${COINS[coin].name}${COINS[coin].icon}`;

// A reaction with a coin's emoji tips the message's author one of it.
export const tipOf = (emoji: string): Coin | null => (Object.keys(COINS) as Coin[]).find((c) => emoji.includes(COINS[c].icon)) ?? null;

// `!coins [name]`, `!give <name> <amount> <coin> [why]`, `!menu`, `!buy <item>`, and for admins `!mint <name> <amount> <coin> [why]`.
export async function coinCommand(office: DurableObjectStub<Office>, who: string, text: string, admin: boolean): Promise<string | null> {
  const m = text.match(/^[!/](coins|give|mint|menu|buy)(?:@\w+)?(?:\s+([\s\S]+))?$/i);
  if (!m) return null;
  const verb = m[1].toLowerCase();
  const args = (m[2] ?? "").trim();
  if (verb === "menu") return MACHINES.map((mc) => `${mc.name}: ${mc.items.map((i) => `${i.icon} ${i.name} ${show(i.coin, i.price)}`).join(" · ")}`).join("\n");
  if (verb === "buy") {
    const out = await office.buy(who, args);
    return "error" in out ? out.error : `${out.says.at(-1)}`;
  }
  if (verb === "coins") return walletLine(await office.wallet(args.replace(/^@/, "") || who));
  const p = args.match(/^@?(\S+)\s+(\d+)\s+(\S+)(?:\s+([\s\S]+))?$/);
  const coin = p && coinOf(p[3]);
  if (!p || !coin) return `usage: !${verb} <name> <amount> <муркоін|хрюкоін> [why]`;
  if (verb === "mint" && !admin) return "only admins mint";
  const out = await office.pay(coin, Number(p[2]), verb === "mint" ? null : who, p[1], p[4] ?? "");
  return "error" in out ? out.error : `${verb === "mint" ? "minted" : `${who} gave`} ${show(coin, Number(p[2]))} to ${p[1]}`;
}

export const walletLine = (w: { name: string; balance: Record<Coin, number> }) =>
  `${w.name}: ${(Object.keys(COINS) as Coin[]).map((c) => show(c, w.balance[c])).join(", ")}`;
