import type { Container } from "pixi.js";

/**
 * Caméra 2D sur un conteneur : glisser pour se déplacer, molette ou pincement pour zoomer,
 * flèches et +/− au clavier. `scale` = pixels écran par case.
 */
export class Camera {
  scale = 1;
  x = 0; // position écran du coin haut-gauche de la carte
  y = 0;
  minScale = 0.2;
  maxScale = 32;
  private target: { x: number; y: number; scale: number } | null = null;
  private pointers = new Map<number, { x: number; y: number }>();
  private pinchDist = 0;
  /** Vrai si le dernier appui a bougé (ce n'était donc pas un clic). */
  dragged = false;

  constructor(
    el: HTMLElement,
    private readonly world: Container,
    private readonly mapW: number,
    private readonly mapH: number,
  ) {
    el.addEventListener("pointerdown", (e) => {
      el.setPointerCapture(e.pointerId);
      this.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      this.dragged = false;
      this.target = null;
      if (this.pointers.size === 2) this.pinchDist = this.pinchSpan();
    });
    el.addEventListener("pointermove", (e) => {
      const prev = this.pointers.get(e.pointerId);
      if (!prev) return;
      const next = { x: e.clientX, y: e.clientY };
      if (this.pointers.size === 1) {
        if (Math.abs(next.x - prev.x) + Math.abs(next.y - prev.y) > 0) this.dragged = true;
        this.x += next.x - prev.x;
        this.y += next.y - prev.y;
      }
      this.pointers.set(e.pointerId, next);
      if (this.pointers.size === 2) {
        const span = this.pinchSpan();
        const [a, b] = [...this.pointers.values()];
        this.zoomAt((a.x + b.x) / 2, (a.y + b.y) / 2, span / (this.pinchDist || span));
        this.pinchDist = span;
        this.dragged = true;
      }
      this.clamp();
    });
    const release = (e: PointerEvent) => this.pointers.delete(e.pointerId);
    el.addEventListener("pointerup", release);
    el.addEventListener("pointercancel", release);
    el.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        this.target = null;
        this.zoomAt(e.clientX, e.clientY, Math.exp(-e.deltaY * 0.0015));
      },
      { passive: false },
    );
    window.addEventListener("keydown", (e) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
      const step = 80;
      const cx = window.innerWidth / 2, cy = window.innerHeight / 2;
      switch (e.key) {
        case "ArrowLeft": this.x += step; break;
        case "ArrowRight": this.x -= step; break;
        case "ArrowUp": this.y += step; break;
        case "ArrowDown": this.y -= step; break;
        case "+": case "=": this.zoomAt(cx, cy, 1.4); break;
        case "-": this.zoomAt(cx, cy, 1 / 1.4); break;
        default: return;
      }
      this.target = null;
      this.clamp();
    });
  }

  private pinchSpan() {
    const [a, b] = [...this.pointers.values()];
    return Math.hypot(a.x - b.x, a.y - b.y);
  }

  /** Échelle qui fait tenir toute la carte à l'écran. */
  fitScale() {
    // Un peu de marge : on voit la feuille entière posée sur la table.
    return Math.min(window.innerWidth / this.mapW, window.innerHeight / this.mapH) * 0.86;
  }

  fit() {
    this.minScale = this.fitScale() * 0.5;
    this.scale = this.fitScale();
    this.x = (window.innerWidth - this.mapW * this.scale) / 2;
    this.y = (window.innerHeight - this.mapH * this.scale) / 2;
  }

  zoomAt(sx: number, sy: number, factor: number) {
    const s = Math.min(this.maxScale, Math.max(this.minScale, this.scale * factor));
    const k = s / this.scale;
    this.x = sx - (sx - this.x) * k;
    this.y = sy - (sy - this.y) * k;
    this.scale = s;
    this.clamp();
  }

  /** Glisse en douceur vers la case (tx, ty) à l'échelle donnée. */
  flyTo(tx: number, ty: number, scale: number) {
    this.target = {
      scale,
      x: window.innerWidth / 2 - (tx + 0.5) * scale,
      y: window.innerHeight / 2 - (ty + 0.5) * scale,
    };
  }

  /** Garde une case au centre de l'écran (suivi d'un habitant), en douceur. */
  centerOn(tx: number, ty: number) {
    if (this.target) return;
    const gx = window.innerWidth / 2 - tx * this.scale;
    const gy = window.innerHeight / 2 - ty * this.scale;
    this.x += (gx - this.x) * 0.15;
    this.y += (gy - this.y) * 0.15;
  }

  /** Case sous un point de l'écran. */
  toTile(sx: number, sy: number) {
    return { x: Math.floor((sx - this.x) / this.scale), y: Math.floor((sy - this.y) / this.scale) };
  }

  toScreen(tx: number, ty: number) {
    return { x: this.x + tx * this.scale, y: this.y + ty * this.scale };
  }

  /** Garde au moins un bout de carte à l'écran. */
  private clamp() {
    const margin = 120;
    const w = this.mapW * this.scale, h = this.mapH * this.scale;
    this.x = Math.min(window.innerWidth - margin, Math.max(margin - w, this.x));
    this.y = Math.min(window.innerHeight - margin, Math.max(margin - h, this.y));
  }

  /** À chaque image : animation éventuelle, puis application au conteneur. */
  update(dt: number) {
    if (this.target) {
      const t = 1 - Math.exp(-dt * 6);
      // Interpolation en échelle logarithmique, position recalculée pour garder le cap.
      const s = Math.exp(Math.log(this.scale) + (Math.log(this.target.scale) - Math.log(this.scale)) * t);
      const cx = window.innerWidth / 2, cy = window.innerHeight / 2;
      const goalTile = { x: (cx - this.target.x) / this.target.scale, y: (cy - this.target.y) / this.target.scale };
      const curTile = { x: (cx - this.x) / this.scale, y: (cy - this.y) / this.scale };
      const tile = { x: curTile.x + (goalTile.x - curTile.x) * t, y: curTile.y + (goalTile.y - curTile.y) * t };
      this.scale = s;
      this.x = cx - tile.x * s;
      this.y = cy - tile.y * s;
      if (Math.abs(Math.log(s / this.target.scale)) < 0.002 && Math.hypot(goalTile.x - tile.x, goalTile.y - tile.y) < 0.05) {
        this.target = null;
      }
    }
    this.world.position.set(this.x, this.y);
    this.world.scale.set(this.scale);
  }
}
