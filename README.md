# Hameaux

Un monde de villages simulé en continu sur un serveur, consultable dans le navigateur. Voir [PLAN.md](PLAN.md).

## Structure

- `crates/hameaux-core` : moteur (monde, carte, habitants et leurs esprits, villages, économie, commerce, société, mode dieu et religion, simulation, chronique et histoire, sauvegarde). Ni réseau ni disque.
- `crates/hameaux-worldgen` : génération du monde (relief, rivières, milieux, ressources, villages).
- `crates/hameaux-protocol` : messages serveur ↔ navigateur ; génère `web/src/protocol/*.ts` via `cargo test`.
- `crates/hameaux-server` : boucle de simulation, API HTTP/WebSocket, sauvegardes.
- `crates/hameaux-cli` : aperçu PNG d'un monde, simulation accélérée sans serveur.
- `web/` : client navigateur (TypeScript + PixiJS, Vite).

## Développement

```bash
cargo test                     # tests + régénération des types TypeScript
cd web && npm install && npm run build && cd ..
HAMEAUX_SEED=42 cargo run -p hameaux-server   # http://localhost:8080
```

Pour modifier le client avec rechargement à chaud : laisser tourner le serveur, puis `cd web && npm run dev` (Vite relaie `/ws` et `/api` vers le port 8080).

Aperçu d'un monde sans lancer le serveur (statistiques + image) :

```bash
cargo run --release -p hameaux-cli -- map --seed 42 --out carte.png
cargo run --release -p hameaux-cli -- run --seed 42 --days 336   # 12 ans : population, greniers, humeurs, intentions, commerce, prix, société, histoire, le Ciel, événements
cargo run --release -p hameaux-cli -- run --days 336 --god demo  # … avec un dieu automatique qui essaie chaque pouvoir
cargo run --release -p hameaux-cli -- run --days 30 --god actes.json  # … en rejouant des commandes datées
```

Un script `--god` est une liste JSON de commandes datées par `tick` ou par `day` (fractionnaire) : `[{"day": 1.5, "command": {"power": "rain", "village": 0}}]`. Le journal du serveur (`GET /api/god/deeds`) a la même forme et se rejoue tel quel sur un monde de même graine.

## Variables d'environnement

| Variable | Défaut | Rôle |
|---|---|---|
| `HAMEAUX_BIND` | `0.0.0.0:8080` | adresse d'écoute |
| `HAMEAUX_DATA_DIR` | `./data` (`/data` dans Docker) | sauvegardes |
| `HAMEAUX_WEB_DIR` | `./web/dist` | client compilé |
| `HAMEAUX_SEED` | aléatoire | graine d'un nouveau monde |
| `HAMEAUX_MAP_SIZE` | `1024` | côté de la carte d'un nouveau monde (cases) |
| `HAMEAUX_VILLAGES` | `12` | nombre de villages d'un nouveau monde |
| `HAMEAUX_SPEED` | `60` | secondes de jeu par seconde réelle |
| `HAMEAUX_SAVE_EVERY` | `300` | sauvegarde automatique (s) |
| `HAMEAUX_KEEP_SAVES` / `HAMEAUX_KEEP_DAILY` | `48` / `30` | rotation des sauvegardes |

## API

