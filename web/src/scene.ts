import { Container, Graphics } from "pixi.js";
import type { PersonDot } from "./protocol/PersonDot";
import type { Scene } from "./protocol/Scene";

/** Durée d'interpolation des déplacements (le serveur envoie une scène toutes les 250 ms). */
const LERP_MS = 260;

const ROOFS = [0xb5523b, 0x8e3b2f, 0x6b4c3b, 0x9c6b3e, 0x7a5a8a, 0x4f6d7a];
const SHIRTS = [0xd94f3d, 0x3d6fd9, 0xe0b13a, 0x4caf6b, 0x8e5fc2, 0xe07b39, 0x2fa7a7, 0xf2f2f2];
/** Teint (`looks[1]`) et cheveux (`looks[2]`). */
const SKINS = [0xf3cfae, 0xdcaa80, 0xb87d52, 0x7d4f31];
const HAIRS = [0x2b1d14, 0x5a3a22, 0x9a6230, 0xd9b26a, 0xa63d1f];
const OUTLINE = 0x1e1a14;
/** Couleur de ce qui est porté : index du bien + 1 (voir `Good`), 255 = baluchon. */
/** Code `carry` d'une charrette de marchand. */
const CART = 254;

const CARRY: Record<number, number> = {
  1: 0xe6c35c, // grain
  2: 0xf2ead8, // farine
  3: 0xc98a3d, // pain
  4: 0x7fb3d5, // poisson
  5: 0xa8433a, // viande
  6: 0x8a5a2b, // bois
  7: 0x9e9e9e, // pierre
  8: 0x5b4a45, // minerai
  9: 0x6e7f8a, // outils
  255: 0x6b4f3a, // baluchon
};
/** `act` : index de `Act` côté Rust. */
const ACT_CHATTING = 13;
const ACT_FEASTING = 26;
const ACT_PRAYING = 28;
/** Effets du Ciel (`SpellKind` côté Rust). */
const SPELL = { rain: 0, drought: 1, storm: 2, abundance: 3, locusts: 4, bounty: 5 } as const;
/** Rayon (cases) des effets du Ciel autour du grenier. */
const SPELL_R = 16;
const FLAGS = [0xe0533d, 0xf2c94c, 0x3d8fd9, 0x4caf6b, 0xf2f2f2];

interface Moving {
  dot: PersonDot;
  fromX: number;
  fromY: number;
  t0: number;
  /** Regarde vers la droite (1) ou la gauche (-1). */
  face: number;
}

/**
 * Habitants et bâtiments de la zone regardée, dessinés en coordonnées de cases sous la
 * même caméra que la carte. Les bâtiments sont redessinés à chaque scène, les habitants
 * à chaque image (positions interpolées).
 */
export class SceneLayer {
  readonly container = new Container();
  private readonly buildings = new Graphics();
  private readonly people = new Graphics();
  /** Effets du Ciel, redessinés à chaque image par-dessus les habitants. */
  private readonly sky = new Graphics();
  private civic: Scene["civic"] = [];
  private moving = new Map<number, Moving>();
  private villageColor = new Map<number, number>();
  selected: number | null = null;

  constructor() {
    this.container.addChild(this.buildings, this.people, this.sky);
  }

  clear() {
    this.buildings.clear();
    this.people.clear();
    this.sky.clear();
    this.moving.clear();
    this.civic = [];
  }

  update(scene: Scene) {
    const now = performance.now();
    const next = new Map<number, Moving>();
    for (const dot of scene.people) {
      const prev = this.moving.get(dot.id);
      const p = prev ? this.position(prev, now) : { x: dot.x, y: dot.y };
      // Grand saut (arrivée, reprise) : pas d'animation.
      const jump = Math.hypot(p.x - dot.x, p.y - dot.y) > 6;
      const dx = dot.x - p.x;
      const face = Math.abs(dx) > 0.01 ? Math.sign(dx) : (prev?.face ?? (dot.id % 2 ? 1 : -1));
      next.set(dot.id, { dot, fromX: jump ? dot.x : p.x, fromY: jump ? dot.y : p.y, t0: now, face });
    }
    this.moving = next;
    for (const c of scene.civic) this.villageColor.set(c.village, c.color);
    this.civic = scene.civic;
    this.drawBuildings(scene);
  }

