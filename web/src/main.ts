import "./style.css";
import { Application, Container, Graphics, Sprite, Text, Texture } from "pixi.js";
import { Camera } from "./camera";
import { Heaven } from "./god";
import { connect } from "./net";
import { css, isWater } from "./palette";
import type { ClockView } from "./protocol/ClockView";
import type { GodGauge } from "./protocol/GodGauge";
import type { MapChanges } from "./protocol/MapChanges";
import type { MapInfo } from "./protocol/MapInfo";
import type { RunState } from "./protocol/RunState";
import type { Scene } from "./protocol/Scene";
import type { VillageView } from "./protocol/VillageView";
import type { WeatherMap } from "./protocol/WeatherMap";
import { Pages } from "./pages";
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

// --- Dernière visite : tick où l'on regardait le monde la dernière fois (dans ce navigateur).
const VISIT_KEY = "hameaux.visit";
/** Six heures de jeu d'absence au moins pour qu'on résume ce qui s'est passé. */
const WELCOME_AFTER = 6 * 360;
let previousVisit: number | null = null;
let visitRead = false;

function readVisit() {
  try {
    const v = JSON.parse(localStorage.getItem(VISIT_KEY) ?? "null");
    if (v && v.seed === seed && typeof v.tick === "number" && clock && v.tick <= clock.tick) previousVisit = v.tick;
  } catch {
    // Stockage indisponible (navigation privée…) : pas de résumé, tant pis.
  }
  visitRead = true;
}

function saveVisit() {
  if (!clock || !visitRead) return;
  try {
    localStorage.setItem(VISIT_KEY, JSON.stringify({ seed, tick: clock.tick }));
  } catch {
    // idem
  }
}
setInterval(saveVisit, 15000);
window.addEventListener("pagehide", saveVisit);
document.addEventListener("visibilitychange", () => document.visibilityState === "hidden" && saveVisit());

/** Branchés une fois la carte prête (voir `start`). */
const hooks: {
  scene?: (s: Scene) => void;
  mapRev?: (rev: number) => void;
  events?: (seq: number) => void;
  villages?: (v: VillageView[]) => void;
  weather?: (w: WeatherMap) => void;
  welcome?: () => void;
  god?: (g: GodGauge) => void;
} = {};

