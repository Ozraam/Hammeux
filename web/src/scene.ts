import { Container, Graphics } from "pixi.js";
import type { PersonDot } from "./protocol/PersonDot";
import type { Scene } from "./protocol/Scene";

/** Durée d'interpolation des déplacements (le serveur envoie une scène toutes les 250 ms). */
const LERP_MS = 260;

const ROOFS = [0xb5523b, 0x8e3b2f, 0x6b4c3b, 0x9c6b3e, 0x7a5a8a, 0x4f6d7a];
const SHIRTS = [0xd94f3d, 0x3d6fd9, 0xe0b13a, 0x4caf6b, 0x8e5fc2, 0xe07b39, 0x2fa7a7, 0xf2f2f2];
const CARRY = [0, 0x8a5a2b, 0x9e9e9e, 0xe6c35c, 0x6b4f3a];
/** `act` : index de `Act` côté Rust. */
const ACT_CHATTING = 13;

interface Moving {
  dot: PersonDot;
  fromX: number;
  fromY: number;
  t0: number;
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
  private moving = new Map<number, Moving>();
  private villageColor = new Map<number, number>();
  selected: number | null = null;

  constructor() {
    this.container.addChild(this.buildings, this.people);
  }

  clear() {
    this.buildings.clear();
    this.people.clear();
    this.moving.clear();
  }

  update(scene: Scene) {
    const now = performance.now();
    const next = new Map<number, Moving>();
    for (const dot of scene.people) {
      const prev = this.moving.get(dot.id);
      const p = prev ? this.position(prev, now) : { x: dot.x, y: dot.y };
      // Grand saut (arrivée, reprise) : pas d'animation.
      const jump = Math.hypot(p.x - dot.x, p.y - dot.y) > 6;
      next.set(dot.id, { dot, fromX: jump ? dot.x : p.x, fromY: jump ? dot.y : p.y, t0: now });
    }
    this.moving = next;
    for (const c of scene.civic) this.villageColor.set(c.village, c.color);
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
      g.rect(f.x + pad, f.y + pad, s - 2 * pad, s - 2 * pad).fill(c).stroke({ color: 0x6b4a2a, width: 0.08 });
      // Sillons.
      const furrow = f.state === 3 ? 0xc79a32 : f.state === 2 ? mix(0x7f9f45, 0xbf9a3a, f.growth) : 0x5e4127;
      for (let k = 1; k < 6; k++) {
        const yy = f.y + pad + ((s - 2 * pad) * k) / 6;
        g.moveTo(f.x + pad + 0.1, yy).lineTo(f.x + s - pad - 0.1, yy);
      }
      g.stroke({ color: furrow, width: 0.07 });
    }
    for (const c of scene.civic) {
      // Grenier 2 × 2 au toit de la couleur du village, puits à côté.
      g.rect(c.x + 0.1, c.y + 0.5, 1.8, 1.35).fill(0x8a5a33).stroke({ color: 0x4a2f1a, width: 0.08 });
      g.poly([c.x - 0.05, c.y + 0.6, c.x + 1, c.y - 0.1, c.x + 2.05, c.y + 0.6]).fill(c.color).stroke({ color: 0x3a2a1a, width: 0.08 });
      g.rect(c.x + 0.8, c.y + 1.2, 0.4, 0.65).fill(0x3a2615);
      g.circle(c.well_x + 0.5, c.well_y + 0.5, 0.36).fill(0x8f8a80).stroke({ color: 0x5a554c, width: 0.08 });
      g.circle(c.well_x + 0.5, c.well_y + 0.5, 0.2).fill(0x2f6fa3);
    }
    for (const h of scene.houses) {
      const { x, y } = h;
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

  /** Redessine les habitants (à chaque image). `scale` : pixels par case. */
  draw(scale: number) {
    const g = this.people.clear();
    const now = performance.now();
    const minPx = 2.6;
    for (const m of this.moving.values()) {
      const { dot } = m;
      const p = this.position(m, now);
      const r = Math.max(dot.stage === 0 ? 0.13 : 0.18, minPx / scale) * (dot.stage === 0 ? 0.8 : 1);
      const shirt = SHIRTS[dot.looks[0] % SHIRTS.length];
      if (dot.id === this.selected) {
        g.circle(p.x, p.y, r * 2.2).fill({ color: 0xffe066, alpha: 0.35 }).stroke({ color: 0xffe066, width: r * 0.4 });
      }
      g.circle(p.x, p.y, r).fill(shirt).stroke({ color: 0x1e1a14, width: r * 0.35 });
      if (dot.stage === 2) g.circle(p.x, p.y - r * 0.2, r * 0.45).fill(0xdddddd);
      if (dot.carry) {
        g.rect(p.x + r * 0.6, p.y - r * 1.4, r * 1.1, r * 1.1).fill(CARRY[dot.carry] ?? 0x888888).stroke({ color: 0x1e1a14, width: r * 0.2 });
      }
      if (dot.act === ACT_CHATTING) {
        g.roundRect(p.x - r * 0.3, p.y - r * 3.2, r * 1.9, r * 1.2, r * 0.3).fill({ color: 0xffffff, alpha: 0.9 });
      }
    }
  }
}

function mix(a: number, b: number, t: number): number {
  const ch = (c: number, s: number) => (c >> s) & 0xff;
  const m = (s: number) => Math.round(ch(a, s) + (ch(b, s) - ch(a, s)) * Math.max(0, Math.min(1, t))) << s;
  return m(16) | m(8) | m(0);
}