  private position(m: Moving, now: number) {
    const t = Math.min(1, (now - m.t0) / LERP_MS);
    return { x: m.fromX + (m.dot.x - m.fromX) * t, y: m.fromY + (m.dot.y - m.fromY) * t };
  }

  /** Position actuelle (interpolée) d'un habitant visible. */
  positionOf(id: number) {
    const m = this.moving.get(id);
    return m ? this.position(m, performance.now()) : null;
  }

  /** Habitant le plus proche d'un point (cases), dans un rayon donné. */
  pick(x: number, y: number, radius: number): number | null {
    const now = performance.now();
    let best: number | null = null;
    let bd = radius;
    for (const m of this.moving.values()) {
      const p = this.position(m, now);
      const d = Math.hypot(p.x - x, p.y - y);
      if (d < bd) {
        bd = d;
        best = m.dot.id;
      }
    }
    return best;
  }

  private drawBuildings(scene: Scene) {
    const g = this.buildings.clear();
    for (const f of scene.fields) {
      const s = f.size;
      const pad = 0.12;
      const colors = [0xa58455, 0x7a5634, 0x8a6a3e, 0xe0b84a];
      let c = colors[f.state];
      if (f.state === 2) c = mix(0x9fbf5a, 0xd9b44a, f.growth);
      // Une terre épuisée pâlit ; une terre bien fumée fonce.
      if (f.state < 2) c = f.soil < 1 ? mix(c, 0xc8b89a, Math.min(1, (1 - f.soil) * 1.4)) : mix(c, 0x4a3020, Math.min(0.5, (f.soil - 1) * 3));
      g.rect(f.x + pad, f.y + pad, s - 2 * pad, s - 2 * pad).fill(c).stroke({ color: 0x6b4a2a, width: 0.08 });
      // Sillons.
      const furrow = f.state === 3 ? 0xc79a32 : f.state === 2 ? mix(0x7f9f45, 0xbf9a3a, f.growth) : 0x5e4127;
      for (let k = 1; k < 6; k++) {
        const yy = f.y + pad + ((s - 2 * pad) * k) / 6;
        g.moveTo(f.x + pad + 0.1, yy).lineTo(f.x + s - pad - 0.1, yy);
      }
      g.stroke({ color: furrow, width: 0.07 });
      // Mauvaises herbes dans les champs qu'on ne sarcle pas.
      if (f.state === 2 && f.weeds > 0.15) {
        const n = Math.round(f.weeds * 10);
        for (let k = 0; k < n; k++) {
          const wx = f.x + pad + 0.2 + (((k * 7 + f.id * 3) % 11) / 11) * (s - 2 * pad - 0.4);
          const wy = f.y + pad + 0.2 + (((k * 5 + f.id) % 7) / 7) * (s - 2 * pad - 0.4);
          g.circle(wx, wy, 0.11).fill(0x4f7a2e);
        }
      }
    }
    for (const c of scene.civic) {
      // Grenier 2 × 2 au toit de la couleur du village, puits à côté.
      g.rect(c.x + 0.1, c.y + 0.5, 1.8, 1.35).fill(0x8a5a33).stroke({ color: 0x4a2f1a, width: 0.08 });
      g.poly([c.x - 0.05, c.y + 0.6, c.x + 1, c.y - 0.1, c.x + 2.05, c.y + 0.6]).fill(c.color).stroke({ color: 0x3a2a1a, width: 0.08 });
      g.rect(c.x + 0.8, c.y + 1.2, 0.4, 0.65).fill(0x3a2615);
      g.circle(c.well_x + 0.5, c.well_y + 0.5, 0.36).fill(0x8f8a80).stroke({ color: 0x5a554c, width: 0.08 });
      g.circle(c.well_x + 0.5, c.well_y + 0.5, 0.2).fill(0x2f6fa3);
      if (c.fair) {
        // Étals de la foire devant le grenier : auvents rayés.
        for (let k = 0; k < 3; k++) {
          const sx = c.x - 1.2 + k * 1.3, sy = c.y + 2.4;
          g.rect(sx, sy + 0.25, 0.9, 0.45).fill(0x9c6b3e).stroke({ color: 0x4a2f1a, width: 0.05 });
          for (let s = 0; s < 3; s++) g.rect(sx - 0.05 + s * 0.33, sy, 0.33, 0.28).fill(s % 2 ? 0xf2f2f2 : FLAGS[k]);
        }
      }
      if (c.feast) {
        // Guirlande de fanions du grenier au puits, et un feu de joie.
        const x0 = c.x + 2, y0 = c.y + 0.6, x1 = c.well_x + 0.5, y1 = c.well_y + 0.1;
        g.moveTo(x0, y0).quadraticCurveTo((x0 + x1) / 2, Math.max(y0, y1) + 0.5, x1, y1).stroke({ color: 0x3a2a1a, width: 0.04 });
        for (let k = 1; k < 6; k++) {
          const t = k / 6;
          const fx = x0 + (x1 - x0) * t;
          const fy = (1 - t) * (1 - t) * y0 + 2 * t * (1 - t) * (Math.max(y0, y1) + 0.5) + t * t * y1;
          g.poly([fx - 0.09, fy, fx + 0.09, fy, fx, fy + 0.2]).fill(FLAGS[k % FLAGS.length]);
        }
        const bx = c.well_x + 2.5, by = c.well_y + 1.5;
        g.circle(bx, by, 0.32).fill({ color: 0xffb347, alpha: 0.35 });
        g.poly([bx - 0.18, by + 0.15, bx, by - 0.3, bx + 0.18, by + 0.15]).fill(0xff7a2f);
        g.poly([bx - 0.08, by + 0.15, bx, by - 0.1, bx + 0.08, by + 0.15]).fill(0xffe066);
      }
    }
    for (const h of scene.houses) {
      const { x, y } = h;
      if (h.kind > 0 && h.state !== 0) {
        this.drawWorkshop(g, h.kind, x, y);
        continue;
      }
      const roof = ROOFS[h.roof % ROOFS.length];
      const door = this.villageColor.get(h.village) ?? 0x5a3a20;
      switch (h.state) {
        case 0: {
          // Chantier : fondations, puis charpente, murs et toit selon l'avancement.
          const p = h.progress;
          g.rect(x + 0.12, y + 0.3, 0.76, 0.62).fill(0xb0a89a).stroke({ color: 0x7d766a, width: 0.06 });
          if (p > 0.25) g.rect(x + 0.15, y + 0.92 - 0.6 * Math.min(1, (p - 0.25) / 0.5), 0.7, 0.6 * Math.min(1, (p - 0.25) / 0.5)).fill(0xe8d9b5);
          g.moveTo(x + 0.15, y + 0.92).lineTo(x + 0.15, y + 0.3).moveTo(x + 0.85, y + 0.92).lineTo(x + 0.85, y + 0.3);
          g.moveTo(x + 0.15, y + 0.3).lineTo(x + 0.5, y + 0.05).lineTo(x + 0.85, y + 0.3);
          g.stroke({ color: 0x8a5a33, width: 0.07 });
          if (p > 0.8) g.poly([x + 0.08, y + 0.36, x + 0.5, y + 0.05, x + 0.92, y + 0.36]).fill({ color: roof, alpha: (p - 0.8) / 0.2 });
          break;
        }
        case 1:
        case 2: {
          const dull = h.state === 2;
          g.rect(x + 0.15, y + 0.33, 0.7, 0.6).fill(dull ? 0xb9ad94 : 0xe8d9b5).stroke({ color: 0x5a4632, width: 0.06 });
          g.poly([x + 0.06, y + 0.38, x + 0.5, y + 0.04, x + 0.94, y + 0.38]).fill(dull ? 0x8a8378 : roof).stroke({ color: 0x3a2a1a, width: 0.06 });
          g.rect(x + 0.42, y + 0.62, 0.17, 0.31).fill(dull ? 0x6a5a48 : door);
          break;
        }
        case 3:
          g.rect(x + 0.15, y + 0.55, 0.3, 0.38).fill(0x9a917f);
          g.rect(x + 0.55, y + 0.7, 0.3, 0.23).fill(0x9a917f);
          g.rect(x + 0.15, y + 0.33, 0.7, 0.6).stroke({ color: 0x6f675a, width: 0.05 });
          break;
        default:
          g.circle(x + 0.4, y + 0.6, 0.32).fill(0x4f8a3c);
          g.circle(x + 0.65, y + 0.5, 0.25).fill(0x5d9a46);
          g.rect(x + 0.2, y + 0.75, 0.25, 0.15).fill(0x9a917f);
      }
    }
  }

