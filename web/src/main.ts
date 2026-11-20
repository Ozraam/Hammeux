import "./style.css";
import { Application, Container, Graphics, Sprite, Text, Texture } from "pixi.js";
import { Camera } from "./camera";
import { connect } from "./net";
import { css, isWater } from "./palette";
import type { ClockView } from "./protocol/ClockView";
import type { MapChanges } from "./protocol/MapChanges";
import type { MapInfo } from "./protocol/MapInfo";
import type { RunState } from "./protocol/RunState";
import type { Scene } from "./protocol/Scene";
import type { VillageView } from "./protocol/VillageView";
import { Chronicle, Panel } from "./panels";
import { SceneLayer } from "./scene";
import { TABLE_COLOR, buildTable } from "./table";
import { DETAIL_PX, WorldMap } from "./worldmap";

const TICK_SECONDS = 10;
const START_HOUR = 6;
/** À partir de combien de pixels par case on dessine les tronçons en détail. */
const DETAIL_FROM = 3;
const DETAIL_CACHE = 64;

// --- Temps reçu du serveur, extrapolé entre deux messages pour une lumière fluide.
let clock: ClockView | null = null;
let run: RunState = { speed: 60, paused: false };
let receivedAt = 0;
let seed = 0;

function estimatedTick(): number {
  if (!clock) return 0;
  if (run.paused) return clock.tick;
  return clock.tick + ((performance.now() - receivedAt) / 1000) * (run.speed / TICK_SECONDS);
}

function hourOf(tick: number): number {
  return (((tick * TICK_SECONDS + START_HOUR * 3600) % 86400) / 86400) * 24;
}

/** Obscurité de la nuit (0 en plein jour, 1 au cœur de la nuit). */
function darkness(hour: number): number {
  if (hour >= 7.5 && hour <= 18) return 0;
  if (hour > 18 && hour < 21) return (hour - 18) / 3;
  if (hour > 5 && hour < 7.5) return 1 - (hour - 5) / 2.5;
  return 1;
}

// --- Interface (HUD).
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const clockEl = $("clock");
const metaEl = $("meta");
const statusEl = $("status");
const pauseBtn = $<HTMLButtonElement>("pause");
const speedSel = $<HTMLSelectElement>("speed");
const tipEl = $("tip");
const villagesEl = $("villages");
let mapStatus = "";

function renderHud() {
  if (clock) {
    clockEl.textContent = run.paused ? `${clock.label} — en pause` : clock.label;
    pauseBtn.textContent = run.paused ? "Reprendre" : "Pause";
    if (![...speedSel.options].some((o) => Number(o.value) === run.speed)) {
      speedSel.add(new Option(`×${run.speed}`, String(run.speed)));
    }
    speedSel.value = String(run.speed);
  }
  const parts = [`Graine ${seed}`];
  if (clock) parts.push(`tick ${clock.tick.toLocaleString("fr-FR")}`);
  if (mapStatus) parts.push(mapStatus);
  metaEl.textContent = parts.join(" · ");
}

pauseBtn.onclick = () => fetch(run.paused ? "/api/admin/resume" : "/api/admin/pause", { method: "POST" });
speedSel.onchange = () =>
  fetch("/api/admin/speed", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ speed: Number(speedSel.value) }),
  });

/** Branchés une fois la carte prête (voir `start`). */
const hooks: {
  scene?: (s: Scene) => void;
  mapRev?: (rev: number) => void;
  events?: (seq: number) => void;
  villages?: (v: VillageView[]) => void;
} = {};

const link = connect({
  onMessage(msg) {
    switch (msg.type) {
      case "Scene":
        hooks.scene?.(msg);
        return;
      case "Villages":
        hooks.villages?.(msg.villages);
        return;
      case "Hello":
        seed = msg.seed;
        break;
    }
    clock = msg.clock;
    run = msg.run;
    receivedAt = performance.now();
    hooks.mapRev?.(msg.map_rev);
    hooks.events?.(msg.events_seq);
    renderHud();
  },
  onStatus(ok) {
    statusEl.classList.toggle("ok", ok);
    if (!ok) clockEl.textContent = "Connexion perdue, nouvelle tentative…";
  },
});

