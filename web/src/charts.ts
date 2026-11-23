// Petits graphiques SVG (courbes, aires empilées, barres signées), dessinés à la largeur de
// leur conteneur, avec une infobulle au survol. Pas de bibliothèque : quelques dizaines de
// points par courbe suffisent.

export interface Series {
  label: string;
  color: string;
  values: number[];
}

export interface ChartSpec {
  kind: "line" | "stack" | "bars";
  series: Series[];
  /** Libellé de chaque point (date). */
  labels: string[];
  height?: number;
  /** Mise en forme des valeurs (axe et infobulle). */
  format?: (v: number) => string;
  /** L'axe vertical part de zéro (par défaut oui). */
  zero?: boolean;
}

const specs = new Map<string, ChartSpec>();
let nextId = 0;

/** Réserve la place d'un graphique dans du HTML ; le dessiner ensuite avec `mountCharts`. */
export function chart(spec: ChartSpec): string {
  const id = `c${nextId++}`;
  specs.set(id, spec);
  return `<div class="chart" data-chart="${id}" style="min-height:${spec.height ?? 150}px"></div>`;
}

/** Dessine les graphiques réservés dans `root` (et les redessine quand la fenêtre change). */
export function mountCharts(root: HTMLElement) {
  for (const el of root.querySelectorAll<HTMLElement>("[data-chart]")) {
    const spec = specs.get(el.dataset.chart!);
    if (!spec) continue;
    specs.delete(el.dataset.chart!);
    draw(el, spec);
    el.addEventListener("pointermove", (e) => hover(el, spec, e));
    el.addEventListener("pointerleave", () => el.querySelector<HTMLElement>(".chart-tip")?.remove());
    (el as HTMLElement & { _spec?: ChartSpec })._spec = spec;
  }
}

window.addEventListener("resize", () => {
  for (const el of document.querySelectorAll<HTMLElement & { _spec?: ChartSpec }>("[data-chart]")) {
    if (el._spec) draw(el, el._spec);
  }
});

const PAD = { left: 44, right: 10, top: 8, bottom: 20 };

export const fmt = {
  int: (v: number) => (Math.round(v) || 0).toLocaleString("fr-FR"),
  one: (v: number) => (Math.abs(v) >= 100 ? Math.round(v).toLocaleString("fr-FR") : v.toFixed(1).replace(".", ",")),
  price: (v: number) => v.toFixed(2).replace(".", ","),
  pct: (v: number) => `${Math.round(v * 100)} %`,
};

