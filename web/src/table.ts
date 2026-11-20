import { Container, Graphics, Sprite, Texture, TilingSprite } from "pixi.js";
import { hash } from "./worldmap";

/** Marge de papier autour de la carte, en cases. */
export const PAPER_MARGIN = 18;
const TABLE_COLOR = 0x5a3b24;

/**
 * Décor « carte posée sur une table » : plateau en bois, ombre portée, feuille de papier
 * avec un double filet autour de la carte. Tout est en coordonnées de cases, sous la carte.
 */
export function buildTable(mapW: number, mapH: number): Container {
  const decor = new Container();
  const M = PAPER_MARGIN;

  // Plateau : planches répétées, bien plus grand que tout ce qu'on peut voir.
  const wood = new TilingSprite({
    texture: Texture.from(woodCanvas()),
    width: mapW * 6,
    height: mapH * 6,
  });
  wood.position.set(-mapW * 2.5, -mapH * 2.5);
  wood.tileScale.set(1.8); // une planche (128 px) ≈ 230 cases : de larges planches de table
  decor.addChild(wood);

  // Ombre portée de la feuille (lumière venant du haut à gauche).
  const paperW = mapW + 2 * M, paperH = mapH + 2 * M;
  const pad = Math.max(paperW, paperH) * 0.06; // place pour le flou autour de la feuille
  const shadow = new Sprite(Texture.from(shadowCanvas(paperW, paperH, pad)));
  shadow.position.set(-M - pad + paperW * 0.012, -M - pad + paperH * 0.02);
  shadow.width = paperW + 2 * pad;
  shadow.height = paperH + 2 * pad;
  decor.addChild(shadow);

  // Feuille de papier et filets.
  const paper = new Graphics()
    .rect(-M, -M, mapW + 2 * M, mapH + 2 * M)
    .fill(0xefe2c4)
    .rect(-M * 0.55, -M * 0.55, mapW + M * 1.1, mapH + M * 1.1)
    .stroke({ color: 0x7a6247, width: 1.2, alpha: 0.8 })
    .rect(-1.5, -1.5, mapW + 3, mapH + 3)
    .stroke({ color: 0x4a3a2a, width: 2.2 });
  decor.addChild(paper);
  return decor;
}

export { TABLE_COLOR };

/** Bois : planches horizontales, veinures ondulées, joints décalés. */
function woodCanvas(): HTMLCanvasElement {
  const S = 512;
  const PLANK = 128;
  const c = document.createElement("canvas");
  c.width = c.height = S;
  const ctx = c.getContext("2d")!;
  const img = ctx.createImageData(S, S);
  for (let y = 0; y < S; y++) {
    const plank = Math.floor(y / PLANK);
    const tint = 0.9 + hash(plank, 7) * 0.2;
    const joint = Math.floor(hash(plank, 3) * S);
    for (let x = 0; x < S; x++) {
      const yy = y % PLANK;
      const wave = Math.sin(x * 0.012 + plank * 2.1) * 9 + Math.sin(x * 0.041 + plank) * 3;
      const grain = Math.sin((yy + wave) * 0.42 + plank * 5) * 0.5 + 0.5;
      const fine = hash(x >> 1, y) * 0.06;
      let k = tint * (0.82 + grain * 0.16 + fine);
      // Joints entre planches (haut et bas) et bout de planche.
      if (yy < 2 || yy > PLANK - 2) k *= 0.55;
      if (Math.abs(x - joint) < 2) k *= 0.6;
      const o = (y * S + x) * 4;
      img.data[o] = Math.min(255, 0x7a * k);
      img.data[o + 1] = Math.min(255, 0x50 * k);
      img.data[o + 2] = Math.min(255, 0x32 * k);
      img.data[o + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
  return c;
}

/** Ombre floue de la feuille, dessinée une fois en petit (mêmes proportions) puis étirée. */
function shadowCanvas(paperW: number, paperH: number, pad: number): HTMLCanvasElement {
  const k = 320 / (paperW + 2 * pad); // pixels de canevas par case
  const c = document.createElement("canvas");
  c.width = Math.round((paperW + 2 * pad) * k);
  c.height = Math.round((paperH + 2 * pad) * k);
  const ctx = c.getContext("2d")!;
  // Astuce compatible partout : le rectangle est dessiné hors du canevas, seule son ombre y tombe.
  ctx.shadowColor = "rgba(0, 0, 0, 0.85)";
  ctx.shadowBlur = pad * k * 0.6;
  ctx.shadowOffsetX = 10000;
  ctx.fillRect(pad * k - 10000, pad * k, paperW * k, paperH * k);
  return c;
}
