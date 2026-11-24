// Mode dieu (phase 8) : jauge de foi, palette de pouvoirs, choix de la cible sur la carte.
//
// Un pouvoir se choisit dans la palette (ou dans la fiche d'un habitant ou d'un village) ;
// s'il lui faut une cible, un bandeau demande de la désigner : clic sur un village, un
// habitant ou une case de la carte. La commande part alors au serveur (`POST /api/god`) ;
// le résultat s'affiche dans un petit message, et dans la chronique.

import { esc } from "./panels";
import type { GodGauge } from "./protocol/GodGauge";
import type { GodView } from "./protocol/GodView";
import type { PowerView } from "./protocol/PowerView";

/** Ce que désigne un clic pendant le choix d'une cible. */
export type Pick = { village: number } | { person: number } | { tile: [number, number] };

interface Targeting {
  power: PowerView;
  /** Champs déjà réunis pour la commande. */
  fields: Record<string, unknown>;
  /** Deuxième cible attendue (rencontre, route). */
  second: boolean;
}

export interface HeavenActions {
  /** Nom d'un habitant ou d'un village (pour le bandeau). */
  villageName(id: number): string;
  /** Après un acte réussi : rafraîchir ce qui doit l'être (fiche ouverte…). */
  done(): void;
}

const ASK: Record<string, string> = {
  world: "",
  village: "clique sur un village (sur la carte ou dans la liste)",
  person: "clique sur un habitant (zoome sur un village pour les voir)",
  two_people: "clique sur une première personne",
  two_villages: "clique sur le premier village",
  tile: "clique sur une case de la carte",
};

export class Heaven {
  gauge: GodGauge = { faith: 0, max: 100, regen: 0, sign: 0, deeds: 0 };
  view: GodView | null = null;
  targeting: Targeting | null = null;
  private readonly hud = document.getElementById("faith")!;
  private readonly palette = document.getElementById("powers")!;
  private readonly bar = document.getElementById("target-bar")!;
  private readonly toastEl = document.getElementById("toast")!;
  private toastTimer = 0;
  private loading = false;
  private seenDeeds = -1;

