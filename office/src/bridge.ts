export type Message = { id: number; at: number; room: string; sender: string; recipient: string | null; brief: string; text: string; sender_owner?: string | null };
export type Arrival = { name: string; owner: string; harness: string; room: string; icon?: string };

export type Frame =
  | { t: "message"; message: Message; from: Arrival | null }
  | { t: "enter"; member: Arrival }
  | { t: "leave"; name: string }
  | { t: "move"; member: Arrival }
  | { t: "close"; room: string };

export interface Bridge {
  relay(frame: Frame): Promise<void>;
  command(text: string): string | null | Promise<string | null>;
}

export const OFFICE_NAME = "main";

const HARNESS_ICONS: Record<string, string> = { claude: "✳️", codex: "🌀" };
const OWNER_ICONS = ["🦊", "🐸", "🐙", "🦉", "🐝", "🐢", "🦀", "🐳", "🦔", "🐌", "🦜", "🐞", "🦩", "🐿️", "🦭", "🐺"];

export const harnessIcon = (harness: string) => HARNESS_ICONS[harness] ?? "🤖";

// Each owner's animal is their own: the one their name hashes to, or the next one nobody has.
export function ownerIcon(owner: string, taken: string[] = []) {
  let h = 0;
  for (const c of owner) h = (h * 31 + c.codePointAt(0)!) >>> 0;
  for (let i = 0; i < OWNER_ICONS.length; i++) {
    const icon = OWNER_ICONS[(h + i) % OWNER_ICONS.length];
    if (!taken.includes(icon)) return icon;
  }
  return OWNER_ICONS[h % OWNER_ICONS.length];
}

// How an agent is shown in a messenger: what harness it runs in, its name, and whose it is.
export const label = (name: string, who: Pick<Arrival, "owner" | "harness" | "icon"> | null) =>
  who ? `${harnessIcon(who.harness)} ${name} · ${who.icon ?? ownerIcon(who.owner)} ${who.owner}` : name;
