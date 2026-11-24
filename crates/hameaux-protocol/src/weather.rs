//! Météo et risques (phase 10) : fronts et incendies vus de la carte, temps d'un village.

use hameaux_core::{World, weather::SKY_LABELS};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Un front vu de la carte (coordonnées en cases).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FrontView {
    pub id: u32,
    /// 0 pluie, 1 orage, 2 temps sec, 3 vague de froid.
    pub kind: u8,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    /// Force en ce moment (0–1).
    pub strength: f32,
    /// Cases par heure de jeu.
    pub vx: f32,
    pub vy: f32,
}

/// Un incendie de forêt : où il brûle et combien de cases sont en feu.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FireView {
    pub id: u32,
    /// Centre des cases en feu.
    pub x: f32,
    pub y: f32,
    pub burning: u32,
    pub burnt: u32,
}

/// Le temps sur toute la carte (avec les villages, toutes les quelques secondes).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WeatherMap {
    pub fronts: Vec<FrontView>,
    pub fires: Vec<FireView>,
    /// L'hiver en cours est rude.
    pub harsh_winter: bool,
    /// Écart de température de la saison (°C).
    pub anomaly: f32,
}

impl WeatherMap {
    pub fn of(world: &World) -> Self {
        let tick = world.tick;
        let w = &world.weather;
        WeatherMap {
            fronts: w
                .fronts
                .iter()
                .map(|f| FrontView {
                    id: f.id,
                    kind: f.kind as u8,
                    label: f.kind.label().to_string(),
                    x: f.x,
                    y: f.y,
                    radius: f.radius,
                    strength: f.at(f.x, f.y, tick),
                    vx: f.vx,
                    vy: f.vy,
                })
                .collect(),
            fires: w
                .fires
                .iter()
                .filter(|f| !f.burning.is_empty())
                .map(|f| {
                    let n = f.burning.len() as f32;
                    let (sx, sy) = f.burning.iter().fold((0.0, 0.0), |(sx, sy), &(t, _)| {
                        let (x, y) = world.map.xy(t as usize);
                        (sx + x as f32, sy + y as f32)
                    });
                    FireView {
                        id: f.id,
                        x: sx / n + 0.5,
                        y: sy / n + 0.5,
                        burning: f.burning.len() as u32,
                        burnt: f.burnt,
                    }
                })
                .collect(),
            harsh_winter: w.harsh(),
            anomaly: w.anomaly,
        }
    }
}

/// Le temps d'un village (fiche).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SkyView {
    /// Index de `SKY_LABELS` : 0 beau temps, 1 averses, 2 pluie, 3 orage, 4 neige.
    pub sky: u8,
    /// « pluie », « beau temps »…
    pub label: String,
    /// Température en ce moment et moyenne de la veille (°C).
    pub temp: f32,
    pub temp_mean: f32,
    /// Humidité du sol (0–1) et ce qu'elle veut dire (« sol craquelé »…).
    pub wet: f32,
    pub soil: String,
    /// Neige au sol (0–1).
    pub snow: f32,
    pub dry_days: u32,
    /// Ce que le temps fait à la pousse des champs (1 : normal) ; rien ne pousse l'hiver.
    pub crop: f32,
    pub winter: bool,
    /// Une rivière passe près des champs (0 aucune … 3 fleuve).
    pub river: u8,
    /// Crue en cours.
    pub flooded: bool,
    /// Incendie de forêt en cours près du village (cases en feu).
    pub fire: u32,
    /// Hiver rude en cours.
    pub harsh_winter: bool,
    /// Bois de chauffe par personne et par jour.
    pub heating: f32,
    /// Trajets réputés dangereux depuis ce village : (village, danger 0–~3).
    pub dangers: Vec<(String, f32)>,
}

pub fn sky_view(world: &World, vi: usize) -> SkyView {
    let w = &world.weather;
    let l = w.local(vi);
    let id = world.villages[vi].id;
    let sky = l.sky();
    let mut dangers: Vec<(String, f32)> = w
        .danger
        .iter()
        .filter(|((a, b), _)| *a == id || *b == id)
        .map(|((a, b), &d)| (world.village_name(if *a == id { *b } else { *a }), d))
        .filter(|(_, d)| *d >= 0.2)
        .collect();
    dangers.sort_by(|a, b| b.1.total_cmp(&a.1));
    SkyView {
        sky,
        label: SKY_LABELS[sky as usize].to_string(),
        temp: l.temp,
        temp_mean: l.temp_mean,
        wet: l.wet,
        soil: if l.snow > 0.3 {
            "sous la neige"
        } else if l.wet < 0.12 {
            "sol craquelé"
        } else if l.wet < 0.3 {
            "sol sec"
        } else if l.wet <= 0.88 {
            "sol frais"
        } else {
            "sol détrempé"
        }
        .to_string(),
        snow: l.snow,
        dry_days: l.dry_days,
        crop: l.crop,
        winter: world.clock().season == hameaux_core::Season::Winter,
        river: if l.river == 255 { 0 } else { l.river },
        flooded: l.flooded_until > world.tick,
        fire: w
            .fires
            .iter()
            .filter(|f| f.village == Some(id))
            .map(|f| f.burning.len() as u32)
            .sum(),
        harsh_winter: w.harsh(),
        heating: world.heating(id),
        dangers,
    }
}

/// Le temps d'un village, avec son nom.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VillageSky {
    pub id: u32,
    pub name: String,
    pub sky: SkyView,
}

/// Réponse de `GET /api/weather` : la carte du temps, chaque village et le bilan des
/// catastrophes depuis le début du monde.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WeatherReport {
    pub map: WeatherMap,
    pub villages: Vec<VillageSky>,
    /// (libellé, valeur) : orages, crues, incendies, hivers rudes, brigands…
    pub figures: Vec<(String, String)>,
}

impl WeatherReport {
    pub fn of(world: &World) -> Self {
        let st = &world.weather.stats;
        let figures = [
            ("Fronts passés", st.fronts.to_string()),
            ("Orages", st.storms.to_string()),
            ("Gels tardifs", st.frosts.to_string()),
            ("Hivers rudes", st.harsh_winters.to_string()),
            ("Morts de froid", st.cold_deaths.to_string()),
            ("Crues", st.floods.to_string()),
            ("Champs perdus", st.fields_lost.to_string()),
            ("Incendies de forêt", st.fires.to_string()),
            (
                "Cases brûlées",
                format!("{} ({} repoussées)", st.burnt_tiles, st.regrown_tiles),
            ),
            (
                "Marchands détroussés",
                format!(
                    "{} ({:.0} sous, {:.0} unités)",
                    st.robberies, st.stolen_coins, st.stolen_goods
                ),
            ),
            (
                "Charrettes versées",
                format!("{} ({:.0} unités perdues)", st.mishaps, st.lost_goods),
            ),
            ("Heures de retard sur les routes", st.delays.to_string()),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b))
        .collect();
        WeatherReport {
            map: WeatherMap::of(world),
            villages: (0..world.villages.len())
                .map(|vi| VillageSky {
                    id: world.villages[vi].id,
                    name: world.villages[vi].name.clone(),
                    sky: sky_view(world, vi),
                })
                .collect(),
            figures,
        }
    }
}
