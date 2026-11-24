import { css } from "./palette";
import type { EventView } from "./protocol/EventView";
import type { PersonRef } from "./protocol/PersonRef";
import type { CultView } from "./protocol/CultView";
import type { PersonSheet } from "./protocol/PersonSheet";
import type { VillageSheet } from "./protocol/VillageSheet";

export interface PanelActions {
  /** Centre la carte sur une case (et zoome). */
  goTo(x: number, y: number): void;
  /** Suivre (ou non) un habitant avec la caméra. */
  follow(id: number | null): void;
  /** Habitant mis en évidence sur la carte. */
  select(id: number | null): void;
  /** Boutons des pouvoirs du Ciel pour cette fiche. */
  godButtons(target: "person" | "village", id: number, extra?: { sick?: boolean }): string;
  /** Un pouvoir choisi depuis la fiche, sur sa cible. */
  power(key: string, target: "person" | "village", id: number): void;
}

export const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

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

  /** Recharge la fiche ouverte tout de suite (après un acte du Ciel). */
  reload() {
    if (this.current) this.refresh();
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
      // Les sections dépliées le restent d'un rafraîchissement à l'autre.
      const opened = [...this.el.querySelectorAll<HTMLDetailsElement>("details[open]")].map((d) => d.className);
      this.el.innerHTML = cur.kind === "person" ? this.personHtml(data) : this.villageHtml(data);
      for (const d of this.el.querySelectorAll<HTMLDetailsElement>("details")) d.open = opened.includes(d.className);
      this.el.hidden = false;
    } catch (e) {
      this.el.innerHTML = `<button class="close" data-close>×</button><p>Fiche indisponible (${esc(String(e))}).</p>`;
      this.el.hidden = false;
    }
    this.timer = window.setTimeout(() => this.refresh(), 2000);
  }

  private onClick(e: MouseEvent) {
    const t = (e.target as HTMLElement).closest<HTMLElement>("[data-power],[data-person],[data-village],[data-close],[data-follow],[data-goto]");
    if (!t) return;
    if (t.dataset.power) {
      const [kind, id] = (t.dataset.target ?? "").split(":");
      this.actions.power(t.dataset.power, kind as "person" | "village", Number(id));
    } else if (t.dataset.close !== undefined) this.close();
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
    const pages = `<p class="links-row"><a href="#/arbre/${p.id}">Arbre généalogique</a> · <a href="#/chronique?person=${p.id}">Sa chronique</a></p>`;
    return `
      <button class="close" data-close aria-label="Fermer">×</button>
      <h2>${esc(p.name)}</h2>
      <p class="sub">${esc(p.role)} · ${p.age} ans · <a data-village="${p.village}">${esc(p.village_name)}</a></p>
      ${p.office ? `<p class="office">${esc(cap(p.office))}</p>` : ""}
      ${p.alive ? `<p class="now">${p.hidden ? "Dort chez lui" + (p.female ? "le" : "") : esc(cap(p.doing))}</p>` : `<p class="now">${esc(cap(p.status))}</p>`}
      ${p.alive ? `<p class="mood"><span class="dot" style="background:${moodColor(p.mood_value)}"></span>${esc(cap(p.mood))}</p>${p.traits.length ? `<p class="traits muted">${esc(cap(p.traits.join(", ")))}</p>` : ""}` : ""}
      ${p.health ? `<p class="sick">${esc(cap(p.health))}</p>` : ""}
      ${actions}
      ${p.alive ? this.actions.godButtons("person", p.id, { sick: !!p.health }) : ""}
      ${pages}
      <dl>
        ${p.home ? `<dt>Maison</dt><dd>${esc(p.home)}</dd>` : ""}
        <dt>Naissance</dt><dd>${esc(p.born)}</dd>
        ${p.master ? `<dt>Apprenti${p.female ? "e" : ""} de</dt><dd>${link(p.master)}</dd>` : ""}
        ${p.skill ? `<dt>Métier</dt><dd>${esc(p.skill.rank)} ${meter(p.skill.value)}</dd>` : ""}
        ${p.alive ? `<dt>Réputation</dt><dd>${esc(p.reputation_label)}</dd>` : ""}
        <dt>Le Ciel</dt><dd class="${p.belief >= 0.25 ? "belief" : "muted"}">${esc(p.belief_label)}</dd>
        ${p.convictions ? `<dt>Justice</dt><dd>condamné${p.female ? "e" : ""} ${p.convictions === 1 ? "une fois" : `${p.convictions} fois`}</dd>` : ""}
        ${p.goals.length ? `<dt>Rêve${p.goals.length > 1 ? "s" : ""}</dt><dd>${p.goals.map((g) => (g.reached ? `<s class="muted">${esc(g.label)}</s> ✓` : esc(g.label))).join("<br>")}</dd>` : ""}
      </dl>
      ${p.personality.length ? `<details class="personality"><summary>Caractère</summary><div class="traits-grid">${p.personality
        .map((t) => `<span class="lo${t.value < 0.38 ? " on" : ""}">${esc(t.low)}</span><span class="bipolar"><i style="left:${Math.round(t.value * 100)}%"></i></span><span class="hi${t.value > 0.62 ? " on" : ""}">${esc(t.high)}</span>`)
        .join("")}</div></details>` : ""}
      ${p.alive ? `<h3>Besoins</h3><div class="needs">${p.needs.map((n) => `<span>${esc(n.label)}</span>${meter(n.value)}`).join("")}</div>` : ""}
      ${p.household ? `<h3>Son foyer</h3>
        <p>${money(p.household.coins)}${p.household.debt >= 1 ? ` (doit ${money(p.household.debt)} à la caisse)` : ""} · ${Math.round(p.household.food_days)} jours de vivres${p.household.hungry ? ` · <b class="warn">a faim</b>` : ""}${p.household.fields ? ` · cultive ${p.household.fields} champ${p.household.fields > 1 ? "s" : ""}` : ""}</p>
        <p class="muted">${p.household.goods.map((g) => `${qty(g.amount)} ${esc(g.label)}`).join(" · ") || "réserves vides"}</p>` : ""}
      <h3>Famille</h3>${list(p.family)}
      <h3>Relations</h3>${list(p.relations)}
      ${p.memories.length ? `<h3>Souvenirs</h3><ul class="memories">${p.memories
        .map((m) => `<li class="${m.weight >= 0 ? "good" : "bad"}">${m.who !== null ? `<a data-person="${m.who}">${esc(m.text)}</a>` : esc(m.text)} <span class="muted date">· ${esc(m.date)}</span></li>`)
        .join("")}</ul>` : ""}
      <h3>Sa vie</h3>${this.eventsHtml(p.events, false)}
    `;
  }

  private villageHtml(v: VillageSheet): string {
    const jobs = v.jobs.map(([j, n]) => `${n} ${esc(j)}${n > 1 ? "s" : ""}`).join(", ");
    return `
      <button class="close" data-close aria-label="Fermer">×</button>
      <h2><span class="swatch" style="background:${css(v.color)}"></span>${esc(v.name)}</h2>
      <p class="sub">${esc(v.founded)}${v.founded_from ? ` par des familles de <a data-village="${v.founded_from.id}">${esc(v.founded_from.name)}</a>` : ""}</p>
      <div class="row"><button data-goto="${v.x + 1},${v.y + 1}">Aller au village</button></div>
      ${this.actions.godButtons("village", v.id)}
      <p class="links-row"><a href="#/village/${v.id}">Tableau de bord</a> · <a href="#/chronique?village=${v.id}">Chronique du village</a></p>
      <dl>
        <dt>Habitants</dt><dd>${v.population} (dont ${v.children} enfants, ${v.elders} aînés)</dd>
        <dt>Métiers</dt><dd>${jobs || "—"}</dd>
        <dt>Maisons</dt><dd>${v.houses} habitées${v.sites ? `, ${v.sites} en chantier` : ""}${v.ruins ? `, ${v.ruins} en ruine` : ""}</dd>
        <dt>Champs</dt><dd>${v.fields}${v.fields_ripe ? ` (${v.fields_ripe} prêts à moissonner)` : ""}${v.fields > v.land.owned ? `, ${v.fields - v.land.owned} au village` : ""}</dd>
        <dt>Vivres</dt><dd>${v.food_days} jours (grenier commun : ${Math.round(v.food)} rations)${v.hungry ? ` <b class="warn">disette</b>` : ""}</dd>
        <dt>Chantiers</dt><dd>${v.wood} bois · ${v.has_stone ? `${v.stone} pierre` : "pas de carrière"}</dd>
        <dt>Caisse</dt><dd>${money(v.treasury)} · taxe ${Math.round(v.tax * 100)} %</dd>
        <dt>Santé</dt><dd>${v.epidemic ? `<b class="warn">${esc(v.epidemic)}</b> · ` : ""}${v.sick ? `${v.sick} malade${v.sick > 1 ? "s" : ""}` : "personne n'est malade"}</dd>
        ${v.festivities.length ? `<dt>Fêtes</dt><dd>${v.festivities.map(esc).join("<br>")}</dd>` : ""}
        <dt>Ateliers</dt><dd>${v.workshops.length ? v.workshops.map(esc).join(", ") : "aucun"}</dd>
      </dl>
      ${landHtml(v)}
      ${v.cult ? cultHtml(v.cult) : ""}
      <h3>Gouvernement</h3>
      ${v.chief
        ? `<p><a data-person="${v.chief.id}">${esc(v.chief.name)}</a> <span class="muted">· ${esc(v.chief.note)}</span>${v.council.length ? `<br><span class="muted">conseil :</span> ${v.council.map((r) => `<a data-person="${r.id}">${esc(r.name)}</a>`).join(", ")}` : ""}<br><span class="muted">${esc(v.next_election)}</span></p>`
        : `<p class="muted">${esc(v.next_election)}</p>`}
      ${v.techs.length ? `<h3>Savoirs</h3><ul class="techs">${v.techs
        .map((t) => `<li><b>${esc(cap(t.label))}</b> <span class="muted">· ${esc(t.effect)}</span><br><span class="muted date">${t.by !== null ? `<a data-person="${t.by}">${esc(t.origin)}</a>` : t.from ? `<a data-village="${t.from.id}">${esc(t.origin)}</a>` : esc(t.origin)}</span></li>`)
        .join("")}</ul>` : ""}
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
      ${v.merchants.length ? `<h3>Marchands</h3><ul>${v.merchants.map((r) => `<li><a data-person="${r.id}">${esc(r.name)}</a> <span class="muted">· ${esc(r.note)}</span></li>`).join("")}</ul>` : ""}
      ${v.links.length ? `<h3>Autres villages</h3><table class="links">
        <tr><th></th><th>trajet</th><th>nouvelles</th></tr>
        ${v.links
          .map(
            (l) => `<tr><td><a data-village="${l.village.id}">${esc(l.village.name)}</a>${l.dearer.length ? `<br><span class="muted">plus cher là-bas : ${l.dearer.map(esc).join(", ")}</span>` : ""}</td>
              <td>${l.way ? `${esc(l.way)}${l.trips ? ` <span class="muted">(${l.trips})</span>` : ""}` : "—"}</td>
              <td class="muted">${esc(l.news) || "aucune"}</td></tr>`,
          )
          .join("")}
      </table>` : ""}
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

/** Religion du village et effets du Ciel en cours (fiche de village). */
function cultHtml(c: CultView): string {
  const priest = c.priest ? `<a data-person="${c.priest.id}">${esc(c.priest.name)}</a> <span class="muted">· ${esc(c.priest.note)}</span>` : "";
  const lines = [
    c.name ? `<b>${esc(cap(c.name))}</b> <span class="muted">· ${esc(c.founded ?? "")}</span>` : `<span class="muted">Pas encore de culte.</span>`,
    `Le village ${esc(c.attitude)} · ${c.believers} croyant${c.believers > 1 ? "s" : ""} sur ${c.population}`,
    c.miracles || c.scourges ? `<span class="muted">${c.miracles} bienfait${c.miracles > 1 ? "s" : ""}, ${c.scourges} fléau${c.scourges > 1 ? "x" : ""}</span>` : "",
    priest,
    c.temple ? `Temple : ${esc(c.temple)}` : "",
    ...c.spells.map((s) => `<span class="belief">✦ ${esc(cap(s))}</span>`),
  ].filter(Boolean);
  return `<h3>Le Ciel</h3><p>${lines.join("<br>")}</p>`;
}

export function money(c: number) {
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

/** Petite jauge 0–1. */
function meter(v: number): string {
  const pct = Math.round(Math.max(0, Math.min(1, v)) * 100);
  const cls = v < 0.3 ? "low" : v < 0.6 ? "mid" : "";
  return `<span class="meter ${cls}" title="${pct} %"><i style="width:${pct}%"></i></span>`;
}

/** Couleur de l'humeur : rouge (au plus mal) → jaune → vert (radieux). */
function moodColor(v: number): string {
  const t = Math.max(0, Math.min(1, (v + 0.6) / 1.4));
  return `hsl(${Math.round(t * 120)}, 65%, 55%)`;
}

export function cap(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** Terres, gibier et poisson, année, taille et aumônes, argent des foyers (phase 9). */
function landHtml(v: VillageSheet): string {
  const l = v.land;
  const soil = l.soil >= 1.05 ? "bien fumées" : l.soil >= 0.9 ? "en bon état" : l.soil >= 0.7 ? "fatiguées" : "épuisées";
  const wild = (x: number) => (x >= 0.75 ? "abondant" : x >= 0.45 ? "moins abondant" : x >= 0.25 ? "rare" : "presque disparu");
  return `
      <h3>Terres et argent</h3>
      <dl>
        <dt>Terres</dt><dd>${soil} ${meter(Math.min(1, l.soil / 1.1))}${l.worn ? ` <span class="warn">${l.worn} champ${l.worn > 1 ? "s" : ""} épuisé${l.worn > 1 ? "s" : ""}</span>` : ""}</dd>
        <dt>Cette année</dt><dd>${esc(l.year)}</dd>
        <dt>Gibier</dt><dd>${wild(l.game)} ${meter(l.game)}</dd>
        <dt>Poisson</dt><dd>${wild(l.fish)} ${meter(l.fish)}</dd>
        <dt>Taille</dt><dd>${money(l.levied)} levés depuis le printemps${l.alms >= 1 ? ` · ${money(l.alms)} d'aumônes` : ""}</dd>
        <dt>Foyers</dt><dd>${money(l.coins_poor)} chez les plus pauvres · ${money(l.coins_median)} au milieu · ${money(l.coins_rich)} chez les plus riches</dd>
        ${l.smithies > 1 ? `<dt>Forges</dt><dd>${l.smithies}</dd>` : ""}
      </dl>`;
}
