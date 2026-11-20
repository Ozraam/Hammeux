//! Vues du monde envoyées au navigateur : scène visible, fiches, chronique.

use hameaux_core::{
    Clock, World,
    events::Event,
    people::{Person, Status},
    settlement::{FieldState, HouseState},
    sim::lien_de,
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
    /// Index de `Carry` + 1 (0 = rien).
    pub carry: u8,
    /// Vêtement, peau, cheveux.
    pub looks: [u8; 3],
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
                carry: p.brain.carry.map_or(0, |c| c as u8 + 1),
                looks: p.looks,
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
    /// Meilleurs amis (score dans `note`).
    pub friends: Vec<PersonRef>,
    pub master: Option<PersonRef>,
    pub events: Vec<EventView>,
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
        if let Some(c) = p.brain.carry
            && p.alive()
            && !p.brain.hidden
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

        let mut friends: Vec<(u32, f32)> = p
            .friends
            .iter()
            .map(|(&k, &v)| (k, v))
            .filter(|&(_, s)| s >= 5.0)
            .collect();
        friends.sort_by(|a, b| b.1.total_cmp(&a.1));
        let friends = friends
            .into_iter()
            .filter(|(f, _)| world.people[*f as usize].alive())
            .take(5)
            .map(|(f, s)| person_ref(world, f, lien_de(s)))
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
            friends,
            master: p
                .master
                .filter(|_| tick < p.apprentice_until)
                .map(|m| person_ref(world, m, "maître")),
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
    pub events: Vec<EventView>,
}

impl VillageSheet {
    pub fn of(world: &World, id: u32) -> Option<Self> {
        let v = &world.villages[world.village_index(id)?];
        let tick = world.tick;
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
                (v.stock.food / need) as u32
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
