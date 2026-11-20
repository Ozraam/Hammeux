import { css } from "./palette";
import type { EventView } from "./protocol/EventView";
import type { PersonRef } from "./protocol/PersonRef";
import type { PersonSheet } from "./protocol/PersonSheet";
import type { VillageSheet } from "./protocol/VillageSheet";

export interface PanelActions {
  /** Centre la carte sur une case (et zoome). */
  goTo(x: number, y: number): void;
  /** Suivre (ou non) un habitant avec la caméra. */
  follow(id: number | null): void;
  /** Habitant mis en évidence sur la carte. */
  select(id: number | null): void;
}

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

/**
 * Panneau latéral : fiche d'un habitant ou d'un village, rafraîchie toutes les 2 s.
 * Les noms sont des liens vers d'autres fiches.
 */
export class Panel {
  private readonly el: HTMLElement;
  private current: { kind: "person" | "village"; id: number } | null = null;
  private following: number | null = null;
  private timer = 0;

  constructor(private readonly actions: PanelActions) {
    this.el = document.getElementById("panel")!;
    this.el.addEventListener("click", (e) => this.onClick(e));
  }

  get followed() {
    return this.following;
  }

  openPerson(id: number) {
    this.current = { kind: "person", id };
    this.actions.select(id);
    if (this.following !== null && this.following !== id) this.setFollow(null);
    this.refresh();
  }

  openVillage(id: number) {
    this.current = { kind: "village", id };
    this.actions.select(null);
    this.setFollow(null);
    this.refresh();
  }

  close() {
    this.current = null;
    this.el.hidden = true;
    this.actions.select(null);
    this.setFollow(null);
    clearTimeout(this.timer);
  }

  private setFollow(id: number | null) {
    this.following = id;
    this.actions.follow(id);
  }

  private async refresh() {
    clearTimeout(this.timer);
    const cur = this.current;
    if (!cur) return;
    try {
      const res = await fetch(`/api/${cur.kind}/${cur.id}`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      if (this.current !== cur) return; // une autre fiche a été ouverte entre-temps
      this.el.innerHTML = cur.kind === "person" ? this.personHtml(data) : this.villageHtml(data);
      this.el.hidden = false;
    } catch (e) {
      this.el.innerHTML = `<button class="close" data-close>×</button><p>Fiche indisponible (${esc(String(e))}).</p>`;
      this.el.hidden = false;
    }
    this.timer = window.setTimeout(() => this.refresh(), 2000);
  }

  private onClick(e: MouseEvent) {
    const t = (e.target as HTMLElement).closest<HTMLElement>("[data-person],[data-village],[data-close],[data-follow],[data-goto]");
    if (!t) return;
    if (t.dataset.close !== undefined) this.close();
    else if (t.dataset.person) this.openPerson(Number(t.dataset.person));
    else if (t.dataset.village) this.openVillage(Number(t.dataset.village));
    else if (t.dataset.follow) {
      const id = Number(t.dataset.follow);
      this.setFollow(this.following === id ? null : id);
      this.refresh();
    } else if (t.dataset.goto) {
      const [x, y] = t.dataset.goto.split(",").map(Number);
      this.actions.goTo(x, y);
    }
  }

  private personHtml(p: PersonSheet): string {
    const link = (r: PersonRef) =>
      `<a data-person="${r.id}" class="${r.alive ? "" : "dead"}">${esc(r.name)}</a>${r.note ? ` <span class="muted">· ${esc(r.note)}</span>` : ""}`;
    const list = (items: PersonRef[]) => (items.length ? `<ul>${items.map((r) => `<li>${link(r)}</li>`).join("")}</ul>` : `<p class="muted">—</p>`);
    const following = this.following === p.id;
    const actions = p.alive
      ? `<div class="row">
           <button data-goto="${p.x},${p.y}">Centrer</button>
           <button data-follow="${p.id}" class="${following ? "on" : ""}">${following ? "Ne plus suivre" : "Suivre"}</button>
         </div>`
      : "";
    return `
      <button class="close" data-close aria-label="Fermer">×</button>
      <h2>${esc(p.name)}</h2>
      <p class="sub">${esc(p.role)} · ${p.age} ans · <a data-village="${p.village}">${esc(p.village_name)}</a></p>
      ${p.alive ? `<p class="now">${p.hidden ? "Dort chez lui" + (p.female ? "le" : "") : esc(cap(p.doing))}</p>` : `<p class="now">${esc(cap(p.status))}</p>`}
      ${actions}
      <dl>
        ${p.home ? `<dt>Maison</dt><dd>${esc(p.home)}</dd>` : ""}
        <dt>Naissance</dt><dd>${esc(p.born)}</dd>
        ${p.master ? `<dt>Apprenti${p.female ? "e" : ""} de</dt><dd>${link(p.master)}</dd>` : ""}
      </dl>
      ${p.household ? `<h3>Son foyer</h3>
        <p>${money(p.household.coins)} · ${Math.round(p.household.food_days)} jours de vivres${p.household.hungry ? ` · <b class="warn">a faim</b>` : ""}</p>
        <p class="muted">${p.household.goods.map((g) => `${qty(g.amount)} ${esc(g.label)}`).join(" · ") || "réserves vides"}</p>` : ""}
      <h3>Famille</h3>${list(p.family)}
      <h3>Amis</h3>${list(p.friends)}
      <h3>Sa vie</h3>${this.eventsHtml(p.events, false)}
    `;
  }

  private villageHtml(v: VillageSheet): string {
    const jobs = v.jobs.map(([j, n]) => `${n} ${esc(j)}${n > 1 ? "s" : ""}`).join(", ");
    return `
      <button class="close" data-close aria-label="Fermer">×</button>
      <h2><span class="swatch" style="background:${css(v.color)}"></span>${esc(v.name)}</h2>
      <p class="sub">${esc(v.founded)}</p>
      <div class="row"><button data-goto="${v.x + 1},${v.y + 1}">Aller au village</button></div>
      <dl>
        <dt>Habitants</dt><dd>${v.population} (dont ${v.children} enfants, ${v.elders} aînés)</dd>
        <dt>Métiers</dt><dd>${jobs || "—"}</dd>
        <dt>Maisons</dt><dd>${v.houses} habitées${v.sites ? `, ${v.sites} en chantier` : ""}${v.ruins ? `, ${v.ruins} en ruine` : ""}</dd>
        <dt>Champs</dt><dd>${v.fields}${v.fields_ripe ? ` (${v.fields_ripe} prêts à moissonner)` : ""}</dd>
        <dt>Vivres</dt><dd>${v.food_days} jours (grenier commun : ${Math.round(v.food)} rations)${v.hungry ? ` <b class="warn">disette</b>` : ""}</dd>
        <dt>Chantiers</dt><dd>${v.wood} bois · ${v.has_stone ? `${v.stone} pierre` : "pas de carrière"}</dd>
        <dt>Caisse</dt><dd>${money(v.treasury)}</dd>
        <dt>Ateliers</dt><dd>${v.workshops.length ? v.workshops.map(esc).join(", ") : "aucun"}</dd>
      </dl>
      <h3>Marché</h3>
      <table class="market">
        <tr><th></th><th>prix</th><th>7 j</th><th>vendu</th></tr>
        ${v.market
          .map((r) => {
            const change = r.week_ago > 0 ? r.price / r.week_ago - 1 : 0;
            const arrow = change > 0.05 ? `<span class="up">▲</span>` : change < -0.05 ? `<span class="down">▼</span>` : "";
            return `<tr><td>${esc(r.label)}</td><td class="num">${r.price.toFixed(2)} ${arrow}</td><td>${spark(r.history)}</td><td class="num muted">${r.volume >= 0.5 ? Math.round(r.volume) : "—"}</td></tr>`;
          })
          .join("")}
      </table>
      <h3>Derniers événements</h3>${this.eventsHtml(v.events, false)}
    `;
  }

  eventsHtml(events: EventView[], withVillage: boolean): string {
    if (!events.length) return `<p class="muted">Rien pour l'instant.</p>`;
    return `<ul class="events">${events
      .map((e) => {
        const who = e.people.length ? ` data-person="${e.people[0]}"` : e.village !== null ? ` data-village="${e.village}"` : "";
        const where = withVillage && e.village_name ? `<span class="muted"> · ${esc(e.village_name)}</span>` : "";
        return `<li><a${who}>${esc(e.text)}</a>${where}<br><span class="muted date">${esc(e.date)}</span></li>`;
      })
      .join("")}</ul>`;
  }
}

/**
 * Chronique du monde (derniers événements), mise à jour quand le serveur signale du nouveau.
 */
export class Chronicle {
  private readonly el: HTMLElement;
  private events: EventView[] = [];
  private seq = -1;
  private loading = false;