  constructor(private readonly a: HeavenActions) {
    this.hud.addEventListener("click", (e) => {
      if ((e.target as Element).closest("[data-palette]")) this.togglePalette();
    });
    this.palette.addEventListener("click", (e) => {
      const t = (e.target as Element).closest<HTMLElement>("[data-power],[data-close]");
      if (!t) return;
      if (t.dataset.close !== undefined) this.togglePalette(false);
      else if (t.dataset.power) this.choose(t.dataset.power);
    });
    this.bar.addEventListener("click", (e) => {
      const t = (e.target as Element).closest<HTMLElement>("[data-cancel],[data-go]");
      if (t?.dataset.cancel !== undefined) this.cancel();
      else if (t?.dataset.go !== undefined) this.submitName();
    });
    this.bar.addEventListener("keydown", (e) => {
      if (e.key === "Enter") this.submitName();
    });
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && this.targeting) {
        e.stopImmediatePropagation();
        this.cancel();
      }
    }, true);
    this.load();
  }

  /** Nouvelle jauge reçue avec un `Tick`. */
  update(g: GodGauge) {
    this.gauge = g;
    this.renderHud();
    if (!this.palette.hidden) this.renderPalette();
    if (g.deeds !== this.seenDeeds) {
      this.seenDeeds = g.deeds;
      this.load();
    }
  }

  async load() {
    if (this.loading) return;
    this.loading = true;
    try {
      this.view = await (await fetch("/api/god")).json();
      this.renderPalette();
    } catch {
      // Serveur injoignable : on réessaiera au prochain acte.
    } finally {
      this.loading = false;
    }
  }

  power(key: string): PowerView | undefined {
    return this.view?.powers.find((p) => p.key === key);
  }

  private renderHud() {
    const g = this.gauge;
    const pct = Math.max(0, Math.min(100, (g.faith / g.max) * 100));
    this.hud.innerHTML = `
      <div class="faith-row">
        <span class="faith-label" title="La foi se recharge chaque jour, et plus vite quand on prie">Foi</span>
        <span class="faith-bar"><i style="width:${pct.toFixed(1)}%"></i></span>
        <span class="faith-num">${Math.floor(g.faith)}<span class="muted">/${Math.round(g.max)}</span></span>
      </div>
      <div class="faith-row">
        <span class="muted">+${Math.round(g.regen)} par jour</span>
        <button type="button" data-palette class="${this.palette.hidden ? "" : "on"}">✦ Pouvoirs</button>
      </div>`;
  }

  togglePalette(open = this.palette.hidden) {
    this.palette.hidden = !open;
    if (open) {
      this.renderPalette();
      this.load();
    }
    this.renderHud();
  }

  private renderPalette() {
    const v = this.view;
    if (!v) {
      this.palette.innerHTML = `<p class="muted">Chargement…</p>`;
      return;
    }
    const groups = new Map<string, PowerView[]>();
    for (const p of v.powers) groups.set(p.category, [...(groups.get(p.category) ?? []), p]);
    const faith = this.gauge.faith;
    this.palette.innerHTML = `
      <header><b>Pouvoirs du Ciel</b><span class="muted">foi ${Math.floor(faith)}</span>
        <button class="close" data-close aria-label="Fermer">×</button></header>
      ${[...groups.entries()]
        .map(
          ([cat, list]) => `<h4>${esc(cat)}</h4><div class="power-grid">${list
            .map((p) => {
              const on = this.targeting?.power.key === p.key;
              return `<button type="button" data-power="${p.key}" class="power ${p.benign ? "good" : "bad"}${on ? " on" : ""}" ${faith < p.cost ? "disabled" : ""} title="${esc(p.help)}">
                <span>${esc(p.label)}</span><span class="cost">${p.cost}</span></button>`;
            })
            .join("")}</div>`,
        )
        .join("")}
      <p class="links-row"><a href="#/ciel">Religion et journal des actes →</a></p>`;
  }

  /** Choisit un pouvoir ; `target` déjà connue (depuis une fiche) l'applique tout de suite. */
  choose(key: string, target?: Pick) {
    const power = this.power(key);
    if (!power) return;
    if (this.gauge.faith < power.cost) {
      this.toast(`Pas assez de foi pour ${power.label.toLowerCase()} (${power.cost} demandés)`, false);
      return;
    }
    this.targeting = { power, fields: {}, second: false };
    if (power.target === "world") {
      this.send({ power: key });
      return;
    }
    if (target) {
      this.pick(target);
      if (this.targeting) this.renderBar();
      return;
    }
    if (window.innerWidth < 700) this.togglePalette(false);
    this.renderBar();
    this.renderPalette();
  }

  /** Le joueur a désigné quelque chose : vrai si c'était une cible attendue. */
  pick(p: Pick): boolean {
    const t = this.targeting;
    if (!t) return false;
    const want = t.power.target;
    const key = t.power.key;
    if ("village" in p && (want === "village" || want === "two_villages")) {
      if (want === "two_villages") {
        if (!t.second) {
          t.fields.from = p.village;
          t.second = true;
          this.renderBar();
          return true;
        }
        if (p.village === t.fields.from) return true;
        t.fields.to = p.village;
      } else t.fields.village = p.village;
      if (key === "rename") {
        this.renderBar();
        return true;
      }
    } else if ("person" in p && (want === "person" || want === "two_people")) {
      if (want === "two_people") {
        if (!t.second) {
          t.fields.a = p.person;
          t.second = true;
          this.renderBar();
          return true;
        }
        if (p.person === t.fields.a) return true;
        t.fields.b = p.person;
      } else t.fields.person = p.person;
    } else if ("tile" in p && want === "tile") {
      t.fields.x = p.tile[0];
      t.fields.y = p.tile[1];
    } else return false;
    this.send({ power: key, ...t.fields, ...this.extra() });
    return true;
  }

  /** Paramètres choisis dans le bandeau (maladie, ressource). */
  private extra(): Record<string, unknown> {
    const sel = this.bar.querySelector<HTMLSelectElement>("select[name=extra]");
    if (!sel) return {};
    const key = this.targeting?.power.key;
    return key === "plague" ? { disease: Number(sel.value) } : key === "deposit" ? { resource: Number(sel.value) } : {};
  }

  private submitName() {
    const t = this.targeting;
    const input = this.bar.querySelector<HTMLInputElement>("input[name=name]");
    if (!t || !input || t.fields.village === undefined) return;
    const name = input.value.trim();
    if (name) this.send({ power: "rename", village: t.fields.village, name });
  }

  private renderBar() {
    const t = this.targeting;
    if (!t) {
      this.bar.hidden = true;
      return;
    }
    const v = this.view;
    let ask = ASK[t.power.target] ?? "";
    if (t.second) ask = t.power.target === "two_people" ? "clique maintenant sur la seconde personne" : "clique maintenant sur le second village";
    let extra = "";
    if (t.power.key === "plague" && v) {
      extra = `<select name="extra" aria-label="Maladie">${v.diseases.map((d) => `<option value="${d.id}">${esc(d.label)}</option>`).join("")}</select>`;
    } else if (t.power.key === "deposit" && v) {
      const iron = v.resources.find((r) => r.label.toLowerCase().includes("fer"))?.id;
      extra = `<select name="extra" aria-label="Ressource">${v.resources.map((r) => `<option value="${r.id}" ${r.id === iron ? "selected" : ""}>${esc(r.label)}</option>`).join("")}</select>`;
    }
    if (t.power.key === "rename" && t.fields.village !== undefined) {
      const old = this.a.villageName(t.fields.village as number);
      ask = `nouveau nom pour ${esc(old)} :`;
      extra = `<input name="name" maxlength="32" value="${esc(old)}" aria-label="Nouveau nom"><button type="button" data-go>Renommer</button>`;
    }
    this.bar.innerHTML = `<b>${esc(t.power.label)}</b> <span class="muted">(${t.power.cost} de foi)</span> — ${ask} ${extra}
      <button type="button" data-cancel>Annuler</button>`;
    this.bar.hidden = false;
    const input = this.bar.querySelector<HTMLInputElement>("input[name=name]");
    if (input) {
      input.focus();
      input.select();
    }
  }

  cancel() {
    this.targeting = null;
    this.bar.hidden = true;
    if (!this.palette.hidden) this.renderPalette();
  }

  private async send(cmd: Record<string, unknown>) {
    this.targeting = null;
    this.bar.hidden = true;
    try {
      const res = await fetch("/api/god", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(cmd),
      });
      const text = await res.text();
      if (res.ok) {
        const deed = JSON.parse(text);
        this.toast(deed.text, true);
        this.gauge = { ...this.gauge, faith: this.gauge.faith - deed.cost };
        this.renderHud();
        this.load();
        this.a.done();
      } else this.toast(text, false);
    } catch (e) {
      this.toast(`Le Ciel ne répond pas (${e})`, false);
    }
    if (!this.palette.hidden) this.renderPalette();
  }

  toast(text: string, ok: boolean) {
    clearTimeout(this.toastTimer);
    this.toastEl.textContent = text;
    this.toastEl.className = ok ? "ok" : "bad";
    this.toastEl.hidden = false;
    this.toastTimer = window.setTimeout(() => (this.toastEl.hidden = true), ok ? 5000 : 6000);
  }

  /** Boutons de pouvoirs pour une fiche (habitant ou village). */
  sheetButtons(target: "person" | "village", id: number, extra: { sick?: boolean } = {}): string {
    const v = this.view;
    if (!v) return "";
    const list = v.powers.filter((p) => {
      if (target === "person") return p.target === "person" && (p.key !== "heal" || extra.sick);
      return p.target === "village";
    });
    const buttons = list
      .map(
        (p) =>
          `<button type="button" class="chip ${p.benign ? "good" : "bad"}" data-power="${p.key}" data-target="${target}:${id}" ${this.gauge.faith < p.cost ? "disabled" : ""} title="${esc(p.help)} (${p.cost} de foi)">${esc(p.label)}</button>`,
      )
      .join("");
    const meet = target === "person" ? `<button type="button" class="chip good" data-power="matchmake" data-target="person:${id}" title="Choisis ensuite l'autre personne">Faire se rencontrer…</button>` : "";
    return `<details class="god-powers"><summary>Pouvoirs du Ciel</summary><div class="chips">${buttons}${meet}</div></details>`;
  }
}