  /** Moulin (ailes), four à pain (cheminée qui fume), forge (toit sombre, braises), temple. */
  private drawWorkshop(g: Graphics, kind: number, x: number, y: number) {
    switch (kind) {
      case 4: {
        // Temple : murs blancs, haut toit pointu, clocheton et sa lueur dorée.
        g.circle(x + 0.5, y + 0.2, 0.55).fill({ color: 0xffe08a, alpha: 0.18 });
        g.rect(x + 0.12, y + 0.42, 0.76, 0.51).fill(0xf2efe6).stroke({ color: 0x5a4632, width: 0.06 });
        g.poly([x + 0.04, y + 0.46, x + 0.5, y + 0.08, x + 0.96, y + 0.46]).fill(0x5b6f8a).stroke({ color: 0x2f3a4a, width: 0.06 });
        g.rect(x + 0.42, y - 0.2, 0.16, 0.3).fill(0xf2efe6).stroke({ color: 0x5a4632, width: 0.04 });
        g.poly([x + 0.38, y - 0.18, x + 0.5, y - 0.42, x + 0.62, y - 0.18]).fill(0x5b6f8a);
        g.circle(x + 0.5, y - 0.46, 0.05).fill(0xffd34d);
        g.roundRect(x + 0.4, y + 0.6, 0.2, 0.33, 0.1).fill(0x6b4a2a);
        g.circle(x + 0.5, y + 0.3, 0.07).fill(0xffd34d);
        break;
      }
      case 1: {
        g.rect(x + 0.3, y + 0.35, 0.4, 0.58).fill(0xe8d9b5).stroke({ color: 0x5a4632, width: 0.06 });
        g.poly([x + 0.25, y + 0.38, x + 0.5, y + 0.15, x + 0.75, y + 0.38]).fill(0x8e3b2f);
        const cx = x + 0.5, cy = y + 0.3;
        for (let k = 0; k < 4; k++) {
          const a = (k * Math.PI) / 2 + Math.PI / 4;
          g.moveTo(cx, cy).lineTo(cx + Math.cos(a) * 0.45, cy + Math.sin(a) * 0.45);
        }
        g.stroke({ color: 0x5a3a20, width: 0.08 });
        break;
      }
      case 2:
        g.rect(x + 0.12, y + 0.38, 0.76, 0.55).fill(0xd8c3a0).stroke({ color: 0x5a4632, width: 0.06 });
        g.poly([x + 0.06, y + 0.42, x + 0.5, y + 0.12, x + 0.94, y + 0.42]).fill(0x9c6b3e);
        g.rect(x + 0.66, y + 0.08, 0.14, 0.24).fill(0x6b4c3b);
        g.circle(x + 0.73, y - 0.02, 0.1).fill({ color: 0xdddddd, alpha: 0.7 });
        g.roundRect(x + 0.35, y + 0.6, 0.3, 0.25, 0.12).fill(0x3a2615);
        break;
      default:
        g.rect(x + 0.1, y + 0.4, 0.8, 0.53).fill(0x8a8378).stroke({ color: 0x3a3632, width: 0.06 });
        g.poly([x + 0.05, y + 0.44, x + 0.5, y + 0.14, x + 0.95, y + 0.44]).fill(0x4a4a4f);
        g.rect(x + 0.35, y + 0.62, 0.3, 0.31).fill(0x2a1a10);
        g.circle(x + 0.5, y + 0.82, 0.08).fill(0xff8a3d);
    }
  }