const link = connect({
  onMessage(msg) {
    switch (msg.type) {
      case "Scene":
        hooks.scene?.(msg);
        return;
      case "Villages":
        hooks.villages?.(msg.villages);
        hooks.weather?.(msg.weather);
        return;
      case "Hello":
        seed = msg.seed;
        reloadIfOutdated(msg.web_build);
        break;
    }
    clock = msg.clock;
    run = msg.run;
    receivedAt = performance.now();
    if (!visitRead) {
      readVisit();
      hooks.welcome?.();
    }
    hooks.mapRev?.(msg.map_rev);
    hooks.events?.(msg.events_seq);
    hooks.god?.(msg.god);
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
  // Le temps sur la carte : fronts (nuées) et incendies (lueurs), redessinés à chaque image.
  const weatherLayer = new Graphics();
  let weatherMap: WeatherMap | null = null;
  hooks.weather = (w) => (weatherMap = w);
  world.addChild(buildTable(map.w, map.h), overview, details, weatherLayer, scene.container);
  const night = new Graphics();
  const markers = new Container();
  app.stage.addChild(world, night, markers);

  const camera = new Camera(app.canvas, world, map.w, map.h);
  camera.fit();
  window.addEventListener("resize", () => camera.fit());

  // Fiches (habitant, village) et chronique.
  let following: number | null = null;
  let pages: Pages | null = null;
  // Mode dieu : la fiche ouverte est rechargée après chaque acte.
  let sheetRef: Panel | null = null;
  const heaven = new Heaven({
    villageName: (id) => info.villages.find((v) => v.id === id)?.name ?? `village ${id}`,
    done: () => sheetRef?.reload(),
  });
  const sheet = new Panel({
    godButtons: (target, id, extra) => heaven.sheetButtons(target, id, extra),
    power: (key, target, id) => heaven.choose(key, target === "person" ? { person: id } : { village: id }),
    goTo: (x, y) => {
      if (pages?.open) pages.close();
      camera.flyTo(x, y, Math.max(camera.scale, 14));
    },
    follow: (id) => (following = id),
    select: (id) => (scene.selected = id),
  });
  sheetRef = sheet;
  // Signe dans le ciel : un grand éclat de lumière sur tout l'écran.
  let lastSign = -1;
  let signAt = -1e9;
  hooks.god = (g) => {
    if (lastSign >= 0 && g.sign > lastSign) signAt = performance.now();
    lastSign = g.sign;
    heaven.update(g);
  };
  pages = new Pages({
    info,
    openPerson: (id) => sheet.openPerson(id),
    openVillage: (id) => sheet.openVillage(id),
    goTo: (x, y) => camera.flyTo(x, y, Math.max(camera.scale, 14)),
    overview: () => map.overview,
    mapLoaded: () => map.loadedCount,
    lastVisit: () => previousVisit,
    now: () => clock?.tick ?? 0,
  });
  pages.show();
  // De retour après une longue absence : ce qui s'est passé (sauf si l'on arrive sur une page).
  let welcomed = false;
  hooks.welcome = () => {
    if (welcomed || !visitRead || !clock) return;
    welcomed = true;
    if (previousVisit !== null && clock.tick - previousVisit >= WELCOME_AFTER && !pages?.open) {
      pages?.go(`#/visite?since=${previousVisit}`);
    }
  };
  hooks.welcome();
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

  // Villages : repères à taille constante à l'écran + liste cliquable. De nouveaux villages
  // peuvent être fondés en cours de partie : ils apparaissent quand le serveur les annonce.
  const panel = $<HTMLDetailsElement>("villages-panel");
  const narrow = () => window.innerWidth < 600;
  if (narrow()) panel.open = false;
  const villageButtons = new Map<number, HTMLButtonElement>();
  const villageLabel = (v: VillageView) =>
    `<span class="swatch" style="background:${css(v.color)}"></span>${v.name}<span class="muted count">${v.population}</span>`;
  const villageMarkers: { v: VillageView; c: Container; label: Text }[] = [];
  const addVillage = (v: VillageView) => {
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
      if (heaven.targeting) return; // le clic sur la carte désigne la cible (voir pointerup)
      camera.flyTo(v.x + 1, v.y + 1, 14);
      sheet.openVillage(v.id);
    });
    markers.addChild(c);
    villageMarkers.push({ v, c, label });

    const b = document.createElement("button");
    b.type = "button";
    villageButtons.set(v.id, b);
    b.innerHTML = villageLabel(v);
    b.onclick = () => {
      if (heaven.pick({ village: v.id })) return;
      camera.flyTo(v.x + 1, v.y + 1, 14);
      sheet.openVillage(v.id);
      if (narrow()) panel.open = false;
    };
    villagesEl.appendChild(b);
  };
  for (const v of info.villages) addVillage(v);
  hooks.villages = (list) => {
    for (const v of list) {
      const b = villageButtons.get(v.id);
      if (b) {
        b.innerHTML = villageLabel(v);
        // Village renommé par le Ciel.
        const m = villageMarkers.find((x) => x.v.id === v.id);
        if (m && m.label.text !== v.name) {
          m.label.text = v.name;
          m.v.name = v.name;
          const known = info.villages.find((w) => w.id === v.id);
          if (known) known.name = v.name;
        }
      } else {
        addVillage(v);
        if (!info.villages.some((w) => w.id === v.id)) info.villages.push(v);
      }
    }
  };
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
    // Mode dieu : le clic désigne la cible du pouvoir choisi.
    if (heaven.targeting) {
      const t = camera.toTile(e.clientX, e.clientY);
      const want = heaven.targeting.power.target;
      if ((want === "person" || want === "two_people") && hit !== null) heaven.pick({ person: hit });
      else if (want === "village" || want === "two_villages") {
        const reach = Math.max(24, 16 / camera.scale);
        const near = info.villages
          .map((v) => ({ v, d: Math.hypot(v.x + 1 - wx, v.y + 1 - wy) }))
          .filter((x) => x.d <= reach)
          .sort((a, b) => a.d - b.d)[0];
        if (near) heaven.pick({ village: near.v.id });
        else heaven.toast("Aucun village par ici : clique plus près d'un village", false);
      } else if (want === "tile" && map.inside(t.x, t.y)) heaven.pick({ tile: [t.x, t.y] });
      else if (want === "person") heaven.toast("Aucun habitant ici : zoome sur un village et clique sur quelqu'un", false);
      return;
    }
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
    drawWeather(weatherLayer, weatherMap, camera.scale, map.w, map.h);
    map.flush();
    updateDetails();
    // Nuit : voile bleu nuit sur la carte (les repères restent lisibles).
    const dark = darkness(hourOf(estimatedTick()));
    night.clear();
    if (dark > 0) night.rect(0, 0, app.screen.width, app.screen.height).fill({ color: 0x0b1026, alpha: dark * 0.5 });
    // Signe dans le ciel : un éclat doré qui grandit puis s'efface (5 s).
    const since = (performance.now() - signAt) / 1000;
    if (since < 5) {
      const w = app.screen.width, h = app.screen.height;
      const a = since < 0.6 ? since / 0.6 : 1 - (since - 0.6) / 4.4;
      night.rect(0, 0, w, h).fill({ color: 0xfff1c2, alpha: 0.35 * a });
      const cx = w / 2, cy = h * 0.22;
      for (let k = 0; k < 14; k++) {
        const ang = (k / 14) * Math.PI * 2 + since * 0.15;
        const len = Math.max(w, h) * (0.3 + 0.5 * Math.min(1, since / 1.5));
        night.moveTo(cx, cy).lineTo(cx + Math.cos(ang) * len, cy + Math.sin(ang) * len);
      }
      night.stroke({ color: 0xffe08a, width: 18, alpha: 0.18 * a });
      for (const [r, al] of [[260, 0.25], [150, 0.4], [70, 0.7]] as const) night.circle(cx, cy, r * (0.6 + 0.4 * Math.min(1, since))).fill({ color: 0xfff6d8, alpha: al * a });
    }
    document.body.classList.toggle("targeting", !!heaven.targeting);
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

/** Couleur et opacité des fronts (`FrontView.kind` : pluie, orage, temps sec, froid). */
const FRONTS: [number, number][] = [
  [0x8a9ab8, 0.55],
  [0x2a3050, 0.6],
  [0xffe7a0, 0.22],
  [0xd8f2ff, 0.3],
];

/**
 * Fronts et incendies sur la carte (en cases). De loin, les fronts sont bien visibles ; de
 * près, ils s'effacent derrière la pluie et la neige dessinées autour des villages.
 */
function drawWeather(g: Graphics, w: WeatherMap | null, scale: number, mw: number, mh: number) {
  g.clear();
  if (!w) return;
  const t = performance.now() / 1000;
  const fade = scale >= 6 ? 0.25 : scale >= 2 ? 0.6 : 1;
  for (const f of w.fronts) {
    const [color, alpha] = FRONTS[f.kind] ?? FRONTS[0];
    const a = alpha * Math.min(1, f.strength + 0.15) * fade;
    if (a < 0.01 || f.x + f.radius < 0 || f.x - f.radius > mw || f.y + f.radius < 0 || f.y - f.radius > mh) continue;
    // Bords adoucis : trois disques de plus en plus petits.
    for (const k of [1, 0.75, 0.5]) g.circle(f.x, f.y, f.radius * k).fill({ color, alpha: a / 3 });
    // Une ligne d'orages : quelques éclairs de loin en loin.
    if (f.kind === 1 && scale < 6 && (t * 0.8 + f.id) % 1 < 0.05) {
      g.circle(f.x, f.y, f.radius * 0.5).fill({ color: 0xf2f4ff, alpha: 0.25 });
    }
  }
  for (const fire of w.fires) {
    const r = Math.max(2.5, Math.sqrt(fire.burning) * 1.4);
    const p = 0.75 + 0.25 * Math.sin(t * 5 + fire.id);
    g.circle(fire.x, fire.y, r * 2.2).fill({ color: 0xff7a1a, alpha: 0.18 * p });
    g.circle(fire.x, fire.y, r).fill({ color: 0xffb03a, alpha: 0.55 * p });
    g.circle(fire.x, fire.y - r * 1.8, r * 1.5).fill({ color: 0x4a4440, alpha: 0.18 });
  }
}

start().catch((e) => {
  console.error(e);
  clockEl.textContent = `Erreur : ${e}`;
});

/**
 * Le serveur sert une nouvelle version du client (mise à jour pendant que l'onglet était
 * ouvert) : on recharge, sinon l'ancien code lirait des fiches qu'il ne comprend plus.
 */
function reloadIfOutdated(build: string) {
  if (!build) return;
  const mine = [...document.querySelectorAll<HTMLScriptElement>("script[type=module][src]")].map((s) => new URL(s.src).pathname);
  if (mine.some((p) => p.startsWith("/assets/")) && !mine.includes(build)) location.reload();
}
