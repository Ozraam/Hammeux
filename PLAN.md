# Hameaux — plan

Reprise du village du launcher TV (`tv/launcher/app/.../village/`, voir `VILLAGE.md` là-bas) en beaucoup plus grand : un monde avec plusieurs villages, une économie et des villageois plus riches, qui tourne en continu sur le serveur et se consulte dans un navigateur.

## Ce qu'on garde du village TV

- Grille de cases, A*, chantiers en plusieurs étapes (piquetage → toit), champs (labour → récolte), déclin (abandon → ruine → végétation).
- Familles : couples, naissances, enfants, métier hérité, apprentissage, déménagements, amitiés, retraite, départs groupés en disette.
- Lien avec le réel : jour/nuit, saisons, météo (Open-Meteo), journal d'événements, fêtes, incendies.
- Les tests d'équilibre (simuler N jours sans écran et vérifier que le village tient).

Ce qui change : le moteur ne dessine plus rien. Il tourne seul sur le serveur, et le navigateur n'est qu'une fenêtre sur lui. Il n'y a plus de rattrapage au retour, puisque la simulation ne s'arrête jamais (sauf redémarrage : on recharge la dernière sauvegarde).

## Architecture

```
hameaux/                       (workspace Cargo)
├── crates/
│   ├── hameaux-core/          moteur pur : monde, agents, économie. Aucun réseau, aucun disque.
│   ├── hameaux-worldgen/      génération du monde (bruit, rivières, biomes, placement des villages)
│   ├── hameaux-server/        binaire : boucle de simulation + API web + sauvegardes
│   ├── hameaux-cli/           simulation sans serveur (N années en accéléré, rapports, équilibrage)
│   └── hameaux-protocol/      types échangés avec le navigateur (sérialisation, versions)
└── web/                       client navigateur
```

