//! Vues du monde envoyées au navigateur : scène visible, fiches, chronique.

use hameaux_core::{
    Clock, World,
    economy::{Good, Workshop},
    events::Event,
    mind::{NEED_LABELS, feminine},
    people::Carry,
    people::{Person, Status},
    settlement::{FieldState, HouseState},
    tuning::{FIELD_SIZE, YEAR},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ------------------------------------------------------------------ scène

/// Un habitant visible. `act` et `carry` : voir `Act` et `Carry` côté Rust (index).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PersonDot {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub female: bool,
    /// 0 enfant, 1 adulte, 2 aîné.
    pub stage: u8,
    /// Index de `Act`.
    pub act: u8,
    /// Ce qu'il porte : 0 rien, 1 + index du bien (`Good`), 254 une charrette, 255 son baluchon.
    pub carry: u8,
    /// Vêtement, peau, cheveux.
    pub looks: [u8; 3],
    /// Malade.
    pub sick: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HouseView {
    pub id: u32,
    pub village: u32,
    pub x: u32,
    pub y: u32,
    /// 0 chantier, 1 habitée, 2 abandonnée, 3 ruine, 4 envahie par la végétation.
    pub state: u8,
    /// Avancement du chantier, 0–1.
    pub progress: f32,
    pub roof: u8,
    pub family: String,
    /// 0 maison, 1 moulin, 2 four à pain, 3 forge.
    pub kind: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FieldView {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    pub size: u32,
    /// 0 friche, 1 labouré, 2 semé, 3 mûr.
    pub state: u8,
    pub growth: f32,
    pub work: f32,
}

/// Grenier et puits d'un village.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CivicView {
    pub village: u32,
    pub x: u32,
    pub y: u32,
    pub well_x: u32,
    pub well_y: u32,
    pub color: u32,
    /// Fête des moissons (ou premier jour de foire) aujourd'hui.
    pub feast: bool,
    /// Foire ouverte.
    pub fair: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Scene {
    #[ts(type = "number")]
    pub tick: u64,
    pub people: Vec<PersonDot>,
    pub houses: Vec<HouseView>,
    pub fields: Vec<FieldView>,
    pub civic: Vec<CivicView>,
}

impl Scene {
    /// Ce qui se trouve dans le rectangle de cases `[x0, x1] × [y0, y1]` (avec une marge).
    pub fn of(world: &World, x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let m = 4.0;
        let inside = |x: f32, y: f32| {
            x >= x0 as f32 - m && y >= y0 as f32 - m && x <= x1 as f32 + m && y <= y1 as f32 + m
        };
        let tick = world.tick;
        let mut scene = Scene {
            tick,
            ..Default::default()
        };
        for v in &world.villages {
            // Un village ne s'étend pas à plus d'une soixantaine de cases de son grenier.
            if !inside(v.x as f32, v.y as f32) && !near(v.x, v.y, x0, y0, x1, y1, 60) {
                continue;
            }
            scene.civic.push(CivicView {
                village: v.id,
                x: v.x,
                y: v.y,
                well_x: v.well.0,
                well_y: v.well.1,
                color: v.color,
                feast: world.festive(v.id),
                fair: world.fair_open(v.id),
            });
            for h in v.houses.iter().filter(|h| inside(h.x as f32, h.y as f32)) {
                scene.houses.push(HouseView {
                    id: h.id,
                    village: v.id,
                    x: h.x,
                    y: h.y,
                    state: match h.state {
                        HouseState::Building => 0,
                        HouseState::Inhabited => 1,
                        HouseState::Abandoned => 2,
                        HouseState::Ruin => 3,
                        HouseState::Overgrown => 4,
                    },
                    progress: h.progress(),
                    roof: h.roof,
                    family: h.family.clone(),
                    kind: match world.economy.workshops.get(&h.id) {
                        None => 0,
                        Some(Workshop::Mill) => 1,
                        Some(Workshop::Bakery) => 2,
                        Some(Workshop::Smithy) => 3,
                    },
                });
            }
            for f in v.fields.iter().filter(|f| inside(f.x as f32, f.y as f32)) {
                scene.fields.push(FieldView {
                    id: f.id,
                    x: f.x,
                    y: f.y,
                    size: FIELD_SIZE,
                    state: match f.state {
                        FieldState::Fallow => 0,
                        FieldState::Ploughed => 1,
                        FieldState::Sown => 2,
                        FieldState::Ripe => 3,
                    },
                    growth: f.growth,
                    work: f.work,
                });
            }
        }
        for p in world.alive().filter(|p| !p.brain.hidden && inside(p.x, p.y)) {
            scene.people.push(PersonDot {
                id: p.id,
                x: p.x,
                y: p.y,
                female: p.female,
                stage: if p.child(tick) {
                    0
                } else if p.elder(tick) {
                    2
                } else {
                    1
                },
                act: p.brain.act as u8,
                carry: match p.brain.carry {
                    None => 0,
                    Some(Carry::Good(g)) => g as u8 + 1,
                    Some(Carry::Bundle) => 255,
                    Some(Carry::Cart) => 254,
                },
                looks: p.looks,
                sick: world.society.sick.contains_key(&p.id),
            });
        }
        scene
    }
}

fn near(x: u32, y: u32, x0: i32, y0: i32, x1: i32, y1: i32, margin: i32) -> bool {
    let (x, y) = (x as i32, y as i32);
    x >= x0 - margin && y >= y0 - margin && x <= x1 + margin && y <= y1 + margin
}

// ------------------------------------------------------------------ fiches

/// Lien vers un habitant (nom cliquable dans une fiche).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PersonRef {
    pub id: u32,
    pub name: String,
    pub alive: bool,
    /// Lien de parenté ou rôle (« mère », « ami »…), si utile.
    pub note: String,
}

fn person_ref(world: &World, id: u32, note: impl Into<String>) -> PersonRef {
    let p = &world.people[id as usize];
    PersonRef {
        id,
        name: p.name(),
        alive: p.alive(),
        note: note.into(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PersonSheet {
    pub id: u32,
    pub name: String,
    pub female: bool,
    pub age: u32,
    /// « vivante », « décédé à 74 ans »…
    pub status: String,
    pub alive: bool,
    pub village: u32,
    pub village_name: String,
    /// « fermière », « enfant », « retraité »…
    pub role: String,
    /// Ce qu'il fait maintenant (« coupe du bois », « discute avec Lucie »…).
    pub doing: String,
    pub home: Option<String>,
    pub born: String,
    pub x: f32,
    pub y: f32,
    pub hidden: bool,
    /// Famille proche : conjoint, parents, enfants, frères et sœurs.
    pub family: Vec<PersonRef>,
    /// Relations les plus fortes (amis, amours, rivaux ; le lien dans `note`).
    pub relations: Vec<PersonRef>,
    /// Traits de caractère les plus marqués (« curieuse », « bourru »…).
    pub traits: Vec<String>,
    /// Humeur (« joyeux », « triste », « fâchée »…).
    pub mood: String,
    /// Humeur de -1 à 1 (pour la couleur).
    pub mood_value: f32,
    /// Besoins, de 0 (manque) à 1 (comblé).
    pub needs: Vec<NeedView>,
    pub goals: Vec<GoalView>,
    pub skill: Option<SkillView>,
    /// Opinion moyenne du village (-100 à 100) et son libellé.
    pub reputation: Option<f32>,
    pub reputation_label: String,
    /// Souvenirs, du plus marquant au plus faible.
    pub memories: Vec<MemoryView>,
    pub master: Option<PersonRef>,
    /// Réserves et argent de son foyer.
    pub household: Option<HouseholdView>,
    /// « cheffe de Clairval », « membre du conseil de Belair ».
    pub office: Option<String>,
    /// « malade (fièvre), encore 2 jours ».
    pub health: Option<String>,
    /// Condamnations par le chef.
    pub convictions: u32,
    pub events: Vec<EventView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NeedView {
    pub label: String,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GoalView {
    pub label: String,
    pub reached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkillView {
    /// Métier (« boulangère »).
    pub job: String,
    pub value: f32,
    /// « débutant », « compagnon », « maître »…
    pub rank: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryView {
    pub text: String,
    pub date: String,
    /// Charge émotionnelle (-1 à 1).
    pub weight: f32,
    /// La personne concernée, s'il y en a une.
    pub who: Option<u32>,
}

/// Une quantité d'un bien.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GoodAmount {
    pub good: u8,
    pub label: String,
    pub amount: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HouseholdView {
    pub coins: f32,
    /// Jours de nourriture d'avance.
    pub food_days: f32,
    pub goods: Vec<GoodAmount>,
    pub hungry: bool,
}

fn goods_list(inv: &hameaux_core::economy::Inv) -> Vec<GoodAmount> {
    Good::ALL
        .iter()
        .filter(|&&g| inv[g as usize] >= 0.05)
        .map(|&g| GoodAmount {
            good: g as u8,
            label: g.label().into(),
            amount: inv[g as usize],
        })
        .collect()
}

impl PersonSheet {
    pub fn of(world: &World, id: u32) -> Option<Self> {
        let p = world.people.get(id as usize)?;
        let tick = world.tick;
        let age_at = |t: u64| ((t as i64 - p.born) as f32 / YEAR as f32).max(0.0) as u32;
        let (status, age) = match p.status {
            Status::Alive => (format!("vivant{}", p.e()), age_at(tick)),
            Status::Dead { tick: t } => (format!("décédé{} à {} ans", p.e(), age_at(t)), age_at(t)),
            Status::Left { tick: t } => (
                format!("parti{} du village à {} ans", p.e(), age_at(t)),
                age_at(t),
            ),
        };
        let village = world.village_index(p.village).map(|vi| &world.villages[vi]);
        let role = if p.child(tick) {
            if p.female {
                "enfant (fille)".into()
            } else {
                "enfant (garçon)".into()
            }
        } else {
            let job = p.job.map(|j| j.label(p.female)).unwrap_or("sans métier");
            if p.elder(tick) {
                format!("{job} à la retraite")
            } else if tick < p.apprentice_until {
                format!("apprenti{} {job}", if p.female { "e" } else { "" })
            } else {
                job.to_string()
            }
        };
        let mut doing = if p.alive() {
            p.brain.act.label().to_string()
        } else {
            String::new()
        };
        // Ce qu'il a en tête, s'il est en route pour voir quelqu'un.
        if let Some(q) = p.brain.intent.target()
            && p.alive()
            && matches!(p.brain.act, hameaux_core::people::Act::Walking)
            && let Some(q) = world.people.get(q as usize)
        {
            doing = format!("{} {}", p.brain.intent.label(), q.first);
        }
        if let Some(with) = p.brain.with {
            if matches!(
                p.brain.act,
                hameaux_core::people::Act::Chatting
                    | hameaux_core::people::Act::Learning
                    | hameaux_core::people::Act::Playing
            ) && let Some(q) = world.people.get(with as usize)
            {
                doing += &format!(" avec {}", q.first);
            }
            if p.brain.act == hameaux_core::people::Act::Building
                && let Some(h) = village.and_then(|v| v.house(with))
                && !h.family.is_empty()
            {
                doing = format!("construit la maison des {}", h.family);
            }
        }
        // Marchand : où il en est de son voyage.
        if let Some(t) = world.trade.traders.get(&id).filter(|_| p.alive()) {
            use hameaux_core::trade::Leg;
            let place = world.village_name(t.dest);
            let cargo: Vec<String> = Good::ALL
                .iter()
                .filter(|&&g| t.cargo[g as usize] >= 1.0)
                .map(|&g| format!("{:.0} {}", t.cargo[g as usize], g.label()))
                .collect();
            let with = if cargo.is_empty() {
                String::new()
            } else {
                format!(" avec {}", cargo.join(", "))
            };
            match t.leg {
                Leg::Idle => {}
                Leg::Buying => {
                    doing = format!("attend le marché pour charger sa charrette (part pour {place})")
                }
                Leg::Outbound => doing = format!("en route vers {place}{with}"),
                Leg::Selling if p.brain.hidden => doing = format!("dort à l'auberge de {place}"),
                Leg::Selling => doing = format!("attend le marché de {place}{with}"),
                Leg::Returning => doing = format!("rentre de {place}{with}"),
            }
        }
        let sickness = world.sick(id).filter(|_| p.alive());
        if let Some(s) = sickness
            && !p.brain.hidden
            && !world.away(id as usize)
        {
            doing = format!("est malade, alité{} ({})", p.e(), s.disease.label());
        }
        if let Some(c) = p.brain.carry
            && p.alive()
            && !p.brain.hidden
            && c != Carry::Cart
        {
            doing += &format!(" (porte {})", c.label());
        }
        let home = p
            .home
            .and_then(|h| village?.house(h))
            .map(|h| format!("maison des {}", h.family));

        let mut family = Vec::new();
        if let Some(q) = p.partner {
            let partner = &world.people[q as usize];
            let word = if partner.female { "épouse" } else { "époux" };
            let note = if partner.alive() {
                word.to_string()
            } else {
                format!("{word} (décédé{})", partner.e())
            };
            family.push(person_ref(world, q, note));
        }
        for &q in &p.parents {
            family.push(person_ref(
                world,
                q,
                if world.people[q as usize].female {
                    "mère"
                } else {
                    "père"
                },
            ));
        }
        let mut siblings: Vec<u32> = p
            .parents
            .iter()
            .flat_map(|&q| world.people[q as usize].children.iter().copied())
            .filter(|&c| c != id)
            .collect();
        siblings.sort();
        siblings.dedup();
        for q in siblings {
            family.push(person_ref(
                world,
                q,
                if world.people[q as usize].female {
                    "sœur"
                } else {
                    "frère"
                },
            ));
        }
        for &q in &p.children {
            family.push(person_ref(
                world,
                q,
                if world.people[q as usize].female {
                    "fille"
                } else {
                    "fils"
                },
            ));
        }

        // Les liens les plus forts, hors famille proche (déjà listée) : amitiés, amours, rivalités.
        let kin: Vec<u32> = family.iter().map(|r| r.id).collect();
        let mut rel: Vec<(u32, f32)> = p
            .mind
            .relations
            .iter()
            .filter(|&(k, _)| !kin.contains(k) && world.people[*k as usize].alive())
            .map(|(&k, r)| (k, r.affection.max(r.rivalry * 1.5).max(r.attraction * 1.2)))
            .filter(|&(_, s)| s >= 8.0)
            .collect();
        rel.sort_by(|a, b| b.1.total_cmp(&a.1));
        let relations = rel
            .into_iter()
            .take(7)
            .map(|(f, _)| {
                let q = &world.people[f as usize];
                person_ref(world, f, p.mind.rel(f).label(q.female))
            })
            .collect();
        let m = &p.mind;
        let needs = NEED_LABELS
            .iter()
            .zip(m.needs.to_array())
            .map(|(l, v)| NeedView {
                label: (*l).into(),
                value: v,
            })
            .collect();
        let goals = m
            .goals
            .iter()
            .map(|g| GoalView {
                label: g.goal.label().into(),
                reached: g.reached.is_some(),
            })
            .collect();
        let skill = p.job.filter(|_| !p.child(tick)).map(|j| {
            let v = m.skill(j);
            let rank = hameaux_core::sim::LEVEL_LABELS[hameaux_core::sim::skill_level(v)];
            SkillView {
                job: j.label(p.female).into(),
                value: v,
                rank: feminine(rank, p.female),
            }
        });
        let reputation = world.reputation(id);
        let e = p.e();
        let reputation_label = match reputation {
            None => "inconnu".to_string() + e,
            Some(r) if r >= 30.0 => format!("très estimé{e}"),
            Some(r) if r >= 12.0 => format!("apprécié{e}"),
            Some(r) if r >= 0.0 => format!("peu connu{e}"),
            Some(_) => format!("mal vu{e}"),
        };
        let mut memories: Vec<&hameaux_core::mind::Memory> = m.memories.iter().collect();
        memories.sort_by(|a, b| b.weight.abs().total_cmp(&a.weight.abs()));
        let memories = memories
            .into_iter()
            .take(8)
            .map(|mem| {
                let who = mem
                    .who
                    .and_then(|w| world.people.get(w as usize))
                    .map(|q| q.first.clone())
                    .unwrap_or_default();
                let c = Clock::at(mem.tick);
                MemoryView {
                    text: mem.text(&who, p.female),
                    date: format!("An {}, {}", c.year, c.season.label().to_lowercase()),
                    weight: mem.weight,
                    who: mem.who,
                }
            })
            .collect();

        let events = world
            .chronicle
            .events
            .iter()
            .rev()
            .filter(|e| e.people.contains(&id))
            .take(12)
            .map(|e| EventView::of(world, e))
            .collect();

        Some(PersonSheet {
            id,
            name: p.name(),
            female: p.female,
            age,
            status,
            alive: p.alive(),
            village: p.village,
            village_name: village.map(|v| v.name.clone()).unwrap_or_default(),
            role,
            doing,
            home,
            born: born_label(p),
            x: p.x,
            y: p.y,
            hidden: p.brain.hidden,
            family,
            relations,
            traits: m.traits.describe(p.female),
            mood: feminine(m.mood_label(), p.female),
            mood_value: m.mood.valence() + 0.5 * (m.needs.wellbeing() - 0.7),
            needs,
            goals,
            skill,
            reputation,
            reputation_label,
            memories,
            master: p
                .master
                .filter(|_| tick < p.apprentice_until)
                .map(|m| person_ref(world, m, "maître")),
            household: p.home.and_then(|h| world.economy.households.get(&h)).map(|h| {
                let members = village
                    .and_then(|v| v.house(p.home?))
                    .map_or(1, |house| house.residents.len().max(1));
                HouseholdView {
                    coins: h.coins,
                    food_days: h.rations() / members as f32,
                    goods: goods_list(&h.inv),
                    hungry: h.hungry_days > 0,
                }
            }),
            office: world
                .society
                .office(p.village, id)
                .filter(|_| p.alive())
                .map(|o| {
                    let name = village.map(|v| v.name.as_str()).unwrap_or_default();
                    match (o, p.female) {
                        ("chef", true) => format!("cheffe {}", hameaux_core::sim::de(name)),
                        ("chef", false) => format!("chef {}", hameaux_core::sim::de(name)),
                        _ => format!("membre du conseil {}", hameaux_core::sim::de(name)),
                    }
                }),
            health: sickness.map(|s| {
                let left = match s.days_left {
                    1 => "encore un jour".to_string(),
                    d => format!("encore {d} jours"),
                };
                format!("malade : {} ({left})", s.disease.label())
            }),
            convictions: world.society.convictions.get(&id).copied().unwrap_or(0),
            events,
        })
    }
}

fn born_label(p: &Person) -> String {
    if p.born < 0 {
        // Né avant la fondation du monde : on donne l'année approximative.
        let years = (-p.born) as f32 / YEAR as f32;
        format!(
            "né{} environ {} ans avant la fondation",
            p.e(),
            years.floor() as u32
        )
    } else {
        let c = Clock::at(p.born as u64);
        format!(
            "né{} en l'an {}, {} jour {}",
            p.e(),
            c.year,
            c.season.label().to_lowercase(),
            c.day
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VillageSheet {
    pub id: u32,
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub color: u32,
    pub population: u32,
    pub children: u32,
    pub elders: u32,
    pub houses: u32,
    pub sites: u32,
    pub ruins: u32,
    pub fields: u32,
    pub fields_ripe: u32,
    pub food: f32,
    pub wood: u32,
    pub stone: u32,
    /// Nombre de jours de nourriture en réserve.
    pub food_days: u32,
    pub hungry: bool,
    pub has_stone: bool,
    pub founded: String,
    /// Habitants par métier : (libellé, nombre).
    pub jobs: Vec<(String, u32)>,
    /// Caisse du village (taxes), en sous.
    pub treasury: f32,
    /// Ateliers construits.
    pub workshops: Vec<String>,
    pub market: Vec<PriceRow>,
    /// Liens avec les autres villages (routes, nouvelles), du plus fréquenté au moins.
    pub links: Vec<LinkView>,
    /// Marchands du village.
    pub merchants: Vec<PersonRef>,
    /// Village d'où sont venus les fondateurs (s'il a été fondé en cours de partie).
    pub founded_from: Option<VillageRef>,
    pub chief: Option<PersonRef>,
    pub council: Vec<PersonRef>,
    /// Taxe sur les ventes (0–1).
    pub tax: f32,
    /// « prochaine élection dans 23 jours ».
    pub next_election: String,
    /// Techniques connues, avec leur origine.
    pub techs: Vec<TechView>,
    /// Malades en ce moment.
    pub sick: u32,
    /// « épidémie de fièvre (2 morts) ».
    pub epidemic: Option<String>,
    /// Fêtes et foires (en cours, annoncées, passées).
    pub festivities: Vec<String>,
    pub events: Vec<EventView>,
}

/// Une technique connue d'un village.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TechView {
    /// « l'assolement ».
    pub label: String,
    /// « les champs rendent un quart de plus ».
    pub effect: String,
    /// « découvert par Jeanne Martin, an 3 » ou « appris de Belair, an 4 ».
    pub origin: String,
    pub by: Option<u32>,
    pub from: Option<VillageRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VillageRef {
    pub id: u32,
    pub name: String,
}

/// Ce qui relie un village à un autre.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LinkView {
    pub village: VillageRef,
    /// « route », « chemin » (trajet connu mais pas encore tracé), ou vide.
    pub way: String,
    pub trips: u32,
    /// Âge des dernières nouvelles de ses prix (« il y a 3 jours »), vide si on n'en sait rien.
    pub news: String,
    /// Ce qui s'y vend bien plus cher qu'ici, d'après ces nouvelles.
    pub dearer: Vec<String>,
}

/// Un bien sur le marché d'un village.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PriceRow {
    pub good: u8,
    pub label: String,
    pub price: f32,
    /// Prix il y a 7 jours (pour la tendance).
    pub week_ago: f32,
    /// Prix des derniers jours (le plus récent à la fin).
    pub history: Vec<f32>,
    pub volume: f32,
    pub supply: f32,
    pub demand: f32,
}

/// Fêtes et foires d'un village, pour sa fiche.
fn festivities(world: &World, id: u32) -> Vec<String> {
    let Some(p) = world.society.polity(id) else {
        return Vec::new();
    };
    let today = hameaux_core::sim::tick_day(world.tick);
    let mut out = Vec::new();
    if world.festive(id) && p.feast_day.is_some() {
        out.push("fête des moissons aujourd'hui".to_string());
    }
    match p.fair_day {
        Some(d) if today >= d => out.push("la foire est ouverte".to_string()),
        Some(d) if d == today + 1 => out.push("grande foire demain".to_string()),
        Some(d) => out.push(format!("grande foire dans {} jours", d - today)),
        None => {}
    }
    let year = |t: u64| Clock::at(t).year;
    if p.last_feast > 0 && p.feast_day.is_none() {
        out.push(format!("dernière fête des moissons : an {}", year(p.last_feast)));
    }
    if p.last_fair > 0 && p.fair_day.is_none() {
        out.push(format!("dernière foire : an {}", year(p.last_fair)));
    }
    out
}

impl VillageSheet {
    pub fn of(world: &World, id: u32) -> Option<Self> {
        let v = &world.villages[world.village_index(id)?];
        let tick = world.tick;
        let polity = world.society.polity(id);
        let people: Vec<&Person> = world
            .alive()
            .filter(|p| p.village == id && !p.brain.leaving)
            .collect();
        let children = people.iter().filter(|p| p.child(tick)).count() as u32;
        let elders = people.iter().filter(|p| p.elder(tick)).count() as u32;
        let mut jobs: Vec<(String, u32)> = hameaux_core::people::Job::ALL
            .iter()
            .map(|&j| {
                let n = people
                    .iter()
                    .filter(|p| p.job == Some(j) && !p.elder(tick))
                    .count() as u32;
                (j.label(false).to_string(), n)
            })
            .filter(|(_, n)| *n > 0)
            .collect();
        jobs.sort_by_key(|j| std::cmp::Reverse(j.1));
        let need: f32 = people.iter().map(|p| if p.child(tick) { 0.6 } else { 1.0 }).sum();
        let vi = world.village_index(id)?;
        let household_food: f32 = v
            .houses
            .iter()
            .filter_map(|h| world.economy.households.get(&h.id))
            .map(|h| h.rations())
            .sum();
        let market = world.economy.markets.get(vi);
        let market = Good::ALL
            .iter()
            .map(|&g| {
                let gi = g as usize;
                let history: Vec<f32> = market
                    .map(|m| m.history.iter().map(|p| p[gi]).collect())
                    .unwrap_or_default();
                let price = market.map_or(g.base_price(), |m| m.price[gi]);
                let week_ago = history.len().checked_sub(8).map_or(price, |k| history[k]);
                PriceRow {
                    good: g as u8,
                    label: g.label().into(),
                    price,
                    week_ago,
                    history,
                    volume: market.map_or(0.0, |m| m.volume[gi]),
                    supply: market.map_or(0.0, |m| m.supply[gi]),
                    demand: market.map_or(0.0, |m| m.demand[gi]),
                }
            })
            .collect();
        let c = Clock::at(v.founded);
        Some(VillageSheet {
            id,
            name: v.name.clone(),
            x: v.x,
            y: v.y,
            color: v.color,
            population: people.len() as u32,
            children,
            elders,
            houses: v
                .houses
                .iter()
                .filter(|h| h.state == HouseState::Inhabited)
                .count() as u32,
            sites: v
                .houses
                .iter()
                .filter(|h| h.state == HouseState::Building)
                .count() as u32,
            ruins: v
                .houses
                .iter()
                .filter(|h| matches!(h.state, HouseState::Ruin | HouseState::Overgrown))
                .count() as u32,
            fields: v.fields.len() as u32,
            fields_ripe: v.fields.iter().filter(|f| f.state == FieldState::Ripe).count() as u32,
            food: v.stock.food,
            wood: v.stock.wood,
            stone: v.stock.stone,
            food_days: if need > 0.0 {
                ((v.stock.food + household_food) / need) as u32
            } else {
                0
            },
            hungry: v.hungry_days > 0,
            has_stone: v.has_stone,
            founded: format!(
                "fondé en l'an {}, {} jour {}",
                c.year,
                c.season.label().to_lowercase(),
                c.day
            ),
            jobs,
            treasury: world.economy.treasury.get(vi).copied().unwrap_or(0.0),
            workshops: v
                .houses
                .iter()
                .filter(|h| h.state == HouseState::Inhabited)
                .filter_map(|h| world.economy.workshops.get(&h.id).map(|w| w.label().to_string()))
                .collect(),
            market,
            links: links(world, vi),
            merchants: world
                .trade
                .traders
                .keys()
                .filter(|&&p| world.people[p as usize].village == id && world.people[p as usize].alive())
                .map(|&p| {
                    let t = &world.trade.traders[&p];
                    person_ref(
                        world,
                        p,
                        format!("{} voyages, {:.0} sous gagnés", t.trips, t.profit),
                    )
                })
                .collect(),
            founded_from: world.trade.founded_from.get(&id).map(|&o| VillageRef {
                id: o,
                name: world.village_name(o),
            }),
            chief: polity.and_then(|p| p.chief).map(|c| {
                person_ref(
                    world,
                    c,
                    if world.people[c as usize].female {
                        "cheffe"
                    } else {
                        "chef"
                    },
                )
            }),
            council: polity
                .map(|p| {
                    p.council
                        .iter()
                        .map(|&c| person_ref(world, c, "conseil"))
                        .collect()
                })
                .unwrap_or_default(),
            tax: world.society.tax(id),
            next_election: match polity.filter(|p| p.chief.is_some()) {
                Some(p) => {
                    let days = p.next_election.saturating_sub(tick) / hameaux_core::tuning::DAY;
                    match days {
                        0 => "élection demain".to_string(),
                        1 => "prochaine élection dans un jour".to_string(),
                        d => format!("prochaine élection dans {d} jours"),
                    }
                }
                None => "pas encore de chef".to_string(),
            },
            techs: polity
                .map(|p| {
                    p.techs
                        .iter()
                        .map(|k| {
                            let year = Clock::at(k.tick).year;
                            let origin = match (k.by, k.from) {
                                (Some(b), _) => format!(
                                    "découvert ici par {}, an {year}",
                                    world.people[b as usize].name()
                                ),
                                (None, Some(f)) => format!(
                                    "venu {}, an {year}",
                                    hameaux_core::sim::de(&world.village_name(f))
                                ),
                                _ => format!("an {year}"),
                            };
                            TechView {
                                label: k.tech.label().into(),
                                effect: k.tech.effect().into(),
                                origin,
                                by: k.by,
                                from: k.from.map(|f| VillageRef {
                                    id: f,
                                    name: world.village_name(f),
                                }),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
            sick: people
                .iter()
                .filter(|p| world.society.sick.contains_key(&p.id))
                .count() as u32,
            epidemic: polity.and_then(|p| p.epidemic).map(|d| {
                let dead = polity.map_or(0, |p| p.epidemic_dead);
                match dead {
                    0 => format!("épidémie de {}", d.label()),
                    1 => format!("épidémie de {} (un mort)", d.label()),
                    k => format!("épidémie de {} ({k} morts)", d.label()),
                }
            }),
            festivities: festivities(world, id),
            events: world
                .chronicle
                .events
                .iter()
                .rev()
                .filter(|e| e.village == Some(id))
                .take(15)
                .map(|e| EventView::of(world, e))
                .collect(),
        })
    }
}

// ------------------------------------------------------------------ chronique

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventView {
    #[ts(type = "number")]
    pub seq: u64,
    #[ts(type = "number")]
    pub tick: u64,
    pub date: String,
    pub village: Option<u32>,
    pub village_name: Option<String>,
    pub people: Vec<u32>,
    pub text: String,
}

impl EventView {
    pub fn of(world: &World, e: &Event) -> Self {
        let c = Clock::at(e.tick);
        EventView {
            seq: e.seq,
            tick: e.tick,
            date: format!(
                "An {}, {} j{} {:02}:{:02}",
                c.year,
                c.season.label(),
                c.day,
                c.hour,
                c.minute
            ),
            village: e.village,
            village_name: e
                .village
                .and_then(|id| world.village_index(id))
                .map(|vi| world.villages[vi].name.clone()),
            people: e.people.clone(),
            text: e.text.clone(),
        }
    }
}

/// Réponse de `GET /api/map/changes?since=N` : tronçons modifiés depuis la révision `since`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MapChanges {
    #[ts(type = "number")]
    pub rev: u64,
    pub chunks: Vec<[u32; 2]>,
}

impl MapChanges {
    pub fn since(world: &World, since: u64) -> Self {
        let cx = world.map.chunks_x();
        let chunks = world
            .chunk_rev
            .iter()
            .enumerate()
            .filter(|&(_, &r)| r > since)
            .map(|(i, _)| [i as u32 % cx, i as u32 / cx])
            .collect();
        MapChanges {
            rev: world.map_rev,
            chunks,
        }
    }
}

/// Liens d'un village avec les autres : trajets parcourus et nouvelles reçues.
fn links(world: &World, vi: usize) -> Vec<LinkView> {
    let id = world.villages[vi].id;
    let here = world.economy.markets.get(vi).map(|m| m.price);
    let known = world.trade.news.get(&id);
    let mut out: Vec<LinkView> = world
        .villages
        .iter()
        .filter(|v| v.id != id)
        .filter_map(|v| {
            let route = world.trade.route(id, v.id).filter(|r| r.reachable());
            let news = known.and_then(|k| k.get(&v.id));
            if route.is_none_or(|r| r.trips == 0 && !r.road) && news.is_none() {
                return None;
            }
            let trips = route.map_or(0, |r| r.trips);
            let way = match route {
                Some(r) if r.road => "route",
                Some(r) if r.trips > 0 => "chemin",
                _ => "",
            };
            let (news_label, dearer) = match (news, here) {
                (Some(n), Some(here)) => {
                    let days = world.tick.saturating_sub(n.tick) / hameaux_core::tuning::DAY;
                    let label = match days {
                        0 => "aujourd'hui".to_string(),
                        1 => "hier".to_string(),
                        d => format!("il y a {d} jours"),
                    };
                    let dearer = Good::ALL
                        .iter()
                        .filter(|&&g| n.price[g as usize] > here[g as usize] * 1.5)
                        .map(|g| g.label().to_string())
                        .collect();
                    (label, dearer)
                }
                _ => (String::new(), Vec::new()),
            };
            Some(LinkView {
                village: VillageRef {
                    id: v.id,
                    name: v.name.clone(),
                },
                way: way.into(),
                trips,
                news: news_label,
                dearer,
            })
        })
        .collect();
    out.sort_by_key(|l| std::cmp::Reverse((l.trips, !l.news.is_empty())));
    out
}