/** Bornes de l'axe vertical : trois pas « ronds » (1, 2, 2,5 ou 5 × 10ⁿ). */
function bounds(spec: ChartSpec): [number, number, number] {
  const n = spec.labels.length;
  let lo = Infinity;
  let hi = -Infinity;
  if (spec.kind === "stack") {
    for (let i = 0; i < n; i++) {
      const sum = spec.series.reduce((a, s) => a + (s.values[i] ?? 0), 0);
      hi = Math.max(hi, sum);
    }
    lo = 0;
  } else {
    for (const s of spec.series) for (const v of s.values) if (Number.isFinite(v)) {
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
  }
  if (!Number.isFinite(lo)) return [0, 3, 1];
  if (spec.zero !== false || spec.kind !== "line") {
    lo = Math.min(0, lo);
    hi = Math.max(0, hi);
  }
  const whole = spec.format === fmt.int;
  const step = niceStep(Math.max(hi - lo, whole ? 3 : 1e-6) / 3, whole);
  lo = Math.floor(lo / step) * step;
  hi = Math.max(lo + step, Math.ceil(hi / step) * step);
  lo += 0; // pas de « −0 »
  return [lo, hi, step];
}

function niceStep(raw: number, whole: boolean): number {
  const mag = 10 ** Math.floor(Math.log10(raw));
  for (const m of [1, 2, 2.5, 5, 10]) {
    const st = m * mag;
    if (st >= raw - 1e-12 && (!whole || Number.isInteger(st))) return st;
  }
  return whole ? Math.max(1, Math.ceil(raw)) : 10 * mag;
}

function draw(el: HTMLElement, spec: ChartSpec) {
  const w = Math.max(160, el.clientWidth);
  const h = spec.height ?? 150;
  const n = spec.labels.length;
  const format = spec.format ?? fmt.one;
  if (n === 0) {
    el.innerHTML = `<p class="muted empty">Pas encore de relevés.</p>`;
    return;
  }
  const [lo, hi, step] = bounds(spec);
  const iw = w - PAD.left - PAD.right;
  const ih = h - PAD.top - PAD.bottom;
  const x = (i: number) => PAD.left + (n === 1 ? iw / 2 : (i / (n - 1)) * iw);
  const y = (v: number) => PAD.top + ih - ((v - lo) / (hi - lo)) * ih;
  const parts: string[] = [];
  // Grille et graduations.
  for (let v = lo; v <= hi + step / 2; v += step) {
    parts.push(`<line class="grid" x1="${PAD.left}" x2="${w - PAD.right}" y1="${y(v)}" y2="${y(v)}"/>`);
    parts.push(`<text class="axis" x="${PAD.left - 6}" y="${y(v) + 4}" text-anchor="end">${format(v)}</text>`);
  }
  if (lo < 0) parts.push(`<line class="zero" x1="${PAD.left}" x2="${w - PAD.right}" y1="${y(0)}" y2="${y(0)}"/>`);
  const ticks = n > 2 ? [0, Math.floor((n - 1) / 2), n - 1] : [0, n - 1];
  for (const i of new Set(ticks)) {
    const anchor = i === 0 ? "start" : i === n - 1 ? "end" : "middle";
    parts.push(`<text class="axis" x="${x(i)}" y="${h - 4}" text-anchor="${anchor}">${spec.labels[i]}</text>`);
  }
  if (spec.kind === "bars") {
    const bw = Math.max(1, (iw / n) * 0.8 / spec.series.length);
    spec.series.forEach((s, k) => {
      s.values.forEach((v, i) => {
        if (!v) return;
        const x0 = x(i) - (bw * spec.series.length) / 2 + k * bw;
        const y0 = Math.min(y(v), y(0));
        parts.push(`<rect x="${x0.toFixed(1)}" y="${y0.toFixed(1)}" width="${bw.toFixed(1)}" height="${Math.abs(y(v) - y(0)).toFixed(1)}" fill="${s.color}"/>`);
      });
    });
  } else if (spec.kind === "stack") {
    const base = new Array(n).fill(0);
    for (const s of spec.series) {
      const top = s.values.map((v, i) => base[i] + (v ?? 0));
      const pts = top.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`);
      const back = base.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`).reverse();
      parts.push(`<polygon points="${[...pts, ...back].join(" ")}" fill="${s.color}" fill-opacity="0.85" stroke="${s.color}" stroke-width="0.5"/>`);
      top.forEach((v, i) => (base[i] = v));
    }
  } else {
    for (const s of spec.series) {
      const pts = s.values.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`);
      parts.push(`<polyline points="${pts.join(" ")}" fill="none" stroke="${s.color}" stroke-width="1.8" stroke-linejoin="round"/>`);
    }
  }
  const legend = spec.series.length > 1
    ? `<div class="legend">${spec.series.map((s) => `<span><i style="background:${s.color}"></i>${s.label}</span>`).join("")}</div>`
    : "";
  el.innerHTML = `<svg width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${parts.join("")}<line class="cursor" x1="0" x2="0" y1="${PAD.top}" y2="${PAD.top + ih}" visibility="hidden"/></svg>${legend}`;
}

function hover(el: HTMLElement, spec: ChartSpec, e: PointerEvent) {
  const n = spec.labels.length;
  if (!n) return;
  const svg = el.querySelector("svg");
  if (!svg) return;
  const r = svg.getBoundingClientRect();
  const iw = r.width - PAD.left - PAD.right;
  const i = Math.max(0, Math.min(n - 1, Math.round(((e.clientX - r.left - PAD.left) / iw) * (n - 1))));
  const format = spec.format ?? fmt.one;
  const cx = PAD.left + (n === 1 ? iw / 2 : (i / (n - 1)) * iw);
  const cursor = svg.querySelector<SVGLineElement>(".cursor")!;
  cursor.setAttribute("x1", String(cx));
  cursor.setAttribute("x2", String(cx));
  cursor.setAttribute("visibility", "visible");
  let tip = el.querySelector<HTMLElement>(".chart-tip");
  if (!tip) {
    tip = document.createElement("div");
    tip.className = "chart-tip";
    el.appendChild(tip);
  }
  const rows = spec.series
    .filter((s) => spec.series.length < 8 || (s.values[i] ?? 0) !== 0)
    .map((s) => `<div><i style="background:${s.color}"></i>${s.label} <b>${format(s.values[i] ?? 0)}</b></div>`);
  if (spec.kind === "stack" && spec.series.length > 1) {
    rows.push(`<div class="muted">total <b>${format(spec.series.reduce((a, s) => a + (s.values[i] ?? 0), 0))}</b></div>`);
  }
  tip.innerHTML = `<div class="muted">${spec.labels[i]}</div>${rows.join("")}`;
  const left = cx + 12 + tip.offsetWidth > r.width ? cx - 12 - tip.offsetWidth : cx + 12;
  tip.style.left = `${left}px`;
  tip.style.top = `${PAD.top}px`;
  svg.querySelector(".cursor")?.setAttribute("visibility", "visible");
}