- `GET /health` : état et tick courant.
- `GET /ws` : WebSocket ; message `Hello` puis `Tick` (≤ 4 par seconde).
- `GET /api/map` : dimensions, libellés des milieux et ressources, villages (`MapInfo`).
- `GET /api/map/changes?since=N` : tronçons modifiés depuis la révision N (routes, défrichement).
- `GET /api/person/{id}`, `GET /api/village/{id}` : fiches.
- `GET /api/events?since=&village=&limit=` : derniers événements (panneau de la chronique).
- `GET /api/chronicle?before=&village=&person=&kind=&q=&annals=&limit=` : chronique complète, du plus récent au plus ancien, par pages (`EventPage`) ; `annals=true` cherche dans les annales (événements marquants, gardés bien plus longtemps).
- `GET /api/village/{id}/history`, `GET /api/world/history` (`?all=true` : toute l'histoire, un relevé par semaine) : relevés quotidiens pour les tableaux de bord (`HistoryView`).
- `GET /api/tree/{id}` : arbre généalogique (`FamilyTree`).
- `GET /api/trade` : carte du commerce — trajets, routes, ventes des marchands entre villages, charrettes en route (`TradeMap`).
- `GET /api/summary?since=TICK` : ce qui s'est passé depuis une date (`Summary`).
- `GET /api/weather` : fronts et incendies en cours, temps de chaque village, bilan des orages, crues, incendies, hivers rudes et brigands (`WeatherReport`). Les fronts et incendies arrivent aussi avec chaque message `Villages`.
- WebSocket : le client envoie `{"type":"View",x0,y0,x1,y1,detail}` ; le serveur répond par des `Scene` (ce qui est visible) et des `Villages`.
- `GET /api/map/chunk/{cx}/{cy}` : tronçon 64×64 en binaire, 6 couches d'un octet par case (altitude, milieu, drapeaux, ressource, fertilité, ombrage), compressé en gzip.
- `GET /api/god` : foi, pouvoirs (coût, cible), derniers actes, religion de chaque village (`GodView`). La jauge de foi arrive aussi avec chaque `Tick`.
- `POST /api/god` avec une commande, par exemple `{"power": "rain", "village": 3}`, `{"power": "bless", "person": 12}`, `{"power": "matchmake", "a": 12, "b": 40}`, `{"power": "road", "from": 1, "to": 4}`, `{"power": "forest", "x": 300, "y": 410}`, `{"power": "deposit", "x": 300, "y": 410, "resource": 2}`, `{"power": "plague", "village": 3, "disease": 0}`, `{"power": "rename", "village": 3, "name": "Clairefont"}`, `{"power": "sign"}`, `{"power": "flood", "village": 3}`, `{"power": "wildfire", "x": 300, "y": 410}` : l'acte (`DeedView`), ou 400 et la raison du refus (cible inconnue, pas assez de foi…). Les 25 pouvoirs sont listés par `GET /api/god`.
- `GET /api/god/deeds` : tout le journal des actes, rejouable par `hameaux-cli run --god`.
- `POST /api/admin/pause`, `/resume`, `/save` ; `POST /api/admin/speed` avec `{"speed": 600}`.

Pages du site (fragment de l'adresse, à partager ou mettre en favori) : `#/monde`, `#/village/{id}`, `#/commerce`, `#/chronique?village=&kind=&person=&q=&annals=1`, `#/arbre/{id}`, `#/visite?since=TICK`, `#/ciel`.

Pas d'authentification : le site n'est accessible que depuis le réseau local ou le VPN.

## Déploiement sur la tour (Docker)

Prérequis sur la tour : Docker avec le plugin compose, rsync, accès SSH depuis le Mac.

```bash
HAMEAUX_HOST=moi@tour ./deploy.sh
```

Le script copie les sources dans `~/apps/hameaux` (autre dossier : `HAMEAUX_REMOTE_DIR=chemin/relatif/au/home`), crée `.env` depuis `.env.example` la première fois (avec ton UID/GID), puis lance `docker compose up -d --build`. Le monde vit dans `~/apps/hameaux/data` (sauvegardes d'environ 1 Mo dans `data/saves/`), que le déploiement ne touche jamais, et survit aux mises à jour : à l'arrêt, le serveur sauvegarde avant de quitter ; au démarrage, il recharge la sauvegarde la plus récente.

Sur la tour, depuis `~/apps/hameaux` :

```bash
docker compose logs -f                         # journaux
cp -r data ~/sauvegarde-hameaux                # copie du monde (ou rsync/restic sur data/)
```
