import { Texture } from "pixi.js";
import type { MapInfo } from "./protocol/MapInfo";
import { B, BIOME_COLORS, R, RIVER, ROAD, isWater, shadeColor } from "./palette";

/** Pixels par case dans les textures détaillées des tronçons. */
export const DETAIL_PX = 8;

/** Carte reçue du serveur : couches complètes en mémoire + texture d'ensemble (1 px par case). */
export class WorldMap {
  readonly info: MapInfo;
  readonly w: number;
  readonly h: number;
  readonly layers: Record<string, Uint8Array> = {};
  readonly overview: HTMLCanvasElement;
  readonly overviewTexture: Texture;
  private readonly overviewCtx: CanvasRenderingContext2D;
  private loaded: boolean[];
  loadedCount = 0;
  private dirty = false;

  constructor(info: MapInfo) {
    this.info = info;
    this.w = info.width;
    this.h = info.height;
    for (const name of info.chunk_layers) this.layers[name] = new Uint8Array(this.w * this.h);
    this.loaded = new Array(info.chunks_x * info.chunks_y).fill(false);
    this.overview = document.createElement("canvas");
    this.overview.width = this.w;
    this.overview.height = this.h;
    this.overviewCtx = this.overview.getContext("2d")!;
    this.overviewCtx.fillStyle = "#1f4e79";
    this.overviewCtx.fillRect(0, 0, this.w, this.h);
    this.overviewTexture = Texture.from(this.overview);
  }

  get chunkCount() {
    return this.loaded.length;
  }

  isLoaded(cx: number, cy: number) {
    return this.loaded[cy * this.info.chunks_x + cx];
  }

  /** Télécharge tous les tronçons (quelques requêtes en parallèle). */
  async loadAll(onProgress: () => void) {
    const queue: [number, number][] = [];
    for (let cy = 0; cy < this.info.chunks_y; cy++)
      for (let cx = 0; cx < this.info.chunks_x; cx++) queue.push([cx, cy]);
    const worker = async () => {
      for (let job = queue.shift(); job; job = queue.shift()) {
        const [cx, cy] = job;
        for (let attempt = 0; ; attempt++) {
          try {
            const res = await fetch(`/api/map/chunk/${cx}/${cy}`);
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            this.store(cx, cy, new Uint8Array(await res.arrayBuffer()));
            break;
          } catch (e) {
            if (attempt >= 4) throw e;
            await new Promise((r) => setTimeout(r, 500 * (attempt + 1)));
          }
        }
        onProgress();
      }
    };
    await Promise.all(Array.from({ length: 8 }, worker));
  }

  /** Recharge un tronçon modifié (route tracée, forêt coupée…). */
  async reload(cx: number, cy: number) {
    const res = await fetch(`/api/map/chunk/${cx}/${cy}`);
    if (!res.ok) return;
    this.loadedCount--;
    this.store(cx, cy, new Uint8Array(await res.arrayBuffer()));
  }

  private store(cx: number, cy: number, data: Uint8Array) {
    const s = this.info.chunk_size;
    const area = s * s;
    this.info.chunk_layers.forEach((name, l) => {
      const layer = this.layers[name];
      for (let dy = 0; dy < s; dy++) {
        const y = cy * s + dy;
        if (y >= this.h) break;
        const x0 = cx * s;
        const n = Math.min(s, this.w - x0);
        layer.set(data.subarray(l * area + dy * s, l * area + dy * s + n), y * this.w + x0);
      }
    });
    this.loaded[cy * this.info.chunks_x + cx] = true;
    this.loadedCount++;
    this.paintOverview(cx, cy);
  }

  private paintOverview(cx: number, cy: number) {
    const s = this.info.chunk_size;
    const x0 = cx * s, y0 = cy * s;
    const w = Math.min(s, this.w - x0), h = Math.min(s, this.h - y0);
    const img = this.overviewCtx.createImageData(w, h);
    for (let dy = 0; dy < h; dy++) {
      for (let dx = 0; dx < w; dx++) {
        const [r, g, b] = this.tileColor(x0 + dx, y0 + dy);
        const o = (dy * w + dx) * 4;
        img.data[o] = r;
        img.data[o + 1] = g;
        img.data[o + 2] = b;
        img.data[o + 3] = 255;
      }
    }
    this.overviewCtx.putImageData(img, x0, y0);
    this.dirty = true;
  }