// --- Carte.
async function start() {
  const app = new Application();
  // Résolution de l'écran (Retina…) pour des textes et repères nets.
  await app.init({
    resizeTo: window,
    antialias: true,
    background: TABLE_COLOR,
    resolution: window.devicePixelRatio || 1,
    autoDensity: true,
  });
  document.body.prepend(app.canvas);

  const info: MapInfo = await (await fetch("/api/map")).json();
  const map = new WorldMap(info);

  const world = new Container();
  // La carte est une feuille posée sur une table (décor sous la carte).
  const overview = new Sprite(map.overviewTexture);
  const details = new Container();
  const scene = new SceneLayer();
  world.addChild(buildTable(map.w, map.h), overview, details, scene.container);
  const night = new Graphics();
  const markers = new Container();
  app.stage.addChild(world, night, markers);

  const camera = new Camera(app.canvas, world, map.w, map.h);
  camera.fit();
  window.addEventListener("resize", () => camera.fit());

  // Fiches (habitant, village) et chronique.
  let following: number | null = null;
  const sheet = new Panel({
    goTo: (x, y) => camera.flyTo(x, y, Math.max(camera.scale, 14)),
    follow: (id) => (following = id),
    select: (id) => (scene.selected = id),
  });
  const chronicle = new Chronicle(sheet);
  hooks.events = (seq) => chronicle.update(seq);
  hooks.scene = (s) => scene.update(s);

  // Carte modifiée (routes, déboisement) : on recharge les tronçons concernés.
  let mapRev = -1;
  let mapBusy = false;
  hooks.mapRev = async (rev) => {
    if (mapRev < 0) {
      mapRev = rev;
      return;
    }
    if (rev <= mapRev || mapBusy) return;
    mapBusy = true;
    try {
      const ch: MapChanges = await (await fetch(`/api/map/changes?since=${mapRev}`)).json();
      for (const [cx, cy] of ch.chunks) {
        await map.reload(cx, cy);
        const old = cache.get(`${cx},${cy}`);
        if (old) {
          old.sprite.destroy({ texture: true, textureSource: true });
          cache.delete(`${cx},${cy}`);
        }
      }
      mapRev = ch.rev;
    } finally {
      mapBusy = false;
    }
  };

  // Villages : repères à taille constante à l'écran + liste cliquable.
  const villageMarkers = info.villages.map((v) => {
    const g = new Graphics();
    g.circle(0, 0, 7).fill(v.color).stroke({ color: 0xffffff, width: 2 });
    const label = new Text({
      text: v.name,
      style: {
        fontFamily: "system-ui, -apple-system, 'Segoe UI', sans-serif",
        fontSize: 13,
        fontWeight: "600",
        lineHeight: 18,
        fill: 0xffffff,
        stroke: { color: 0x10142a, width: 4, join: "round" },
        // Marge autour du texte : sans elle, le contour et le bas des lettres sont rognés.
        padding: 6,
      },
    });
    label.anchor.set(0.5, 0);
    label.y = 10;
    const c = new Container();
    c.addChild(g, label);
    c.eventMode = "static";
    c.cursor = "pointer";
    c.on("pointertap", () => {
      camera.flyTo(v.x + 1, v.y + 1, 14);
      sheet.openVillage(v.id);
    });
    markers.addChild(c);
    return { v, c };
  });
  // Sur téléphone, la liste replie et laisse voir la carte.
  const panel = $<HTMLDetailsElement>("villages-panel");
  const narrow = () => window.innerWidth < 600;
  if (narrow()) panel.open = false;
  const villageButtons = new Map<number, HTMLButtonElement>();
  const villageLabel = (v: VillageView) =>
    `<span class="swatch" style="background:${css(v.color)}"></span>${v.name}<span class="muted count">${v.population}</span>`;
  hooks.villages = (list) => {
    for (const v of list) {
      const b = villageButtons.get(v.id);
      if (b) b.innerHTML = villageLabel(v);
    }
  };
  for (const v of info.villages) {
    const b = document.createElement("button");
    b.type = "button";
    villageButtons.set(v.id, b);
    b.innerHTML = villageLabel(v);
    b.onclick = () => {
      camera.flyTo(v.x + 1, v.y + 1, 14);
      sheet.openVillage(v.id);
      if (narrow()) panel.open = false;
    };
    villagesEl.appendChild(b);
  }
  $("fit").onclick = () => {
    const s = camera.fitScale();
    camera.flyTo(map.w / 2 - 0.5, map.h / 2 - 0.5, s);
  };

  // Infobulle : case sous la souris (ou sous le doigt après un appui sans glisser).
  const showTip = (sx: number, sy: number) => {
    const t = camera.toTile(sx, sy);
    if (!map.inside(t.x, t.y) || !map.isLoaded(Math.floor(t.x / info.chunk_size), Math.floor(t.y / info.chunk_size))) {
      tipEl.hidden = true;
      return;
    }
    const i = t.y * map.w + t.x;
    const L = map.layers;
    const biome = L.biome[i];
    const lines = [`<b>${info.biomes[biome]?.label ?? "?"}</b>`];
    const river = L.flags[i] & 3;
    if (river && !isWater(biome)) lines.push(["", "Ruisseau", "Rivière", "Fleuve"][river]);
    const res = L.resource[i];
    if (res) lines.push(`Ressource : ${info.resources[res]?.label}`);
    if (!isWater(biome)) {
      lines.push(`Altitude : ${Math.max(0, (L.elevation[i] - info.sea_level) * 15)} m`);
      lines.push(`Fertilité : ${Math.round((L.fertility[i] / 255) * 100)} %`);
    }
    lines.push(`<span class="muted">${t.x}, ${t.y}</span>`);
    tipEl.innerHTML = lines.join("<br>");
    tipEl.hidden = false;
  };
  app.canvas.addEventListener("pointermove", (e) => {
    if (e.pointerType === "mouse") showTip(e.clientX, e.clientY);
  });
  app.canvas.addEventListener("pointerleave", () => (tipEl.hidden = true));
  app.canvas.addEventListener("pointerup", (e) => {
    if (camera.dragged) {
      // On a déplacé la carte à la main : on arrête de suivre.
      if (following !== null) sheet.openPerson(following);
      following = null;
      return;
    }
    // Clic sur un habitant : sa fiche.
    const wx = (e.clientX - camera.x) / camera.scale;
    const wy = (e.clientY - camera.y) / camera.scale;
    const hit = camera.scale >= DETAIL_FROM ? scene.pick(wx, wy, Math.max(0.7, 12 / camera.scale)) : null;
    if (hit !== null) sheet.openPerson(hit);
    else if (e.pointerType !== "mouse") showTip(e.clientX, e.clientY);
  });

  // Zone regardée : le serveur n'envoie que ce qui s'y trouve.
  let lastView = "";
  let viewAt = 0;
  const sendView = () => {
    const now = performance.now();
    if (now - viewAt < 250) return;
    const t0 = camera.toTile(0, 0);
    const t1 = camera.toTile(window.innerWidth, window.innerHeight);
    const detail = camera.scale >= DETAIL_FROM;
    const key = `${t0.x},${t0.y},${t1.x},${t1.y},${detail}`;
    if (key === lastView) return;
    lastView = key;
    viewAt = now;
    link.send({ type: "View", x0: t0.x, y0: t0.y, x1: t1.x, y1: t1.y, detail });
    if (!detail) scene.clear();
  };

  // Tronçons détaillés : créés à la demande quand on zoome, gardés dans un petit cache.
  const cache = new Map<string, { sprite: Sprite; used: number }>();
  let frame = 0;
  const updateDetails = () => {
    frame++;
    const show = camera.scale >= DETAIL_FROM;
    details.visible = show;
    if (!show) return;
    const s = info.chunk_size;
    const t0 = camera.toTile(0, 0);
    const t1 = camera.toTile(window.innerWidth, window.innerHeight);
    const cx0 = Math.max(0, Math.floor(t0.x / s)), cy0 = Math.max(0, Math.floor(t0.y / s));
    const cx1 = Math.min(info.chunks_x - 1, Math.floor(t1.x / s)), cy1 = Math.min(info.chunks_y - 1, Math.floor(t1.y / s));
    let budget = 2; // tronçons dessinés par image, pour ne pas saccader
    for (let cy = cy0; cy <= cy1; cy++) {
      for (let cx = cx0; cx <= cx1; cx++) {
        const key = `${cx},${cy}`;
        const hit = cache.get(key);
        if (hit) {
          hit.used = frame;
          hit.sprite.visible = true;
          continue;
        }
        if (budget <= 0 || !map.isLoaded(cx, cy)) continue;
        budget--;
        const sprite = new Sprite(Texture.from(map.renderChunk(cx, cy)));
        sprite.position.set(cx * s, cy * s);
        sprite.scale.set(1 / DETAIL_PX);
        details.addChild(sprite);
        cache.set(key, { sprite, used: frame });
      }
    }
    for (const entry of cache.values()) {
      if (entry.used !== frame) entry.sprite.visible = false;
    }
    if (cache.size > DETAIL_CACHE) {
      const old = [...cache.entries()].sort((a, b) => a[1].used - b[1].used).slice(0, cache.size - DETAIL_CACHE);
      for (const [key, entry] of old) {
        entry.sprite.destroy({ texture: true, textureSource: true });
        cache.delete(key);
      }
    }
  };

  app.ticker.add((ticker) => {
    if (following !== null) {
      const p = scene.positionOf(following);
      if (p) camera.centerOn(p.x, p.y);
    }
    camera.update(ticker.deltaMS / 1000);
    sendView();
    scene.draw(camera.scale);
    map.flush();
    updateDetails();
    // Nuit : voile bleu nuit sur la carte (les repères restent lisibles).
    const dark = darkness(hourOf(estimatedTick()));
    night.clear();
    if (dark > 0) night.rect(0, 0, app.screen.width, app.screen.height).fill({ color: 0x0b1026, alpha: dark * 0.5 });
    // De près, le grenier est visible : on garde seulement le nom, au-dessus du toit.
    const close = camera.scale >= 6;
    for (const { v, c } of villageMarkers) {
      const p = camera.toScreen(v.x + 1, v.y + (close ? -0.3 : 1));
      c.position.set(p.x, p.y - (close ? 30 : 0));
      c.children[0].visible = !close;
    }
  });

  mapStatus = `carte 0/${map.chunkCount}`;
  renderHud();
  await map.loadAll(() => {
    mapStatus = map.loadedCount < map.chunkCount ? `carte ${map.loadedCount}/${map.chunkCount}` : `carte ${map.w}×${map.h}`;
    renderHud();
  });
}

start().catch((e) => {
  console.error(e);
  clockEl.textContent = `Erreur : ${e}`;
});