  /**
   * Effets du Ciel autour des greniers : pluie (traits qui tombent), orage (nuée sombre,
   * pluie drue, éclairs), sécheresse (chaleur orangée qui ondule), sauterelles (essaim qui
   * tourbillonne), abondance (poissons et feuilles qui scintillent), récolte miraculeuse
   * (paillettes dorées qui montent).
   */
  private drawSky(now: number, scale: number) {
    const g = this.sky.clear();
    const t = now / 1000;
    /** Un trait d'au moins un pixel et demi à l'écran. */
    const px = (w: number) => Math.max(w, 1.5 / scale);
    for (const c of this.civic) {
      if (!c.spells.length) continue;
      const cx = c.x + 1, cy = c.y + 1;
      const has = (k: number) => c.spells.includes(k);
      if (has(SPELL.drought)) {
        const p = 0.5 + 0.5 * Math.sin(t * 1.3);
        g.circle(cx, cy, SPELL_R).fill({ color: 0xff9a3c, alpha: 0.08 + 0.05 * p });
        for (let k = 0; k < 10; k++) {
          const a = (k / 10) * Math.PI * 2 + t * 0.2;
          const r = SPELL_R * (0.3 + 0.6 * rnd(c.village, k));
          const x = cx + Math.cos(a) * r, y = cy + Math.sin(a) * r;
          g.moveTo(x - 0.6, y).quadraticCurveTo(x, y - 0.4 - 0.3 * Math.sin(t * 3 + k), x + 0.6, y);
        }
        g.stroke({ color: 0xffd08a, width: px(0.08), alpha: 0.5 });
      }
      const storm = has(SPELL.storm);
      if (storm) {
        g.circle(cx, cy, SPELL_R * 1.1).fill({ color: 0x1a1f33, alpha: 0.32 });
        // Éclair de temps en temps.
        const flash = Math.floor(t * 0.7 + c.village);
        if (rnd(flash, 1) < 0.35 && (t * 0.7 + c.village) % 1 < 0.08) {
          g.circle(cx, cy, SPELL_R * 1.1).fill({ color: 0xe8f0ff, alpha: 0.35 });
          let x = cx + (rnd(flash, 2) - 0.5) * SPELL_R, y = cy - SPELL_R * 0.8;
          g.moveTo(x, y);
          for (let k = 0; k < 6; k++) {
            x += (rnd(flash, 3 + k) - 0.5) * 2.2;
            y += SPELL_R * 0.28;
            g.lineTo(x, y);
          }
          g.stroke({ color: 0xfaf7ff, width: px(0.25), alpha: 0.95 });
        }
      }
      if (has(SPELL.rain) || storm) {
        if (!storm) g.circle(cx, cy, SPELL_R).fill({ color: 0x6f86a8, alpha: 0.16 });
        const n = storm ? 260 : 150;
        const len = storm ? 1.4 : 0.9;
        for (let k = 0; k < n; k++) {
          const fx = rnd(c.village, k) * 2 - 1, fy = rnd(c.village, k + 500);
          const x = cx + fx * SPELL_R - (storm ? 0.8 : 0.3);
          const y = cy - SPELL_R + ((fy * 2 * SPELL_R + t * (storm ? 22 : 14)) % (2 * SPELL_R));
          if (Math.hypot(x - cx, y - cy) > SPELL_R) continue;
          g.moveTo(x, y).lineTo(x + (storm ? 0.5 : 0.18), y + len);
        }
        g.stroke({ color: 0xd6e6ff, width: px(0.07), alpha: storm ? 0.7 : 0.6 });
      }
      if (has(SPELL.locusts)) {
        for (let k = 0; k < 220; k++) {
          const a = rnd(c.village, k) * Math.PI * 2 + t * (0.6 + rnd(c.village, k + 9) * 0.8);
          const r = SPELL_R * (0.15 + 0.85 * rnd(c.village, k + 77));
          const x = cx + Math.cos(a) * r + Math.sin(t * 5 + k) * 0.3;
          const y = cy + Math.sin(a) * r * 0.7 + Math.cos(t * 4 + k) * 0.3;
          g.circle(x, y, px(0.09)).fill({ color: 0x3b3a1c, alpha: 0.85 });
        }
      }
      if (has(SPELL.abundance)) {
        for (let k = 0; k < 18; k++) {
          const x = cx + (rnd(c.village, k + 200) * 2 - 1) * SPELL_R;
          const y = cy + (rnd(c.village, k + 300) * 2 - 1) * SPELL_R;
          const a = 0.5 + 0.5 * Math.sin(t * 2 + k);
          if (k % 2) g.ellipse(x, y, 0.35, 0.15).fill({ color: 0x8fd3ff, alpha: 0.5 * a });
          else g.poly([x, y - 0.3, x + 0.2, y, x, y + 0.3, x - 0.2, y]).fill({ color: 0x8be08b, alpha: 0.5 * a });
        }
      }
      if (has(SPELL.bounty)) {
        for (let k = 0; k < 60; k++) {
          const x = cx + (rnd(c.village, k + 600) * 2 - 1) * SPELL_R;
          const y = cy + SPELL_R - ((rnd(c.village, k + 700) * 2 * SPELL_R + t * 2.5) % (2 * SPELL_R));
          if (Math.hypot(x - cx, y - cy) > SPELL_R) continue;
          g.circle(x, y, px(0.12) + 0.06 * Math.sin(t * 4 + k)).fill({ color: 0xffe066, alpha: 0.75 });
        }
      }
    }
  }