  /** À appeler une fois par image : envoie la texture d'ensemble au GPU si elle a changé. */
  flush() {
    if (this.dirty) {
      this.overviewTexture.source.update();
      this.dirty = false;
    }
  }

  tileColor(x: number, y: number): [number, number, number] {
    const i = y * this.w + x;
    const biome = this.layers.biome[i];
    const flags = this.layers.flags[i];
    const river = flags & 3;
    const c = flags & 4 ? ROAD : river && !isWater(biome) ? RIVER : BIOME_COLORS[biome] ?? 0xff00ff;
    return shadeColor(c, this.layers.shade[i] / 128);
  }

  inside(x: number, y: number) {
    return x >= 0 && y >= 0 && x < this.w && y < this.h;
  }

  /** Dessine un tronçon en détail (DETAIL_PX par case) : textures, arbres, rivières, ressources. */
  renderChunk(cx: number, cy: number): HTMLCanvasElement {
    const s = this.info.chunk_size;
    const P = DETAIL_PX;
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = s * P;
    const ctx = canvas.getContext("2d")!;
    const img = ctx.createImageData(s * P, s * P);
    const x0 = cx * s, y0 = cy * s;
    const L = this.layers;

    // Fond : couleur ombrée + léger grain par pixel.
    for (let ty = 0; ty < s; ty++) {
      for (let tx = 0; tx < s; tx++) {
        const x = x0 + tx, y = y0 + ty;
        if (!this.inside(x, y)) continue;
        const i = y * this.w + x;
        const biome = L.biome[i];
        const base = BIOME_COLORS[biome] ?? 0xff00ff;
        const k = L.shade[i] / 128;
        for (let py = 0; py < P; py++) {
          for (let px = 0; px < P; px++) {
            const n = hash(x * P + px, y * P + py);
            const grain = isWater(biome) ? 1 + (n - 0.5) * 0.04 : 1 + (n - 0.5) * 0.1;
            const [r, g, b] = shadeColor(base, k * grain);
            const o = ((ty * P + py) * s * P + tx * P + px) * 4;
            img.data[o] = r;
            img.data[o + 1] = g;
            img.data[o + 2] = b;
            img.data[o + 3] = 255;
          }
        }
      }
    }
    ctx.putImageData(img, 0, 0);

    // Chemins : terre battue, reliée aux cases de chemin voisines.
    ctx.strokeStyle = "#c8a46a";
    ctx.lineCap = "round";
    ctx.lineWidth = P * 0.55;
    for (let ty = -1; ty <= s; ty++) {
      for (let tx = -1; tx <= s; tx++) {
        const x = x0 + tx, y = y0 + ty;
        if (!this.inside(x, y) || !(L.flags[y * this.w + x] & 4)) continue;
        const cxp = (tx + 0.5) * P, cyp = (ty + 0.5) * P;
        ctx.beginPath();
        ctx.moveTo(cxp, cyp);
        ctx.lineTo(cxp + 0.01, cyp);
        ctx.stroke();
        for (const [dx, dy] of [[1, 0], [0, 1], [1, 1], [-1, 1]]) {
          const nx = x + dx, ny = y + dy;
          if (!this.inside(nx, ny) || !(L.flags[ny * this.w + nx] & 4)) continue;
          ctx.beginPath();
          ctx.moveTo(cxp, cyp);
          ctx.lineTo(cxp + dx * P, cyp + dy * P);
          ctx.stroke();
        }
      }
    }

    // Rivières : traits entre cases voisines, épaisseur selon la taille.
    ctx.strokeStyle = ctx.fillStyle = "#3f88c5";
    ctx.lineCap = "round";
    for (let ty = -1; ty <= s; ty++) {
      for (let tx = -1; tx <= s; tx++) {
        const x = x0 + tx, y = y0 + ty;
        if (!this.inside(x, y)) continue;
        const i = y * this.w + x;
        const size = L.flags[i] & 3;
        if (!size || isWater(L.biome[i])) continue;
        const cxp = (tx + 0.5) * P, cyp = (ty + 0.5) * P;
        ctx.lineWidth = size * 1.6 + 1;
        for (const [dx, dy] of [[1, 0], [0, 1], [1, 1], [-1, 1]]) {
          const nx = x + dx, ny = y + dy;
          if (!this.inside(nx, ny)) continue;
          const j = ny * this.w + nx;
          if ((L.flags[j] & 3) || isWater(L.biome[j])) {
            ctx.beginPath();
            ctx.moveTo(cxp, cyp);
            ctx.lineTo(cxp + dx * P, cyp + dy * P);
            ctx.stroke();
          }
        }
      }
    }

    // Ponts : là où un chemin traverse une rivière.
    ctx.fillStyle = "#8a5a33";
    for (let ty = 0; ty < s; ty++) {
      for (let tx = 0; tx < s; tx++) {
        const x = x0 + tx, y = y0 + ty;
        if (!this.inside(x, y)) continue;
        const f = L.flags[y * this.w + x];
        if (f & 4 && f & 3) ctx.fillRect(tx * P + 1, ty * P + 1, P - 2, P - 2);
      }
    }

    // Arbres, roseaux, ressources.
    for (let ty = 0; ty < s; ty++) {
      for (let tx = 0; tx < s; tx++) {
        const x = x0 + tx, y = y0 + ty;
        if (!this.inside(x, y)) continue;
        const i = y * this.w + x;
        const biome = L.biome[i];
        const k = L.shade[i] / 128;
        const n = hash(x, y);
        const px = tx * P, py = ty * P;
        if (L.flags[i] & 7) continue;
        if (biome === B.Forest && n < 0.8) {
          const r = 2.2 + n * 1.3;
          fill(ctx, 0x2f6a2a, k);
          ctx.beginPath();
          ctx.arc(px + 2 + n * 4, py + 2 + hash(y, x) * 4, r, 0, Math.PI * 2);
          ctx.fill();
        } else if (biome === B.Conifers && n < 0.85) {
          fill(ctx, 0x1f4d33, k);
          const bx = px + 1 + n * 3, by = py + 1 + hash(y, x) * 2;
          ctx.beginPath();
          ctx.moveTo(bx + 2.5, by);
          ctx.lineTo(bx + 5, by + 6);
          ctx.lineTo(bx, by + 6);
          ctx.fill();
        } else if (biome === B.Marsh && n < 0.5) {
          ctx.strokeStyle = "#4b6b52";
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(px + 2, py + 6);
          ctx.lineTo(px + 3, py + 2);
          ctx.moveTo(px + 5, py + 6);
          ctx.lineTo(px + 5, py + 3);
          ctx.stroke();
        }
        drawResource(ctx, L.resource[i], px, py, n);
      }
    }
    return canvas;
  }
}

