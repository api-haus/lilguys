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
      { id: "matchalatte", name: "матча-лате", icon: "🍵", price: 2, coin: "murkoin" },
      { id: "hrukachino", name: "хрюкачино", icon: "🐽", price: 1, coin: "hrukoin" },
    ],
  },
  {
    id: "tea",
    name: "чайна",
    says: ["гріє воду…", "проливає листя…", "заварює…", "розливає по піалах…", "готово 🍵"],
    items: [
      { id: "puerh", name: "пуер", icon: "🍵", price: 2, coin: "murkoin" },
      { id: "gaba", name: "габа", icon: "🍵", price: 2, coin: "murkoin" },
      { id: "oolong", name: "улун", icon: "🍵", price: 2, coin: "murkoin" },
      { id: "jasmine", name: "жасминовий", icon: "🌼", price: 1, coin: "murkoin" },
      { id: "ceremonialmatcha", name: "церемоніальна матча", icon: "🍵", price: 3, coin: "murkoin" },
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
      { id: "negroni", name: "негроні", icon: "🍷", price: 4, coin: "murkoin" },
      { id: "daiquiri", name: "дайкірі", icon: "🍸", price: 3, coin: "murkoin" },
      { id: "manhattan", name: "манхеттен", icon: "🥃", price: 4, coin: "murkoin" },
      { id: "martini", name: "мартіні", icon: "🍸", price: 4, coin: "murkoin" },
      { id: "vodkamartini", name: "горілчаний мартіні", icon: "🍸", price: 4, coin: "murkoin" },
      { id: "whiskeysour", name: "віскі-сауер", icon: "🥃", price: 3, coin: "murkoin" },
      { id: "caipirinha", name: "кайпіріньйа", icon: "🍋", price: 3, coin: "murkoin" },
      { id: "mintjulep", name: "м'ятний джулеп", icon: "🌿", price: 3, coin: "murkoin" },
      { id: "moscowmule", name: "московський мул", icon: "🍺", price: 3, coin: "murkoin" },
      { id: "dark_n_stormy", name: "дарк-н-сторми", icon: "⛈️", price: 3, coin: "murkoin" },
      { id: "aperolspritz", name: "апероль-шприц", icon: "🍊", price: 3, coin: "murkoin" },
      { id: "americano_cocktail", name: "американо-коктейль", icon: "🍹", price: 3, coin: "murkoin" },
      { id: "bloodymary", name: "кривава мері", icon: "🍅", price: 3, coin: "murkoin" },
      { id: "whiterussian", name: "білий росіянин", icon: "🥛", price: 4, coin: "murkoin" },
      { id: "blackrussian", name: "чорний росіянин", icon: "☕", price: 4, coin: "murkoin" },
      { id: "espressomartini", name: "еспресо-мартіні", icon: "☕", price: 4, coin: "murkoin" },
      { id: "longisland", name: "лонг-айленд", icon: "🥤", price: 5, coin: "murkoin" },
      { id: "tequilasunrise", name: "текіла-санрайз", icon: "🌅", price: 3, coin: "murkoin" },
      { id: "sexonthebeach", name: "секс на пляжі", icon: "🏖️", price: 3, coin: "murkoin" },
      { id: "bluelagoon", name: "блакитна лагуна", icon: "🌊", price: 3, coin: "murkoin" },
      { id: "cubalibre", name: "куба лібре", icon: "🥤", price: 3, coin: "murkoin" },
      { id: "bramble", name: "брамбл", icon: "🫐", price: 4, coin: "murkoin" },
      { id: "bellini", name: "беліні", icon: "🍑", price: 3, coin: "murkoin" },
      { id: "mimosa", name: "мімоза", icon: "🥂", price: 3, coin: "murkoin" },
      { id: "kir", name: "кір-рояль", icon: "🥂", price: 3, coin: "murkoin" },
      { id: "sidecar", name: "сайдкар", icon: "🍸", price: 4, coin: "murkoin" },
      { id: "gimlet", name: "гімлет", icon: "🍸", price: 3, coin: "murkoin" },
      { id: "tomcollins", name: "том колінз", icon: "🥤", price: 3, coin: "murkoin" },
      { id: "singapuresling", name: "сінгапурський слінг", icon: "🍒", price: 4, coin: "murkoin" },
      { id: "zombie", name: "зомбі", icon: "🧟", price: 5, coin: "murkoin" },
      { id: "painkiller", name: "пейнкілер", icon: "🏝️", price: 4, coin: "murkoin" },
      { id: "hurricane", name: "ураган", icon: "🌀", price: 4, coin: "murkoin" },
      { id: "mai_tai", name: "май-тай", icon: "🌺", price: 4, coin: "murkoin" },
      { id: "amarettosour", name: "амаретто-сауер", icon: "🍋", price: 3, coin: "murkoin" },
      { id: "irishcoffee", name: "айриш-кофі", icon: "☕", price: 3, coin: "murkoin" },
      { id: "tropiki", name: "тропіки", icon: "🌴", price: 4, coin: "murkoin" },
      { id: "hanami", name: "ханамі", icon: "🌸", price: 4, coin: "murkoin" },
      { id: "morskoyezh", name: "морський їжак", icon: "🦔", price: 3, coin: "murkoin" },
      { id: "shotclover", name: "шот кловер клаб", icon: "🥃", price: 2, coin: "murkoin" },
      { id: "shotmango", name: "шот манго бум", icon: "🥭", price: 2, coin: "murkoin" },
      { id: "shotbumble", name: "шот бамбл", icon: "🐝", price: 2, coin: "murkoin" },
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
