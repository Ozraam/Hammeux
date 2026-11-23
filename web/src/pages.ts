// Pages du site (phase 7), ouvertes par-dessus la carte et adressées par le fragment de
// l'adresse (on peut les partager et revenir en arrière) :
//   #/monde            tableau de bord du monde
//   #/village/3        tableau de bord d'un village
//   #/commerce         carte du commerce
//   #/chronique?…      chronique complète (filtres : village, kind, person, q, annals)
//   #/arbre/12         arbre généalogique
//   #/visite?since=T   depuis ta dernière visite

import { chart, fmt, mountCharts, type Series } from "./charts";
import { css } from "./palette";
import { cap, esc, money } from "./panels";
import type { EventPage } from "./protocol/EventPage";
import type { EventView } from "./protocol/EventView";
import type { FamilyTree } from "./protocol/FamilyTree";
import type { HistoryView } from "./protocol/HistoryView";
import type { MapInfo } from "./protocol/MapInfo";
import type { Summary } from "./protocol/Summary";
import type { TradeMap } from "./protocol/TradeMap";
import type { TreePerson } from "./protocol/TreePerson";

export interface PageActions {
  info: MapInfo;
  openPerson(id: number): void;
  openVillage(id: number): void;
  goTo(x: number, y: number): void;
  /** Vue d'ensemble de la carte (pour le fond de la carte du commerce). */
  overview(): HTMLCanvasElement;
  /** Tronçons de carte déjà chargés (la vue d'ensemble change tant qu'il en manque). */
  mapLoaded(): number;
  /** Tick de la dernière visite (avant celle-ci), s'il y en a une. */
  lastVisit(): number | null;
  /** Tick actuel du monde. */
  now(): number;
}

const GOOD_COLORS = ["#e0c068", "#efe3c2", "#c98b4a", "#5aa9e6", "#d0605e", "#8a6a45", "#a3a3a3", "#7c6f9b", "#5fc2a8"];
const FOODS = [0, 1, 2, 3, 4];
const MATERIALS = [5, 6, 7, 8];
const C = { pop: "#7bd88f", kids: "#ffd98a", elders: "#b9a6e6", births: "#7bd88f", deaths: "#ef6f6c", gone: "#f2a65a", line: "#ffd98a", money: "#7cc4f2" };

type Route = { name: string; id: number | null; params: URLSearchParams };