  /** Redessine les habitants (à chaque image). `scale` : pixels par case. */
  draw(scale: number) {
    const g = this.people.clear();
    const now = performance.now();
    this.drawSky(now, scale);
    // Du plus haut au plus bas de l'écran : ceux de devant passent par-dessus.
    const list = [...this.moving.values()].map((m) => ({ m, p: this.position(m, now) })).sort((a, b) => a.p.y - b.p.y);
    for (const { m, p } of list) this.figure(g, m, p.x, p.y, scale, now);
  }

  /**
   * Un habitant en silhouette : ombre au sol, jambes (qui bougent quand il marche), tunique
   * ou robe de la couleur de son vêtement, tête et cheveux. Les pieds sont un peu sous sa
   * position, pour que la silhouette se tienne sur sa case.
   */
  private figure(g: Graphics, m: Moving, x: number, y: number, scale: number, now: number) {
    const { dot, face } = m;
    const child = dot.stage === 0;
    const elder = dot.stage === 2;
    // Hauteur : une demi-case environ, jamais moins de quelques pixels à l'écran.
    const h = Math.max(child ? 0.5 : 0.7, (child ? 9 : 12) / scale);
    const u = h / 10;
    const feet = y + h * 0.42;
    const top = feet - h;
    const walking = now - m.t0 < LERP_MS && Math.hypot(dot.x - m.fromX, dot.y - m.fromY) > 0.02;
    const swing = walking ? Math.sin(now / 90 + dot.id) * 1.1 * u : 0;
    const line = Math.max(0.3 * u, 0.7 / scale);
    const shirt = SHIRTS[dot.looks[0] % SHIRTS.length];
    const skin = dot.sick ? mix(SKINS[dot.looks[1] % SKINS.length], 0x9acd5a, 0.55) : SKINS[dot.looks[1] % SKINS.length];
    const hair = elder ? 0xd8d8d2 : HAIRS[dot.looks[2] % HAIRS.length];

    if (dot.id === this.selected) {
      g.ellipse(x, feet, 4 * u, 1.6 * u).fill({ color: 0xffe066, alpha: 0.35 }).stroke({ color: 0xffe066, width: line * 1.5 });
    }
    g.ellipse(x, feet, 2.3 * u, 0.8 * u).fill({ color: 0x000000, alpha: 0.28 });

    if (dot.carry === CART) {
      // Charrette derrière lui : caisse brune et deux roues.
      const cx = x - face * 3.6 * u;
      g.rect(cx - 2.2 * u, feet - 4 * u, 4.4 * u, 2.6 * u).fill(0x8a5a2b).stroke({ color: OUTLINE, width: line });
      g.moveTo(cx + face * 2.2 * u, feet - 3 * u).lineTo(x - face * 0.6 * u, feet - 4.2 * u).stroke({ color: 0x5a3a20, width: line });
      g.circle(cx - 1.2 * u, feet - 1 * u, 1 * u).fill(0x3a2a1a);
      g.circle(cx + 1.2 * u, feet - 1 * u, 1 * u).fill(0x3a2a1a);
    }

    // Jambes.
    const hip = feet - 3.4 * u;
    g.moveTo(x - 0.6 * u, hip).lineTo(x - 0.6 * u + swing, feet);
    g.moveTo(x + 0.6 * u, hip).lineTo(x + 0.6 * u - swing, feet);
    g.stroke({ color: 0x3a2a1a, width: 1.1 * u, cap: "round" });

    // Cheveux longs (derrière la tête).
    const headY = top + 1.8 * u;
    if (dot.female && !child) g.roundRect(x - 1.8 * u, headY - 0.6 * u, 3.6 * u, 3.4 * u, 1.2 * u).fill(hair);

    // Corps : robe pour les femmes, tunique et ceinture pour les hommes.
    const shoulder = top + 3.4 * u;
    if (dot.female) {
      const hem = feet - (child ? 1.4 : 1.6) * u;
      g.poly([x - 1.4 * u, shoulder, x + 1.4 * u, shoulder, x + 2.3 * u, hem, x - 2.3 * u, hem]).fill(shirt).stroke({ color: OUTLINE, width: line });
    } else {
      g.poly([x - 1.6 * u, shoulder, x + 1.6 * u, shoulder, x + 1.7 * u, hip + 0.6 * u, x - 1.7 * u, hip + 0.6 * u]).fill(shirt).stroke({ color: OUTLINE, width: line });
      g.rect(x - 1.7 * u, hip - 0.4 * u, 3.4 * u, 0.6 * u).fill(0x4a3220);
    }
    // Bras qui balancent.
    g.moveTo(x - 1.5 * u, shoulder + 0.4 * u).lineTo(x - 1.9 * u - swing * 0.6, shoulder + 3 * u);
    g.moveTo(x + 1.5 * u, shoulder + 0.4 * u).lineTo(x + 1.9 * u + swing * 0.6, shoulder + 3 * u);
    g.stroke({ color: mix(shirt, 0x000000, 0.25), width: 0.9 * u, cap: "round" });

    // Tête : cheveux, puis le visage un peu plus bas (frange), un œil du côté où il regarde.
    g.circle(x, headY - 0.2 * u, 1.75 * u).fill(hair);
    g.circle(x, headY + 0.25 * u, 1.5 * u).fill(skin).stroke({ color: OUTLINE, width: line * 0.8 });
    if (scale * u > 1.2) g.circle(x + face * 0.7 * u, headY + 0.2 * u, 0.28 * u).fill(OUTLINE);

    // Canne des aînés.
    if (elder) {
      g.moveTo(x + face * 2.4 * u, shoulder + 2 * u).lineTo(x + face * 2.8 * u, feet).stroke({ color: 0x6b4a2a, width: 0.6 * u, cap: "round" });
    }

    // Ce qu'il porte, tenu devant lui.
    if (dot.carry && dot.carry !== CART) {
      const bx = x + face * 1.9 * u - 1.2 * u;
      g.roundRect(bx, shoulder + 0.6 * u, 2.4 * u, 2.2 * u, 0.4 * u).fill(CARRY[dot.carry] ?? 0x888888).stroke({ color: OUTLINE, width: line * 0.8 });
    }

    if (dot.act === ACT_CHATTING) {
      // Bulle de dialogue.
      const bx = x + face * 1.6 * u, by = top - 3 * u;
      g.roundRect(bx - 1.6 * u, by, 3.2 * u, 2.2 * u, 0.8 * u).fill({ color: 0xffffff, alpha: 0.92 });
      g.poly([bx - face * 0.6 * u, by + 2.1 * u, bx - face * 1.4 * u, by + 3 * u, bx, by + 2.1 * u]).fill({ color: 0xffffff, alpha: 0.92 });
      for (let k = -1; k <= 1; k++) g.circle(bx + k * 0.8 * u, by + 1.1 * u, 0.25 * u).fill(0x555555);
    } else if (dot.act === ACT_PRAYING) {
      // Prière : un halo doré qui pulse au-dessus de la tête.
      const pulse = 0.6 + 0.4 * Math.sin(now / 400 + dot.id);
      g.ellipse(x, top - 1.1 * u, 2 * u, 0.7 * u).stroke({ color: 0xffe08a, width: 0.5 * u, alpha: 0.5 + 0.4 * pulse });
      g.circle(x, top - 1.1 * u, 2.6 * u).fill({ color: 0xffe08a, alpha: 0.12 * pulse });
    } else if (dot.act === ACT_FEASTING) {
      // Danse : petits confettis qui tournent au-dessus de la tête.
      const a = now / 300 + dot.id;
      for (let k = 0; k < 3; k++) {
        const t = a + (k * 2 * Math.PI) / 3;
        g.circle(x + Math.cos(t) * 2.4 * u, top - 1.2 * u + Math.sin(t) * 0.8 * u, 0.5 * u).fill(FLAGS[(dot.id + k) % FLAGS.length]);
      }
    }
  }
}

/** Pseudo-hasard stable : `k`-ième nombre (0–1) d'une graine. */
function rnd(seed: number, k: number): number {
  const s = Math.sin(seed * 12.9898 + k * 78.233) * 43758.5453;
  return s - Math.floor(s);
}

function mix(a: number, b: number, t: number): number {
  const ch = (c: number, s: number) => (c >> s) & 0xff;
  const m = (s: number) => Math.round(ch(a, s) + (ch(b, s) - ch(a, s)) * Math.max(0, Math.min(1, t))) << s;
  return m(16) | m(8) | m(0);
}