**Moteur (`hameaux-core`)**
- Données en structures Rust simples (décidé en phase 2, à la place de `bevy_ecs`) : habitants dans un tableau indexé par identifiant (jamais supprimés, pour garder l'histoire des familles), villages avec leurs maisons et champs, systèmes écrits comme des fonctions appelées dans un ordre fixe. Sérialisation directe avec serde, déterminisme facile. On pourra passer à un ECS si les performances l'exigent (mesuré : ~130 000 ticks/s avec 500 habitants).
- Pas fixe : 1 tick = 1 minute de jeu. Les villageois ne « réfléchissent » pas à chaque tick : chacun a son prochain instant de décision, ce qui étale la charge.
- Déterministe : une graine pour le monde, un générateur aléatoire par système et par tick (`rand_chacha`). Même graine + mêmes entrées = même histoire, ce qui permet de rejouer et de déboguer.
- Échelle visée : carte 1024×1024 cases découpée en tronçons de 64×64, 8 à 15 villages au départ, ~5 000 habitants (jusqu'à ~10 000 si les mesures le permettent). Budget : < 20 ms de calcul par seconde réelle. Le Ryzen 5 2600 (6 cœurs / 12 fils) laisse de la marge : les systèmes indépendants tournent en parallèle (`bevy_ecs` + `rayon`), la carte graphique n'est pas utilisée.
- Chemins : A* hiérarchique (sur les tronçons) + cache des trajets fréquents (maison ↔ champ, village ↔ village). Les routes naissent là où l'on passe souvent, comme sur la TV, mais entre villages aussi.

**Serveur (`hameaux-server`)**
- `tokio` + `axum`. La simulation tourne sur son propre fil ; après chaque tick elle publie un instantané en lecture seule (`arc-swap`) que l'API lit sans jamais la bloquer.
- WebSocket : le navigateur s'abonne aux tronçons visibles et reçoit l'état initial puis des différences (positions, chantiers, événements). Messages de contrôle en JSON (types générés par `ts-rs`) ; tronçons de carte en binaire par HTTP (`/api/map/chunk`), qui pourront être signalés « modifiés » par le WebSocket quand la carte changera (routes, déboisement).
- REST : fiche d'un villageois, d'un village, historique des prix, chronique, arbre généalogique.
- Sauvegarde : état complet toutes les 5 minutes (`postcard` + `zstd`, écriture atomique, 48 copies tournantes, une par jour gardée 30 jours). Numéro de version dans chaque sauvegarde pour pouvoir migrer.
- Historique dans SQLite (`rusqlite`) : événements, naissances et morts, séries de stats (population, prix, stocks). C'est ce qui permet de raconter le passé sans garder tout en mémoire.
- Administration : pause, vitesse, nouvelle graine, sauvegarde/restauration.
- Pouvoirs du joueur : passent par une file de commandes appliquées au début d'un tick et enregistrées avec leur tick, ce qui garde le monde rejouable (voir « Mode dieu »).
- Machine : tour Ryzen 5 2600, 16 Go de RAM, Linux.
- Déploiement Docker :
  - `Dockerfile` multi-étapes : étape Node (build du client avec Vite), étape `rust:slim` (compilation avec cache des dépendances via `cargo-chef`), image finale `debian:bookworm-slim` qui ne contient que le binaire et le client compilé (~190 Mo).
  - `docker-compose.yml` : service `hameaux`, `restart: unless-stopped`, dossier `./data` à côté du projet (`~/apps/hameaux/data`) monté sur `/data` (sauvegardes et SQLite ; visible directement sur la tour, jamais touché par le déploiement, le monde survit aux mises à jour de l'image), conteneur lancé avec l'utilisateur de la tour pour que les fichiers lui appartiennent, port publié sur le réseau local / le VPN et jamais redirigé sur la box, `stop_grace_period: 30s`.
  - Arrêt propre : le serveur intercepte `SIGTERM`, termine le tick en cours, écrit une sauvegarde, puis s'arrête. Au démarrage, il recharge la dernière sauvegarde valide.
  - Mise à jour : `deploy.sh` copie les sources dans `~/apps/hameaux` sur la tour (rsync par SSH) puis lance `docker compose up -d --build`. L'ancien conteneur sauvegarde en s'arrêtant, et le numéro de version de la sauvegarde permet de migrer l'ancien monde.
  - Réglages par variables d'environnement (`HAMEAUX_SPEED`, `HAMEAUX_SEED`, `HAMEAUX_DATA_DIR`, `HAMEAUX_BIND`) dans un fichier `.env`.
  - Santé : point `/health` utilisé par le `healthcheck` Docker ; journaux lisibles via `docker compose logs`.
- Accès : réseau local et VPN seulement (Tailscale ou WireGuard), rien d'exposé sur Internet. Le serveur écoute sur l'adresse du réseau local / du VPN ; pas besoin de Caddy ni de mot de passe au départ.

**Client web (`web/`)** — décidé : TypeScript + PixiJS (WebGL) pour la carte, interface légère (Preact ou équivalent), types générés depuis Rust (`ts-rs`) pour rester synchronisé. Construit avec Vite, servi par le serveur Rust lui-même.

## Le monde

- Génération : relief, humidité et température par bruit (`noise`), rivières par écoulement, lacs, côtes, biomes (plaine, forêt, collines, montagne, marais).
- Ressources placées : bonnes terres, forêts, carrières, argile, gisements de fer, poissons, gibier, baies.
- Villages fondés près de l'eau et des bonnes terres, à distance les uns des autres, chacun avec un nom, une couleur et quelques traits de départ (village de pêcheurs, de mineurs…).
- Temps (décidé, réglable) : 1 jour de jeu = 24 min réelles, 4 saisons de 7 jours, donc 1 an ≈ 11 h réelles et une vie (~70 ans) ≈ 1 mois réel. On voit des générations passer en quelques mois.
- Météo simulée par région (fronts qui traversent la carte), éventuellement calée sur la vraie météo d'une ville.

## Économie

**Biens**
- Bruts : céréales, légumes, fruits, viande, poisson, lait, laine, bois, pierre, argile, minerai de fer, plantes.
- Transformés : farine, pain, planches, briques, fer, outils, tissu, vêtements, poteries, bière, meubles.
- Chaque bien a une durée de vie (le pain rassit, les outils s'usent), ce qui entretient la demande.

**Production** : bâtiments-ateliers (moulin, boulangerie, scierie, four à briques, forge, tisserand, brasserie) avec recettes, temps de travail, outils nécessaires et compétence de l'artisan.

**Propriété** : chaque foyer a son stock et son argent ; le village a un grenier commun alimenté par une taxe. Les maisons et ateliers appartiennent à quelqu'un et se transmettent en héritage.

**Marché** : un marché par village. Chaque jour, les foyers publient ce qu'ils veulent vendre et acheter ; le prix de chaque bien monte quand la demande dépasse l'offre et baisse sinon. Monnaie : le « sou ». Le troc reste possible entre proches.

**Choix du métier** : revenu espéré au prix du marché + compétence + tradition familiale + goûts personnels. Si le pain est cher, des gens deviennent boulangers.

**Commerce entre villages** : métier de marchand. Il compare les prix connus (avec du retard : il ne sait que ce qu'on lui a raconté), charge une charrette, voyage, vend. Les routes commerciales se tracent et s'améliorent avec le passage. Risques en route (météo, brigands plus tard).

## Cerveaux des villageois

**Besoins** (descendent avec le temps) : faim, sommeil, chaleur, sécurité, compagnie, loisir, reconnaissance, sens (foi, appartenance).

**Personnalité** : 5 traits (ouverture, rigueur, sociabilité, gentillesse, nervosité) + valeurs (ambition, générosité, piété, goût du risque, attachement au village). Tirés à la naissance, en partie hérités des parents.

**Compétences** : une par métier, qui progresse en pratiquant et en apprenant d'un maître.

**Humeur et émotions** : joie, colère, peur, tristesse, avec une durée. Elles modifient les décisions (un villageois en colère accepte une dispute plus facilement).

**Mémoire** : souvenirs datés avec une charge émotionnelle qui s'efface (« Paul m'a aidé quand ma maison a brûlé », « Jeanne m'a volé du pain »). Les souvenirs forts restent toute la vie.

**Relations** (par paire) : affection, confiance, respect, attirance, rivalité. Elles évoluent selon les échanges et les souvenirs. **Réputation** : ce que le village pense de quelqu'un, propagé par les commérages (avec déformation possible).

**Décision**, en deux étages :
1. *Intentions* (IA à utilité) : à chaque décision, le villageois note les choses qu'il pourrait faire (manger, travailler, aller au marché, rendre visite à un ami, courtiser, se reposer, aller à la fête…) selon ses besoins, sa personnalité, l'heure et ses relations, et choisit la meilleure, avec un peu de hasard.
2. *Plans* (HTN) : l'intention se découpe en actions (« faire du pain » = aller au moulin → acheter de la farine → aller au four → cuire → vendre). Un plan est abandonné si le monde change (plus de farine, incendie, orage).

**Objectifs de vie** : se marier, avoir des enfants, devenir maître artisan, construire une plus grande maison, devenir chef, partir fonder un village. Ils orientent les intentions sur des années.

## Interactions

- Conversations avec un sujet : nouvelles, commérages, prix, séduction, dispute, conseil. Elles changent les relations et font circuler l'information.
- Entraide (chantier collectif, incendie, prêt de nourriture), cadeaux, dettes.
- Couples, mariages (y compris entre villages, avec déménagement), séparations, veuvage, remariage.
- Conflits : disputes, rancunes, vols, bagarres ; justice par le chef ou un conseil (amende, bannissement).
- Vie collective : chef élu ou désigné par prestige, conseil, taxes, grands travaux (pont, mur, moulin, temple), fêtes, foires entre villages.
- Démographie : maladies et épidémies (qui voyagent avec les marchands), migrations vers les villages prospères, fondation de nouveaux villages quand un village est trop peuplé.
- Savoirs : une technique (moulin à eau, assolement, four à briques) apparaît quelque part et se répand par l'apprentissage et le commerce.
- Plus tard : rivalités entre villages, alliances, conflits armés.

## Mode dieu

Le site ne sert pas qu'à regarder : on peut agir beaucoup sur le monde. Chaque pouvoir est une commande datée (rejouable, visible dans la chronique) et coûte de la « foi » qui se recharge avec le temps, pour éviter de tout écraser.

- **Nature** : pluie, sécheresse, orage, hiver rude, récolte miraculeuse, crue, incendie de forêt, faire pousser une forêt, révéler un gisement, tremblement de terre.
- **Bénédictions et fléaux** : guérir une épidémie ou en déclencher une, invasion de sauterelles, abondance de gibier ou de poisson.
- **Villageois** : soufflet d'inspiration (une idée, un objectif de vie, une technique), rêve ou présage (pousse vers une décision : se marier, partir, construire un temple), guérir, bénir (chance, compétence), maudire, faire se rencontrer deux personnes.
- **Société** : apparaître en signe dans le ciel (les croyants réagissent), envoyer un étranger (marchand, prophète, réfugiés), provoquer une fête, nommer ou renommer un village.
- **Création** : poser une ressource, tracer une route, fonder un village avec quelques familles.
- **Réaction des villageois** : les miracles marquent les souvenirs et nourrissent une religion propre au monde (culte, temple, prêtres, interprétation des présages). Un village frappé souvent peut te craindre ou t'en vouloir ; un village béni peut te vénérer.

Le moteur expose une liste de commandes (`GodCommand`) ; `hameaux-cli` peut les rejouer pour tester l'effet de chaque pouvoir sur l'équilibre.

## Phases

Chaque phase se termine par quelque chose de visible dans le navigateur et par des tests sans écran (`hameaux-cli`, comme `VillageBalanceTest`).

0. ✅ **Socle** (20/11/2026) : workspace Cargo (`hameaux-core`, `hameaux-protocol`, `hameaux-server`, `hameaux-cli`), calendrier (1 tick = 10 s de jeu), sauvegardes versionnées avec rotation, boucle de simulation sur son fil, API (`/health`, `/ws`, pause/vitesse/sauvegarde), page web PixiJS (ciel jour/nuit et saisons), `Dockerfile` + compose + `deploy.sh`, CI GitHub Actions. Reste à faire : premier déploiement sur la tour.
1. ✅ **Monde** (20/11/2026) :
   - `hameaux-worldgen` : relief (bruit fractal + dôme continental + chaînes de montagnes, répartition par rang pour maîtriser la part de mer), mer reliée au bord, écoulement par remplissage de priorité (rivières de 3 tailles, lacs dans les cuvettes profondes), humidité et température, 13 milieux, fertilité, 6 ressources (pierre, fer, argile, poisson, gibier, baies), 12 villages placés selon la fertilité et l'eau douce, avec noms (« -sur-Mer », « -les-Bois »…) et couleurs. 1024×1024 en ~0,15 s ; déterministe (testé).
   - Sauvegarde v2 (~1 Mo) ; une sauvegarde v1 de la phase 0 est reprise au même tick avec une carte générée depuis sa graine.
   - API : `GET /api/map` (infos, libellés, villages) et `GET /api/map/chunk/{cx}/{cy}` (6 couches binaires, gzip ~9 ko par tronçon).
   - Client : vue d'ensemble 1 px/case remplie au fil du chargement, tronçons détaillés à partir de 3 px/case (grain, arbres, roseaux, rivières, ressources), glisser/molette/pincement/clavier, repères et liste des villages (vol vers le village), infobulle de case, voile de nuit.
   - `hameaux-cli map --seed N --out carte.png` : aperçu et statistiques d'un monde.
2. ✅ **Des villages vivants** (20/11/2026) :
   - Échelle : 1 case ≈ 25 m, 1 tick = 10 s de jeu, 1 an = 28 jours de jeu (≈ 11 h réelles), une vie ≈ 62–88 ans.
   - Fondation de chaque village : grenier, puits, 4–5 familles, un chantier, des champs sur les terres les plus fertiles ; chemins tracés jusqu'au grenier (avec ponts), forêt défrichée sous les constructions.
   - Journée : lever 6 h, travail 7–18 h (repos le 7ᵉ jour), soirée près de chez soi ou au puits, nuit à la maison.
   - Métiers : fermier (labour et semis au printemps, moisson en été/automne, rien l'hiver), bûcheron (déboise peu à peu), tailleur de pierre (si carrière à portée), bâtisseur (porte bois et pierre au chantier). Repli sur le bois quand il n'y a rien à faire.
   - Familles : couples, naissances (~1 tous les 5 ans par couple fertile), enfants qui jouent ou suivent un parent, métier à 16 ans avec apprentissage auprès d'un maître, départ de chez les parents vers 19 ans (chantier réservé ou maison libre reprise), décès entre 62 et 88 ans, nouveaux venus quand le grenier déborde, départs en cas de disette, pionniers dans un village vide. Amitiés par proximité, discussions au puits.
   - Maisons vides : abandonnées, puis ruines, puis la nature reprend la parcelle.
   - Chronique (2 000 derniers événements), recherche de chemin A* (ponts, gués, coûts par milieu).
   - Sauvegarde v3 ; une sauvegarde v2 (phase 1) est reprise avec la même carte et des villages fondés au même tick.
   - API : `ClientMsg::View` (zone regardée) → `Scene` (habitants, maisons, champs, greniers visibles, 4 fois par seconde) ; `Villages` (population) ; `GET /api/person/{id}`, `/api/village/{id}`, `/api/events`, `/api/map/changes`.
   - Client : maisons (chantier → toit, abandon, ruine), champs (friche → épis dorés), grenier et puits, habitants interpolés avec ce qu'ils portent ; clic → fiche (famille, amis, activité, histoire) avec « Suivre » ; fiche de village ; chronique ; tronçons de carte rechargés quand une route ou un défrichement les modifie.
   - Équilibre (`hameaux-cli run --days 336`) : ~5 % de croissance par an, villages de 25 à 80 habitants après 12 ans, greniers bien remplis (pas encore de disette : l'économie de la phase 3 doit créer de la tension).
3. ✅ **Économie de base** (20/11/2026) :
   - 9 biens : grain, farine, pain, poisson, viande, bois, pierre, minerai, outils. Rations : pain et viande 1, farine 0,9, grain et poisson 0,8 ; goût (on paie le pain plus cher que le grain) ; la nourriture se gâte (pain 3 %/jour, viande et poisson 4 %, grain 0,5 %).
   - Foyers (un par maison) : réserves et argent (« sous »). Les récoltes et ce qu'on rapporte vont au foyer, 10 % du grain au grenier commun.
   - Nouveaux métiers : meunier (moulin), boulanger (four, consomme du bois), forgeron (forge : minerai + bois → outils), pêcheur, chasseur, mineur. Ateliers construits quand le village grandit (moulin à 12 habitants, four à 16, forge à 20 si du minerai est à portée).
   - Outils : rendement ×1,4, s'usent en ~40 séances.
   - Marché quotidien (11 h) : chaque foyer garde une semaine de vivres, son bois de chauffe (plus l'hiver) et les matières de son métier, vend le reste et achète ce qui manque (la nourriture la moins chère pour son goût). Les prix bougent avec l'offre et la demande (bornés entre 0,3 et 5 fois le prix de départ). Taxe de 10 % pour la caisse du village.
   - Caisse du village : salaire des bâtisseurs, achat du bois et de la pierre des chantiers, achat de vivres pour le grenier commun (qui nourrit les foyers qui n'ont plus rien), avances aux artisans à court d'argent (remboursées sur leurs ventes).
   - Métiers choisis selon ce qu'ils rapportent aux prix du marché (et ce qu'ils ont rapporté), avec un bonus pour le métier des parents ; un adulte peut changer de métier au plus une fois par an. Chômage : quand son métier ne rapporte presque rien et que le foyer a des réserves, on se repose.
   - Une récolte par an (semis au printemps) ; ce qui n'est pas moissonné pourrit à l'hiver ; pas de nouveaux champs quand le grain ne se vend plus.
   - Disette par foyer : le grenier commun aide, puis la famille part au bout de 4 jours de faim.
   - Sauvegarde v4 ; une sauvegarde v3 (phase 2) est reprise avec un foyer par maison (30 sous et sa part du grenier).
   - Site : ateliers dessinés (moulin à ailes, four qui fume, forge), ce que porte chaque habitant ; fiche de village avec caisse, ateliers et marché (prix, tendance sur 7 jours, petite courbe, quantités vendues) ; fiche d'habitant avec son foyer (argent, jours de vivres, réserves).
   - Équilibre (`hameaux-cli run --days 224`) : croissance ~11 %/an, pas de famine sur 8 ans. Limites connues : la nourriture reste abondante (prix au plancher), les outils manquent dans les villages sans forge, l'argent se concentre chez ceux dont les produits se vendent. Le commerce entre villages (phase 5) et la terre qui devient rare doivent créer plus de tension.
4. **Cerveaux v2** : besoins, personnalité, humeur, mémoire, relations, utilité + HTN, objectifs de vie.
5. **Entre villages** : routes, marchands, commerce, mariages et migrations, fondation de villages.
6. **Société** : chef et conseil, taxes, justice, fêtes et foires, maladies, diffusion des savoirs.
7. **Site complet** : fiches détaillées (besoins, traits, souvenirs, relations), arbres généalogiques, tableau de bord par village (population, stocks, courbes de prix), carte du commerce, chronique, page « depuis ta dernière visite ».
8. **Mode dieu** : file de commandes, jauge de foi, pouvoirs sur la nature puis sur les villageois, religion qui naît des miracles. (Les premiers pouvoirs simples — pluie, sécheresse, fête — peuvent arriver dès la phase 3 pour tester l'équilibre.)
9. **Option** : récits rédigés par un LLM à partir de la chronique (biographies, gazette hebdomadaire du monde).
10. **Plus tard** : lien avec le launcher TV (afficher un village du serveur à la place du village local), via l'API REST/WebSocket existante.

Ordre conseillé : 0 → 1 → 2 rapidement pour avoir un monde qui tourne sur le serveur, puis enrichir. L'équilibrage (`hameaux-cli --seed 42 --years 50`, rapport de population, prix et famines) se fait à chaque phase, pas à la fin.

## Décisions (20/11/2026)

- Client web : TypeScript + PixiJS.
- Accès : réseau local / VPN uniquement.
- Temps : 1 jour de jeu = 24 min réelles.
- Interaction : mode dieu riche (beaucoup d'actions).
- Serveur : tour Ryzen 5 2600, 16 Go, GTX 1060 (inutilisée par la simulation).
- Lien TV : plus tard.
- Système de la tour : Linux, déploiement en conteneur Docker (docker compose).