function parse(hash: string): Route | null {
  const m = hash.match(/^#\/([a-z]+)(?:\/(\d+))?(?:\?(.*))?$/);
  if (!m) return null;
  return { name: m[1], id: m[2] !== undefined ? Number(m[2]) : null, params: new URLSearchParams(m[3] ?? "") };
}

export class Pages {
  private readonly el: HTMLElement;
  private route: Route | null = null;
  private timer = 0;
  /** Choix faits dans les pages (gardés d'une page à l'autre). */
  private state = { all: false, good: 2, flowGood: -1, carts: true, colonies: true };
  private overviewUrl = "";
  private overviewAt = 0;
  private overviewLoaded = -1;
  /** Chronique : ce qui est affiché, pour « plus ancien ». */
  private chron: { events: EventView[]; more: boolean; oldest: string } = { events: [], more: false, oldest: "" };
  private searchTimer = 0;

  constructor(private readonly a: PageActions) {
    this.el = document.getElementById("page")!;
    this.el.addEventListener("click", (e) => this.onClick(e));
    this.el.addEventListener("change", (e) => this.onChange(e));
    this.el.addEventListener("input", (e) => this.onInput(e));
    window.addEventListener("hashchange", () => this.show());
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && this.route) this.close();
    });
  }

  get open() {
    return this.route !== null;
  }

  /** Affiche la page désignée par l'adresse (ou rien). */
  show() {
    clearTimeout(this.timer);
    const r = parse(location.hash);
    const known = r && ["monde", "village", "commerce", "chronique", "arbre", "visite"].includes(r.name);
    this.route = known ? r : null;
    this.el.hidden = !this.route;
    document.body.classList.toggle("paged", !!this.route);
    if (!this.route) return;
    if (this.route.name === "chronique") this.chron = { events: [], more: false, oldest: "" };
    this.el.innerHTML = this.frame(`<p class="muted">Chargement…</p>`);
    this.render(true);
  }

  go(hash: string) {
    if (location.hash === hash) this.show();
    else location.hash = hash;
  }

  close() {
    history.pushState(null, "", location.pathname + location.search);
    this.show();
  }

  private async render(first = false) {
    const r = this.route;
    if (!r) return;
    const body = this.el.querySelector<HTMLElement>(".page-body");
    const scroll = body?.scrollTop ?? 0;
    let html = "";
    let refresh = 0;
    try {
      switch (r.name) {
        case "monde":
          html = await this.dashboard(null);
          refresh = 20000;
          break;
        case "village":
          html = await this.dashboard(r.id ?? 0);
          refresh = 20000;
          break;
        case "commerce":
          html = await this.trade();
          refresh = 5000;
          break;
        case "chronique":
          html = await this.chronicle(first);
          break;
        case "arbre":
          html = await this.tree(r.id ?? 0);
          break;
        case "visite":
          {
            const since = r.params.get("since") ?? this.a.lastVisit();
            html = await this.visit(since === null ? Math.max(0, this.a.now() - 8640) : Number(since), since !== null);
          }
          break;
      }
    } catch (e) {
      html = `<p>Page indisponible (${esc(String(e))}).</p>`;
    }
    if (this.route !== r) return; // on a changé de page entre-temps
    this.el.innerHTML = this.frame(html);
    mountCharts(this.el);
    const nb = this.el.querySelector<HTMLElement>(".page-body");
    if (nb && !first) nb.scrollTop = scroll;
    if (refresh) this.timer = window.setTimeout(() => this.render(), refresh);
  }

  private frame(body: string): string {
    const tab = (hash: string, label: string, on: boolean) => `<a href="${hash}" class="tab${on ? " on" : ""}">${label}</a>`;
    const n = this.route?.name;
    return `
      <header>
        <nav>
          ${tab("#/monde", "Monde", n === "monde" || n === "village")}
          ${tab("#/commerce", "Commerce", n === "commerce")}
          ${tab("#/chronique", "Chronique", n === "chronique")}
          ${tab("#/visite", "Depuis ta visite", n === "visite")}
          ${n === "arbre" ? tab(location.hash, "Arbre", true) : ""}
        </nav>
        <button class="close" data-close aria-label="Fermer (Échap)">×</button>
      </header>
      <div class="page-body">${body}</div>`;
  }

  // ---------------------------------------------------------------- événements

  private onClick(e: MouseEvent) {
    const t = (e.target as Element).closest<HTMLElement>("[data-close],[data-person],[data-village],[data-sheet],[data-goto],[data-range],[data-more]");
    if (!t) return;
    const d = t.dataset;
    if (d.close !== undefined) this.close();
    else if (d.person) this.a.openPerson(Number(d.person));
    else if (d.village) this.go(`#/village/${d.village}`);
    else if (d.sheet) this.a.openVillage(Number(d.sheet));
    else if (d.goto) {
      const [x, y] = d.goto.split(",").map(Number);
      this.close();
      this.a.goTo(x, y);
    } else if (d.range) {
      this.state.all = d.range === "all";
      this.render();
    } else if (d.more !== undefined) this.more();
  }

  private onChange(e: Event) {
    const t = e.target as HTMLInputElement | HTMLSelectElement;
    const s = this.state;
    switch (t.name) {
      case "good":
        s.good = Number(t.value);
        this.render();
        break;
      case "flow-good":
        s.flowGood = Number(t.value);
        this.render();
        break;
      case "carts":
        s.carts = (t as HTMLInputElement).checked;
        this.render();
        break;
      case "colonies":
        s.colonies = (t as HTMLInputElement).checked;
        this.render();
        break;
      case "village":
      case "kind":
      case "annals":
        this.filter(t.name, t.type === "checkbox" ? ((t as HTMLInputElement).checked ? "1" : "") : t.value);
        break;
    }
  }

  private onInput(e: Event) {
    const t = e.target as HTMLInputElement;
    if (t.name !== "q") return;
    clearTimeout(this.searchTimer);
    this.searchTimer = window.setTimeout(() => this.filter("q", t.value.trim()), 350);
  }

  /** Change un filtre de la chronique (dans l'adresse, pour pouvoir la partager). */
  private filter(key: string, value: string) {
    const p = new URLSearchParams(this.route?.params);
    if (value) p.set(key, value);
    else p.delete(key);
    const q = p.toString();
    history.replaceState(null, "", `#/chronique${q ? `?${q}` : ""}`);
    this.show();
    if (key === "q") {
      // Garder la main dans le champ de recherche.
      const input = () => this.el.querySelector<HTMLInputElement>("input[name=q]");
      const wait = setInterval(() => {
        const i = input();
        if (i && !i.disabled && this.el.querySelector(".chron-list")) {
          clearInterval(wait);
          i.focus();
          i.setSelectionRange(i.value.length, i.value.length);
        }
      }, 50);
      setTimeout(() => clearInterval(wait), 3000);
    }
  }

  // ---------------------------------------------------------------- tableaux de bord

  private async dashboard(id: number | null): Promise<string> {
    const s = this.state;
    const url = id === null ? `/api/world/history?all=${s.all}` : `/api/village/${id}/history?all=${s.all}`;
    const res = await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const h: HistoryView = await res.json();
    const n = h.days.length;
    const last = <T>(a: T[]) => a[a.length - 1];
    const goods = this.a.info.goods;
    const range = `<div class="seg">
        <button data-range="days" class="${s.all ? "" : "on"}">4 dernières années</button>
        <button data-range="all" class="${s.all ? "on" : ""}">Toute l'histoire</button>
      </div>`;
    const title = id === null
      ? `<h1>Le monde</h1><p class="sub">${this.a.info.villages.length} villages au départ · tous les villages ensemble</p>`
      : `<h1><span class="swatch" style="background:${css(h.color)}"></span>${esc(h.name)}</h1>
         <p class="sub"><a data-sheet="${id}">Fiche du village</a> · <a href="#/chronique?village=${id}">Sa chronique</a> · <a href="#/monde">Tout le monde</a></p>`;
    if (n === 0) return `${title}<p class="muted">Pas encore de relevés : le premier est pris à minuit.</p>`;

    const delta = (a: number[]) => {
      const d = last(a) - a[0];
      return d === 0 ? "" : `<span class="${d > 0 ? "pos" : "neg"}">${d > 0 ? "+" : "−"}${fmt.int(Math.abs(d))}</span>`;
    };
    const sum = (a: number[]) => a.reduce((x, y) => x + y, 0);
    const tile = (label: string, value: string, extra = "") => `<div class="tile"><span>${label}</span><b>${value}</b>${extra ? `<small>${extra}</small>` : ""}</div>`;
    const period = s.all ? "depuis le début" : "sur la période";
    const tiles = `<div class="tiles">
      ${tile("Habitants", fmt.int(last(h.population)), `${delta(h.population)} ${period}`)}
      ${tile("Enfants · aînés", `${last(h.children)} · ${last(h.elders)}`)}
      ${tile("Naissances · décès", `${sum(h.births)} · ${sum(h.deaths)}`, period)}
      ${tile("Vivres d'avance", `${Math.round(last(h.food_days))} jours`)}
      ${tile("Caisse", money(last(h.treasury)), `taxe ${fmt.pct(last(h.tax))}`)}
      ${tile("Argent des foyers", money(last(h.coins)))}
      ${tile("Maisons habitées", fmt.int(last(h.houses)))}
      ${tile("Malades", fmt.int(last(h.sick)))}
    </div>`;

    const labels = h.labels;
    const adults = h.population.map((p, i) => p - h.children[i] - h.elders[i]);
    const people = id === null
      ? chart({ kind: "stack", labels, format: fmt.int, height: 190, series: h.villages.map((v) => ({ label: v.name, color: css(v.color), values: v.values })) })
      : chart({
          kind: "stack", labels, format: fmt.int, height: 170, series: [
            { label: "enfants", color: C.kids, values: h.children },
            { label: "adultes", color: C.pop, values: adults },
            { label: "aînés", color: C.elders, values: h.elders },
          ],
        });
    const goodSeries = (idx: number[], of: number[][]): Series[] => idx.map((g) => ({ label: goods[g].label, color: GOOD_COLORS[g], values: of[g] }));
    const g = s.good;
    const goodSelect = `<select name="good">${goods.map((x) => `<option value="${x.id}"${x.id === g ? " selected" : ""}>${esc(x.label)}</option>`).join("")}</select>`;
    const charts = `
      <div class="charts">
        <section class="wide"><h3>Habitants</h3>${people}</section>
        <section><h3>Naissances, décès et départs</h3>${chart({
          kind: "bars", labels, format: fmt.int, series: [
            { label: "naissances", color: C.births, values: h.births },
            { label: "décès", color: C.deaths, values: h.deaths.map((v) => -v) },
            { label: "départs", color: C.gone, values: h.departures.map((v) => -v) },
          ],
        })}</section>
        <section><h3>Vivres d'avance (jours)</h3>${chart({ kind: "line", labels, format: fmt.int, series: [{ label: "jours", color: C.line, values: h.food_days }] })}</section>
        <section><h3>Argent (sous)</h3>${chart({
          kind: "line", labels, format: fmt.int, series: [
            { label: "caisse du village", color: C.line, values: h.treasury },
            { label: "foyers", color: C.money, values: h.coins },
          ],
        })}</section>
        <section><h3>Réserves de nourriture</h3>${chart({ kind: "line", labels, format: fmt.int, series: goodSeries(FOODS, h.stock) })}</section>
        <section><h3>Matériaux et outils</h3>${chart({ kind: "line", labels, format: fmt.int, series: goodSeries(MATERIALS, h.stock) })}</section>
        <section><h3>Prix ${id === null ? "moyen " : ""}du marché ${goodSelect}</h3>${chart({ kind: "line", labels, format: fmt.price, zero: false, series: [{ label: goods[g].label, color: GOOD_COLORS[g], values: h.price[g] }] })}</section>
        <section><h3>Vendu au marché : ${esc(goods[g].label)}</h3>${chart({ kind: "bars", labels, format: fmt.one, series: [{ label: goods[g].label, color: GOOD_COLORS[g], values: h.volume[g] }] })}</section>
        <section><h3>Taxe du marché</h3>${chart({ kind: "line", labels, format: fmt.pct, series: [{ label: "taxe", color: C.line, values: h.tax }] })}</section>
        <section><h3>Malades</h3>${chart({ kind: "bars", labels, format: fmt.int, series: [{ label: "malades", color: "#b8e07a", values: h.sick }] })}</section>
      </div>`;

    const table = id === null && h.villages.length
      ? `<h3>Villages</h3><table class="grid-table">
          <tr><th>Village</th><th class="num">Habitants</th><th class="num">${s.all ? "Depuis le début" : "Sur la période"}</th></tr>
          ${[...h.villages]
            .sort((a, b) => last(b.values) - last(a.values))
            .map((v) => {
              const first = v.values.find((x) => x > 0) ?? 0;
              const d = last(v.values) - first;
              return `<tr><td><a data-village="${v.id}"><span class="swatch" style="background:${css(v.color)}"></span>${esc(v.name)}</a></td>
                <td class="num">${fmt.int(last(v.values))}</td>
                <td class="num ${d > 0 ? "pos" : d < 0 ? "neg" : "muted"}">${d > 0 ? "+" : d < 0 ? "−" : ""}${fmt.int(Math.abs(d))}</td></tr>`;
            })
            .join("")}
        </table>`
      : "";
    return `${title}${range}${tiles}${charts}${table}`;
  }

  // ---------------------------------------------------------------- commerce

  private overviewImage(): string {
    const loaded = this.a.mapLoaded();
    if (!this.overviewUrl || loaded !== this.overviewLoaded || performance.now() - this.overviewAt > 60000) {
      this.overviewUrl = this.a.overview().toDataURL("image/png");
      this.overviewAt = performance.now();
      this.overviewLoaded = loaded;
    }
    return this.overviewUrl;
  }

  private async trade(): Promise<string> {
    const t: TradeMap = await (await fetch("/api/trade")).json();
    const s = this.state;
    const goods = this.a.info.goods;
    const W = t.width, H = t.height;
    const u = W / 900; // un pixel à l'écran, à peu près
    const byId = new Map(t.villages.map((v) => [v.id, v]));
    const center = (id: number) => {
      const v = byId.get(id)!;
      return { x: v.x + 1, y: v.y + 1 };
    };
    const parts: string[] = [`<image href="${this.overviewImage()}" x="0" y="0" width="${W}" height="${H}" opacity="0.75"/>`];
    // Filiations : qui a fondé qui.
    if (s.colonies) {
      for (const v of t.villages) {
        if (v.founded_from === null || !byId.has(v.founded_from)) continue;
        const a = center(v.founded_from), b = center(v.id);
        parts.push(`<line x1="${a.x}" y1="${a.y}" x2="${b.x}" y2="${b.y}" class="colony" stroke-width="${1.2 * u}" stroke-dasharray="${2 * u} ${4 * u}"/>`);
      }
    }
    // Trajets et routes.
    for (const r of [...t.routes].sort((a, b) => a.trips - b.trips)) {
      const pts = r.path.map(([x, y]) => `${x + 0.5},${y + 0.5}`).join(" ");
      const w = (1 + Math.sqrt(r.trips) * 0.45) * u;
      parts.push(`<polyline points="${pts}" class="${r.road ? "road" : "track"}" stroke-width="${w.toFixed(2)}"${r.road ? "" : ` stroke-dasharray="${3 * u} ${3 * u}"`}><title>${esc(byId.get(r.a)?.name ?? "")} – ${esc(byId.get(r.b)?.name ?? "")} : ${r.trips} voyage${r.trips > 1 ? "s" : ""}${r.road ? " (route)" : ""}</title></polyline>`);
    }
    // Flux : flèches courbes du village des marchands vers celui où ils ont vendu.
    const amount = (f: (typeof t.flows)[number]) => (s.flowGood < 0 ? f.coins : f.goods[s.flowGood]);
    const flows = t.flows.filter((f) => amount(f) >= 1 && byId.has(f.from) && byId.has(f.to));
    const max = Math.max(1, ...flows.map(amount));
    for (const f of flows) {
      const a = center(f.from), b = center(f.to);
      const dx = b.x - a.x, dy = b.y - a.y;
      const len = Math.hypot(dx, dy) || 1;
      const cx = (a.x + b.x) / 2 - (dy / len) * len * 0.18, cy = (a.y + b.y) / 2 + (dx / len) * len * 0.18;
      const w = (1 + 7 * Math.sqrt(amount(f) / max)) * u;
      const color = css(byId.get(f.from)!.color);
      // Pointe de flèche aux deux tiers de la courbe.
      const k = 0.62;
      const px = (1 - k) ** 2 * a.x + 2 * (1 - k) * k * cx + k * k * b.x;
      const py = (1 - k) ** 2 * a.y + 2 * (1 - k) * k * cy + k * k * b.y;
      const tx = 2 * (1 - k) * (cx - a.x) + 2 * k * (b.x - cx), ty = 2 * (1 - k) * (cy - a.y) + 2 * k * (b.y - cy);
      const tl = Math.hypot(tx, ty) || 1;
      const hx = tx / tl, hy = ty / tl;
      const hs = w + 5 * u;
      const tip = `${px + hx * hs},${py + hy * hs} ${px - hy * hs * 0.7},${py + hx * hs * 0.7} ${px + hy * hs * 0.7},${py - hx * hs * 0.7}`;
      const what = s.flowGood < 0 ? flowGoods(f.goods, goods) : `${fmt.int(f.goods[s.flowGood])} ${goods[s.flowGood].label}`;
      const title = `<title>${esc(byId.get(f.from)!.name)} → ${esc(byId.get(f.to)!.name)} : ${esc(what)} (${money(f.coins)})</title>`;
      parts.push(`<g class="flow"><path d="M${a.x},${a.y} Q${cx},${cy} ${b.x},${b.y}" stroke="${color}" stroke-width="${w.toFixed(2)}"/><polygon points="${tip}" fill="${color}"/>${title}</g>`);
    }
    // Charrettes en route.
    if (s.carts) {
      for (const c of t.carts) {
        const cargo = c.cargo.map((g) => `${Math.round(g.amount)} ${g.label}`).join(", ");
        parts.push(`<circle class="cart" data-person="${c.id}" cx="${c.x}" cy="${c.y}" r="${3 * u}" stroke-width="${u}"><title>${esc(c.name)} ${esc(c.leg)} ${esc(byId.get(c.dest)?.name ?? "")}${cargo ? ` avec ${esc(cargo)}` : ""}</title></circle>`);
      }
    }
    // Villages.
    for (const v of t.villages) {
      const r = (3 + Math.sqrt(v.population) * 0.7) * u;
      parts.push(`<g class="tv" data-village="${v.id}"><circle cx="${v.x + 1}" cy="${v.y + 1}" r="${r}" fill="${css(v.color)}" stroke-width="${1.5 * u}"/>
        <text x="${v.x + 1}" y="${v.y + 1 + r + 13 * u}" font-size="${12 * u}" stroke-width="${3 * u}">${esc(v.name)}</text>
        <title>${esc(v.name)} : ${v.population} habitants, ${v.merchants} marchand${v.merchants > 1 ? "s" : ""}</title></g>`);
    }

    const roads = t.routes.filter((r) => r.road).length;
    const trips = t.routes.reduce((a, r) => a + r.trips, 0);
    const top = [...t.flows].sort((a, b) => b.coins - a.coins).slice(0, 15);
    const sold = t.sold.map((q, g) => ({ q, g })).filter((x) => x.q >= 1).sort((a, b) => b.q - a.q);
    const name = (id: number) => `<a data-village="${id}">${esc(byId.get(id)?.name ?? "?")}</a>`;
    const select = `<select name="flow-good"><option value="-1"${s.flowGood < 0 ? " selected" : ""}>tous les biens (en sous)</option>${goods
      .map((x) => `<option value="${x.id}"${x.id === s.flowGood ? " selected" : ""}>${esc(x.label)}</option>`)
      .join("")}</select>`;
    return `
      <h1>Commerce</h1>
      <p class="sub">${roads} route${roads > 1 ? "s" : ""} · ${fmt.int(trips)} voyages · ${t.carts.length} charrette${t.carts.length > 1 ? "s" : ""} en route</p>
      <div class="trade">
        <div class="trade-map"><svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="xMidYMid meet">${parts.join("")}</svg></div>
        <aside>
          <div class="controls">
            <label>Flux : ${select}</label>
            <label><input type="checkbox" name="carts"${s.carts ? " checked" : ""}> charrettes</label>
            <label><input type="checkbox" name="colonies"${s.colonies ? " checked" : ""}> filiations</label>
          </div>
          <p class="legend-line"><i class="l-road"></i> route <i class="l-track"></i> chemin <i class="l-flow"></i> ventes des marchands <i class="l-colony"></i> village fondé par un autre</p>
          <h3>Plus grands échanges</h3>
          ${top.length ? `<ul class="flows">${top.map((f) => `<li>${name(f.from)} → ${name(f.to)} <span class="muted">· ${money(f.coins)}</span><br><span class="muted">${esc(flowGoods(f.goods, goods))}</span></li>`).join("")}</ul>` : `<p class="muted">Aucune vente encore relevée.</p>`}
          <h3>Vendu par les marchands depuis le début</h3>
          <p>${sold.map((x) => `${fmt.int(x.q)} ${esc(goods[x.g].label)}`).join(" · ") || "—"}</p>
          ${t.carts.length ? `<h3>Sur les routes</h3><ul class="flows">${t.carts
            .map((c) => `<li><a data-person="${c.id}">${esc(c.name)}</a> <span class="muted">${esc(c.leg)}</span> ${name(c.dest)}${c.cargo.length ? `<br><span class="muted">${esc(c.cargo.map((g) => `${Math.round(g.amount)} ${g.label}`).join(", "))}</span>` : ""}</li>`)
            .join("")}</ul>` : ""}
        </aside>
      </div>`;
  }

  // ---------------------------------------------------------------- chronique

  private async chronicle(first: boolean): Promise<string> {
    const p = this.route!.params;
    if (first || !this.chron.events.length) {
      const page = await this.fetchChronicle(null);
      this.chron = { events: page.events, more: page.more, oldest: page.oldest };
    }
    const info = this.a.info;
    const village = p.get("village") ?? "";
    const kind = p.get("kind") ?? "";
    const annals = p.get("annals") === "1";
    const person = p.get("person");
    const villages = [...info.villages].sort((a, b) => a.name.localeCompare(b.name, "fr"));
    const filters = `
      <div class="filters">
        <input type="search" name="q" placeholder="Chercher (un nom, « route », « fièvre »…)" value="${esc(p.get("q") ?? "")}">
        <select name="village"><option value="">tous les villages</option>${villages
          .map((v) => `<option value="${v.id}"${String(v.id) === village ? " selected" : ""}>${esc(v.name)}</option>`)
          .join("")}</select>
        <select name="kind"><option value="">tout</option>${info.event_kinds
          .map((k) => `<option value="${k.id}"${String(k.id) === kind ? " selected" : ""}>${esc(cap(k.label))}</option>`)
          .join("")}</select>
        <label title="Les événements marquants, gardés bien plus longtemps que la chronique"><input type="checkbox" name="annals"${annals ? " checked" : ""}> annales seulement</label>
      </div>
      ${person ? `<p class="muted">Événements de <a data-person="${person}">cet habitant</a> · <a href="#/chronique">tout voir</a></p>` : ""}`;
    const list = this.chron.events.length ? groupByDay(this.chron.events) : `<p class="muted">Aucun événement ne correspond.</p>`;
    return `
      <h1>${annals ? "Annales" : "Chronique"}</h1>
      <p class="sub">${annals ? "Les événements marquants" : "Tout ce qui s'est passé"}${this.chron.oldest ? `, depuis ${esc(this.chron.oldest)}` : ""}</p>
      ${filters}
      <div class="chron-list">${list}</div>
      ${this.chron.more ? `<button data-more class="more">Plus ancien</button>` : ""}`;
  }

  private async fetchChronicle(before: number | null): Promise<EventPage> {
    const p = new URLSearchParams(this.route!.params);
    if (p.get("annals") === "1") p.set("annals", "true");
    else p.delete("annals");
    p.set("limit", "80");
    if (before !== null) p.set("before", String(before));
    const res = await fetch(`/api/chronicle?${p}`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return res.json();
  }

  private async more() {
    const last = this.chron.events[this.chron.events.length - 1];
    if (!last) return;
    const page = await this.fetchChronicle(last.seq);
    this.chron.events.push(...page.events);
    this.chron.more = page.more;
    this.render();
  }

  // ---------------------------------------------------------------- arbre généalogique

  private async tree(id: number): Promise<string> {
    const res = await fetch(`/api/tree/${id}`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const t: FamilyTree = await res.json();
    const root = t.people.find((p) => p.id === t.root)!;
    const { boxes, lines, width, height } = layoutTree(t);
    const box = (p: TreePerson, x: number, y: number) => `
      <div class="tp ${p.female ? "f" : "m"}${p.alive ? "" : " dead"}${p.id === t.root ? " root" : ""}" style="left:${x}px;top:${y}px;width:${BW}px;height:${BH}px">
        <a href="#/arbre/${p.id}" class="tn" title="Centrer l'arbre sur ${esc(p.name)}">${esc(p.name)}</a>
        <span class="ty">${esc(p.years)}</span>
        <span class="tr">${esc([p.role, p.village].filter(Boolean).join(" · "))}</span>
        <button class="ti" data-person="${p.id}" title="Fiche">i</button>
      </div>`;
    return `
      <h1>Famille de ${esc(root.name)}</h1>
      <p class="sub">${t.people.length} personnes · ancêtres au-dessus, descendants en dessous · clic sur un nom pour recentrer · <a data-person="${root.id}">sa fiche</a> · <a href="#/chronique?person=${root.id}">sa chronique</a></p>
      <div class="tree-wrap"><div class="tree" style="width:${width}px;height:${height}px">
        <svg width="${width}" height="${height}">${lines.join("")}</svg>
        ${boxes.map(([p, x, y]) => box(p, x, y)).join("")}
      </div></div>`;
  }

  // ---------------------------------------------------------------- depuis ta dernière visite

  private async visit(since: number, known: boolean): Promise<string> {
    const s: Summary = await (await fetch(`/api/summary?since=${since}`)).json();
    const d = s.population_now - s.population_then;
    const changed = s.villages
      .filter((v) => v.new || v.then !== v.now || v.births || v.deaths)
      .sort((a, b) => Math.abs(b.now - b.then) - Math.abs(a.now - a.then));
    const tile = (label: string, value: string, extra = "") => `<div class="tile"><span>${label}</span><b>${value}</b>${extra ? `<small>${extra}</small>` : ""}</div>`;
    return `
      <h1>${known ? "Depuis ta dernière visite" : "Le dernier jour"}</h1>
      <p class="sub">${known ? "Dernière visite" : "Depuis"} : ${esc(s.from)} · maintenant : ${esc(s.to)} — <b>${esc(s.elapsed)}</b> de jeu.</p>
      <div class="tiles">
        ${tile("Habitants", fmt.int(s.population_now), `${d > 0 ? "+" : d < 0 ? "−" : "±"}${fmt.int(Math.abs(d))} (${fmt.int(s.population_then)} avant)`)}
        ${tile("Naissances", fmt.int(s.births))}
        ${tile("Décès", fmt.int(s.deaths))}
        ${tile("Départs", fmt.int(s.departures))}
      </div>
      ${s.kinds.length ? `<h3>Dans la chronique${s.partial ? ` <span class="muted">(partielle : elle ne remonte pas si loin)</span>` : ""}</h3>
        <p class="chips">${s.kinds.map((k) => `<a class="chip" href="#/chronique?kind=${k.kind}">${esc(cap(k.label))} <b>${k.count}</b></a>`).join("")}</p>` : ""}
      <div class="visit">
        <section>
          <h3>Ce qu'il faut savoir</h3>
          ${s.highlights.length ? groupByDay(s.highlights) : `<p class="muted">Rien de marquant : la vie a suivi son cours.</p>`}
        </section>
        <section>
          <h3>Villages</h3>
          ${changed.length ? `<table class="grid-table">
            <tr><th>Village</th><th class="num">Avant</th><th class="num">Maintenant</th><th class="num">Naiss.</th><th class="num">Décès</th></tr>
            ${changed
              .map((v) => {
                const dv = v.now - v.then;
                return `<tr><td><a data-village="${v.village.id}"><span class="swatch" style="background:${css(v.color)}"></span>${esc(v.village.name)}</a>${v.new ? ` <span class="badge">nouveau</span>` : ""}</td>
                  <td class="num muted">${v.new ? "—" : v.then}</td>
                  <td class="num">${v.now} <span class="${dv > 0 ? "pos" : dv < 0 ? "neg" : "muted"}">${dv > 0 ? "+" : dv < 0 ? "−" : ""}${dv ? Math.abs(dv) : ""}</span></td>
                  <td class="num">${v.births || ""}</td><td class="num">${v.deaths || ""}</td></tr>`;
              })
              .join("")}
          </table>` : `<p class="muted">Aucun changement.</p>`}
        </section>
      </div>`;
  }
}

function flowGoods(q: number[], goods: MapInfo["goods"]): string {
  return q
    .map((v, g) => ({ v, g }))
    .filter((x) => x.v >= 1)
    .sort((a, b) => b.v - a.v)
    .map((x) => `${Math.round(x.v)} ${goods[x.g].label}`)
    .join(", ");
}

/** Événements regroupés par jour (« An 3, Été j4 »), avec l'heure. */
function groupByDay(events: EventView[]): string {
  const out: string[] = [];
  let day = "";
  for (const e of events) {
    const [d, time] = splitDate(e.date);
    if (d !== day) {
      if (day) out.push(`</ul>`);
      out.push(`<h4>${esc(d)}</h4><ul class="events">`);
      day = d;
    }
    const who = e.people.length ? `<a data-person="${e.people[0]}">${esc(e.text)}</a>` : esc(e.text);
    const where = e.village !== null && e.village_name ? ` <a class="muted where" data-village="${e.village}">${esc(e.village_name)}</a>` : "";
    out.push(`<li><span class="time muted">${esc(time)}</span> ${who}${where}</li>`);
  }
  if (day) out.push(`</ul>`);
  return out.join("");
}

function splitDate(date: string): [string, string] {
  const k = date.lastIndexOf(" ");
  return k > 0 ? [date.slice(0, k), date.slice(k + 1)] : [date, ""];
}

// ------------------------------------------------------------------ disposition de l'arbre

const BW = 158;
const BH = 54;
const GX = 14;
const GY = 46;
const STEP = BW + GX;

/**
 * Place les personnes : une rangée par génération. Les descendants forment un arbre sous la
 * fratrie de la personne (chaque couple centré au-dessus de ses enfants) ; les ancêtres
 * s'écartent de plus en plus en remontant pour que les couples ne se chevauchent pas.
 */
function layoutTree(t: FamilyTree) {
  const byId = new Map(t.people.map((p) => [p.id, p]));
  const pos = new Map<number, { x: number; g: number }>();
  const lines: string[] = [];
  const root = byId.get(t.root)!;
  const gens = t.people.map((p) => p.generation);
  const minG = Math.min(0, ...gens);
  const rowY = (g: number) => (g - minG) * (BH + GY) + 8;

  // Descendants : largeur de chaque sous-arbre (en boîtes).
  const kids = (p: TreePerson) => p.children.filter((c) => byId.get(c)?.generation === p.generation + 1);
  const partnerOf = (p: TreePerson) => {
    const q = p.partner !== null ? byId.get(p.partner) : undefined;
    return q && q.generation === p.generation ? q : undefined;
  };
  const placedPartner = new Set<number>();
  const widthOf = new Map<number, number>();
  const width = (p: TreePerson): number => {
    const own = partnerOf(p) ? 2 : 1;
    const w = Math.max(own, kids(p).reduce((a, c) => a + width(byId.get(c)!), 0));
    widthOf.set(p.id, w);
    return w;
  };
  const place = (p: TreePerson, left: number) => {
    const w = widthOf.get(p.id) ?? width(p);
    const q = partnerOf(p);
    const own = q && !pos.has(q.id) ? 2 : 1;
    const cx = left + (w * STEP) / 2;
    const x = cx - (own * STEP) / 2;
    pos.set(p.id, { x, g: p.generation });
    if (own === 2 && q) {
      pos.set(q.id, { x: x + STEP, g: q.generation });
      placedPartner.add(q.id);
    }
    let l = left;
    for (const c of kids(p)) {
      const cp = byId.get(c)!;
      if (pos.has(c)) continue;
      place(cp, l);
      l += (widthOf.get(c) ?? 1) * STEP;
    }
  };

  // La fratrie (la personne comprise), par ordre d'identifiant (≈ de naissance).
  const siblings = t.people
    .filter((p) => p.generation === 0 && !placedPartner.has(p.id) && (p.id === root.id || p.parents.some((x) => root.parents.includes(x))))
    .sort((a, b) => a.id - b.id);
  let left = 0;
  for (const s of siblings) {
    width(s);
    if (pos.has(s.id)) continue;
    place(s, left);
    left += (widthOf.get(s.id) ?? 1) * STEP;
  }
  // Conjoints restés seuls (déjà placés avec leur partenaire normalement), et oubliés.
  for (const p of t.people) {
    if (!pos.has(p.id) && p.generation >= 0) {
      pos.set(p.id, { x: left, g: p.generation });
      left += STEP;
    }
  }

  // Ancêtres : écartement qui double à chaque génération qui descend.
  const depth = -minG;
  const placeUp = (child: TreePerson, cx: number, g: number) => {
    const parents = child.parents.map((x) => byId.get(x)).filter((x): x is TreePerson => !!x && x.generation === -g);
    if (!parents.length) return;
    const sep = 2 ** Math.max(0, depth - g) * STEP;
    parents.sort((a, b) => Number(a.female) - Number(b.female));
    const xs = parents.length === 2 ? [cx - sep / 2, cx + sep / 2] : [cx];
    parents.forEach((p, k) => {
      if (pos.has(p.id)) return;
      pos.set(p.id, { x: xs[k] - BW / 2, g: p.generation });
      placeUp(p, xs[k], g + 1);
    });
  };
  const sibXs = siblings.map((s) => pos.get(s.id)!.x + BW / 2);
  const groupCx = sibXs.length ? (Math.min(...sibXs) + Math.max(...sibXs)) / 2 : BW / 2;
  placeUp(root, groupCx, 1);

  // Mise à l'origine.
  const xs = [...pos.values()].map((p) => p.x);
  const minX = Math.min(...xs);
  for (const p of pos.values()) p.x -= minX - 8;
  const maxX = Math.max(...[...pos.values()].map((p) => p.x)) + BW + 8;
  const maxG = Math.max(0, ...gens);
  const height = rowY(maxG) + BH + 8;

  // Traits : entre conjoints, puis des parents vers leurs enfants.
  const at = (id: number) => {
    const p = pos.get(id)!;
    return { x: p.x, y: rowY(p.g) };
  };
  for (const p of t.people) {
    const q = p.partner !== null ? pos.get(p.partner) : undefined;
    if (q && p.id < p.partner! && q.g === p.generation) {
      const a = at(p.id), b = at(p.partner!);
      const [l, r] = a.x < b.x ? [a, b] : [b, a];
      lines.push(`<line class="couple" x1="${l.x + BW}" y1="${l.y + BH / 2}" x2="${r.x}" y2="${r.y + BH / 2}"/>`);
    }
  }
  // Enfants regroupés par couple de parents présents.
  const families = new Map<string, number[]>();
  for (const p of t.people) {
    const ps = p.parents.filter((x) => pos.has(x));
    if (!ps.length || !pos.has(p.id)) continue;
    const key = [...ps].sort((a, b) => a - b).join(",");
    families.set(key, [...(families.get(key) ?? []), p.id]);
  }
  for (const [key, children] of families) {
    const ps = key.split(",").map(Number).map(at);
    const px = ps.reduce((a, p) => a + p.x + BW / 2, 0) / ps.length;
    const py = ps[0].y + (ps.length === 2 ? BH / 2 : BH);
    const cs = children.map(at);
    const bus = cs[0].y - GY / 2;
    const cxs = cs.map((c) => c.x + BW / 2);
    const lo = Math.min(px, ...cxs), hi = Math.max(px, ...cxs);
    lines.push(`<path class="kin" d="M${px},${py} V${bus} M${lo},${bus} H${hi} ${cxs.map((x, k) => `M${x},${bus} V${cs[k].y}`).join(" ")}"/>`);
  }
  const boxes: [TreePerson, number, number][] = t.people.filter((p) => pos.has(p.id)).map((p) => {
    const a = at(p.id);
    return [p, a.x, a.y];
  });
  return { boxes, lines, width: maxX, height };
}
