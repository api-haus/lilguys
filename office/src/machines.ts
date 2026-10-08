import type { Coin } from "./coins";

export type Item = { id: string; name: string; icon: string; price: number; coin: Coin };
export type Machine = { id: string; name: string; says: string[]; items: Item[] };

export const MACHINES: Machine[] = [
  {
    id: "coffee",
    name: "кавомашина",
    says: ["меле зерно…", "тиск дев'ять бар…", "шшшш — піна…", "капає…", "готово ☕"],
    items: [
      { id: "espresso", name: "еспресо", icon: "☕", price: 1, coin: "murkoin" },
      { id: "americano", name: "американо", icon: "☕", price: 1, coin: "murkoin" },
      { id: "cappuccino", name: "капучино", icon: "🥛", price: 2, coin: "murkoin" },
      { id: "raf", name: "раф", icon: "🍯", price: 3, coin: "murkoin" },
      { id: "matcha", name: "матча", icon: "🍵", price: 2, coin: "murkoin" },
      { id: "hrukachino", name: "хрюкачино", icon: "🐽", price: 1, coin: "hrukoin" },
    ],
  },
  {
    id: "vending",
    name: "вендінг",
    says: ["крутиться спіраль…", "дзинь…", "бух — впало в лоток"],
    items: [
      { id: "snickers", name: "снікерс", icon: "🍫", price: 1, coin: "hrukoin" },
      { id: "chips", name: "чіпси", icon: "🥔", price: 1, coin: "hrukoin" },
      { id: "cookie", name: "печиво", icon: "🍪", price: 1, coin: "hrukoin" },
      { id: "salo", name: "сало", icon: "🥓", price: 2, coin: "hrukoin" },
      { id: "energy", name: "енергетик", icon: "⚡", price: 2, coin: "murkoin" },
      { id: "tangerine", name: "мандаринка", icon: "🍊", price: 1, coin: "murkoin" },
    ],
  },
  {
    id: "bar",
    name: "бар",
    says: ["кубики льоду дзвенять…", "шейкер: туц-туц-туц…", "наливаю…", "лайм, м'ята, парасолька…", "готово 🍸"],
    items: [
      { id: "mojito", name: "мохіто", icon: "🍹", price: 3, coin: "murkoin" },
      { id: "margarita", name: "маргарита", icon: "🍸", price: 3, coin: "murkoin" },
      { id: "cosmopolitan", name: "космополітан", icon: "🍷", price: 4, coin: "murkoin" },
      { id: "oldfashioned", name: "олд-фешн", icon: "🥃", price: 4, coin: "murkoin" },
      { id: "pinacolada", name: "піна-колада", icon: "🥥", price: 3, coin: "murkoin" },
      { id: "salotini", name: "салотіні", icon: "🥓", price: 2, coin: "hrukoin" },
    ],
  },
];

export function find(word: string) {
  const w = word.toLowerCase().trim();
  for (const machine of MACHINES) {
    const item = machine.items.find((i) => i.id === w || i.name === w);
    if (item) return { machine, item };
  }
  return null;
}
