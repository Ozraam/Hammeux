// Couleurs de la carte. Les milieux suivent l'ordre de `Biome` côté Rust
// (et les mêmes valeurs que `BIOME_COLORS` dans hameaux-cli).

export const BIOME_COLORS = [
  0x1f4e79, // mer profonde
  0x2f6fa3, // mer
  0x3f88c5, // lac
  0xe9d8a6, // plage
  0xb5c46a, // prairie sèche
  0x8fbf5a, // prairie
  0x4f8a3c, // forêt
  0x356b45, // conifères
  0x6b8f71, // marais
  0xa79c6d, // lande
  0x9a8f62, // collines
  0x8a8178, // montagne
  0xf2f4f7, // neiges
];

export const RIVER = 0x3f88c5;
export const ROAD = 0xc8a46a;
export const DEEP_SEA = BIOME_COLORS[0];

export const enum B {
  DeepSea, Sea, Lake, Beach, Grassland, Meadow, Forest, Conifers, Marsh, Heath, Hills, Mountain, Snow,
}

export const enum R {
  None, Stone, Iron, Clay, Fish, Game, Berries,
}

export function isWater(b: number): boolean {
  return b === B.DeepSea || b === B.Sea || b === B.Lake;
}

/** Multiplie une couleur par un facteur (ombrage), avec saturation à 255. */
export function shadeColor(c: number, k: number): [number, number, number] {
  return [
    Math.min(255, ((c >> 16) & 0xff) * k),
    Math.min(255, ((c >> 8) & 0xff) * k),
    Math.min(255, (c & 0xff) * k),
  ];
}

export function css(c: number): string {
  return "#" + c.toString(16).padStart(6, "0");
}
