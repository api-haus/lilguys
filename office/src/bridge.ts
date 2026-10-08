export type Message = { id: number; at: number; room: string; sender: string; recipient: string | null; brief: string; text: string };
export type Arrival = { name: string; owner: string; harness: string; room: string };

export type Frame =
  | { t: "message"; message: Message; from: Arrival | null }
  | { t: "enter"; member: Arrival }
  | { t: "leave"; name: string };

export interface Bridge {
  relay(frame: Frame): Promise<void>;
  command(text: string): string | null | Promise<string | null>;
}

export const OFFICE_NAME = "main";

const HARNESS_ICONS: Record<string, string> = { claude: "✳️", codex: "🌀" };
const OWNER_ICONS = ["🦊", "🐸", "🐙", "🦉", "🐝", "🐢", "🦀", "🐳", "🦔", "🐌", "🦜", "🐞", "🦩", "🐿️", "🦭", "🐺"];

export const harnessIcon = (harness: string) => HARNESS_ICONS[harness] ?? "🤖";

export function ownerIcon(owner: string) {
  let h = 0;
  for (const c of owner) h = (h * 31 + c.codePointAt(0)!) >>> 0;
  return OWNER_ICONS[h % OWNER_ICONS.length];
}

// How an agent is shown in a messenger: what harness it runs in, its name, and whose it is.
export const label = (name: string, who: Pick<Arrival, "owner" | "harness"> | null) =>
  who ? `${harnessIcon(who.harness)} ${name} · ${ownerIcon(who.owner)} ${who.owner}` : name;