function fill(ctx: CanvasRenderingContext2D, c: number, k: number) {
  const [r, g, b] = shadeColor(c, k);
  ctx.fillStyle = `rgb(${r | 0},${g | 0},${b | 0})`;
}

function drawResource(ctx: CanvasRenderingContext2D, r: number, px: number, py: number, n: number) {
  if (r === R.None) return;
  const P = DETAIL_PX;
  const cx = px + P / 2, cy = py + P / 2;
  switch (r) {
    case R.Stone:
      ctx.fillStyle = "#d6d2c8";
      ctx.fillRect(cx - 2, cy - 1, 4, 3);
      break;
    case R.Iron:
      ctx.fillStyle = "#a0472c";
      ctx.fillRect(cx - 2, cy - 2, 3, 3);
      ctx.fillStyle = "#5b2a1c";
      ctx.fillRect(cx, cy, 2, 2);
      break;
    case R.Clay:
      ctx.fillStyle = "#c9773e";
      ctx.beginPath();
      ctx.ellipse(cx, cy, 3, 2, 0, 0, Math.PI * 2);
      ctx.fill();
      break;
    case R.Fish:
      if (n > 0.6) break;
      ctx.strokeStyle = "rgba(255,255,255,0.7)";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.arc(cx, cy + 2, 2.5, Math.PI * 1.15, Math.PI * 1.85);
      ctx.stroke();
      break;
    case R.Game:
      ctx.fillStyle = "#8a5a2b";
      ctx.beginPath();
      ctx.arc(cx, cy, 1.6, 0, Math.PI * 2);
      ctx.fill();
      break;
    case R.Berries:
      ctx.fillStyle = "#c2185b";
      ctx.beginPath();
      ctx.arc(cx - 1.5, cy, 1.2, 0, Math.PI * 2);
      ctx.arc(cx + 1.5, cy + 1, 1.2, 0, Math.PI * 2);
      ctx.fill();
      break;
  }
}

/** Bruit déterministe 0–1 par coordonnée (même rendu à chaque fois). */
export function hash(x: number, y: number): number {
  let h = (x * 374761393 + y * 668265263) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}