  constructor(private readonly panel: Panel) {
    this.el = document.getElementById("chronicle")!;
    this.el.addEventListener("click", (e) => {
      const t = (e.target as HTMLElement).closest<HTMLElement>("[data-person],[data-village]");
      if (t?.dataset.person) panel.openPerson(Number(t.dataset.person));
      else if (t?.dataset.village) panel.openVillage(Number(t.dataset.village));
    });
  }

  /** Le serveur annonce que la chronique va jusqu'à `seq` (exclu). */
  async update(seq: number) {
    if (seq === this.seq || this.loading) return;
    this.loading = true;
    try {
      const since = this.events.length ? this.events[this.events.length - 1].seq + 1 : 0;
      const res = await fetch(`/api/events?since=${since}&limit=40`);
      const fresh: EventView[] = await res.json();
      this.events = [...this.events, ...fresh].slice(-40);
      this.seq = seq;
      this.el.innerHTML = this.panel.eventsHtml([...this.events].reverse(), true);
    } finally {
      this.loading = false;
    }
  }
}

function money(c: number) {
  return `${Math.round(c)} sou${Math.round(c) > 1 ? "s" : ""}`;
}

function qty(n: number) {
  return n >= 10 ? Math.round(n).toString() : n.toFixed(1).replace(".0", "");
}

/** Petite courbe des prix (SVG). */
function spark(values: number[]): string {
  if (values.length < 2) return "";
  const w = 64, h = 16;
  const min = Math.min(...values), max = Math.max(...values);
  const span = max - min || 1;
  const pts = values.map((v, i) => `${((i / (values.length - 1)) * w).toFixed(1)},${(h - 1 - ((v - min) / span) * (h - 2)).toFixed(1)}`);
  return `<svg class="spark" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}"><polyline points="${pts.join(" ")}" /></svg>`;
}

function cap(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
