//! Outils sans serveur : générer un monde et en faire une image, ou l'avancer en accéléré.
//!
//! ```text
//! hameaux-cli map  [--seed N] [--size N] [--out carte.png]   # aperçu de la carte
//! hameaux-cli run  [--seed N] [--size N] [--days N]          # simulation accélérée
//! ```

use std::{fs::File, io::BufWriter, time::Instant};

use hameaux_core::{Biome, Clock, World, clock::TICKS_PER_DAY, settlement::HouseState};
use hameaux_worldgen::Params;

/// Couleurs des milieux : les mêmes que dans `web/src/palette.ts`.
const BIOME_COLORS: [u32; 13] = [
    0x1f4e79, 0x2f6fa3, 0x3f88c5, 0xe9d8a6, 0xb5c46a, 0x8fbf5a, 0x4f8a3c, 0x356b45, 0x6b8f71, 0xa79c6d,
    0x9a8f62, 0x8a8178, 0xf2f4f7,
];
const RIVER: u32 = 0x3f88c5;
const ROAD: u32 = 0xc8a46a;

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| usage());
    let (mut seed, mut size, mut days, mut out) = (42u64, 1024u32, 7u64, String::from("carte.png"));
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--seed" => seed = value().parse().unwrap_or_else(|_| usage()),
            "--size" => size = value().parse().unwrap_or_else(|_| usage()),
            "--days" => days = value().parse().unwrap_or_else(|_| usage()),
            "--out" => out = value(),
            _ => usage(),
        }
    }
    let params = Params {
        width: size,
        height: size,
        ..Params::default()
    };

    let start = Instant::now();
    let mut world = hameaux_worldgen::generate(seed, &params);
    println!(
        "monde {seed} ({size}×{size}) généré en {:.2} s",
        start.elapsed().as_secs_f64()
    );

    match cmd.as_str() {
        "map" => {
            report(&world);
            write_png(&world, &out);
            println!("aperçu écrit dans {out}");
        }
        "run" => {
            weekly(&world, 0);
            let start = Instant::now();
            let mut tally: std::collections::BTreeMap<String, usize> = Default::default();
            let mut seen = world.chronicle.next_seq;
            // Part des adultes au travail / en visite, relevée à 10 h et 15 h.
            let (mut working, mut social, mut samples) = (0.0f32, 0.0f32, 0.0f32);
            let mut intents = [0.0f32; 11];
            let mut intent_samples = 0.0f32;
            for day in 1..=days {
                for _ in 0..TICKS_PER_DAY {
                    world.step();
                    let c = world.clock();
                    if (c.hour == 10 || c.hour == 15)
                        && world.tick.is_multiple_of(hameaux_core::tuning::HOUR)
                        && !hameaux_core::sim::rest_day(world.tick)
                    {
                        let s = world.mind_stats();
                        working += s.working;
                        for (k, v) in s.intents.iter().enumerate() {
                            intents[k] += v;
                        }
                        intent_samples += 1.0;
                        social += s.social;
                        samples += 1.0;
                    }
                }
                for e in world.chronicle.since(seen) {
                    // Type d'événement : le premier mot et les mots sans majuscule (sans les noms).
                    let kind: String = e
                        .text
                        .split_whitespace()
                        .enumerate()
                        .filter(|(k, w)| {
                            let name = hameaux_core::names::FEMALE.contains(w)
                                || hameaux_core::names::MALE.contains(w);
                            (*k == 0 && !name) || w.chars().next().is_some_and(|c| !c.is_uppercase())
                        })
                        .map(|(_, w)| w)
                        .take(5)
                        .collect::<Vec<_>>()
                        .join(" ");
                    *tally.entry(kind).or_default() += 1;
                }
                seen = world.chronicle.next_seq;
                if day % 7 == 0 || day == days {
                    weekly(&world, day);
                    let s = world.mind_stats();
                    let needs: Vec<String> = hameaux_core::mind::NEED_LABELS
                        .iter()
                        .zip(s.needs)
                        .map(|(l, v)| format!("{l} {v:.2}"))
                        .collect();
                    println!(
                        "    esprits : humeur {:+.2} · bien-être {:.2} · compétence {:.2} · {:.0} relations · {:.1} souvenirs · au travail {:.0} % · en visite {:.0} %",
                        s.mood,
                        s.wellbeing,
                        s.skill,
                        s.relations,
                        s.memories,
                        100.0 * working / samples.max(1.0),
                        100.0 * social / samples.max(1.0),
                    );
                    println!("        {}", needs.join(" · "));
                    let levels: Vec<String> = hameaux_core::sim::LEVEL_LABELS
                        .iter()
                        .zip(s.levels)
                        .map(|(l, v)| format!("{l} {:.0} %", 100.0 * v))
                        .collect();
                    println!("        métiers : {}", levels.join(" · "));
                    (working, social, samples) = (0.0, 0.0, 0.0);
                }
            }
            let secs = start.elapsed().as_secs_f64();
            println!(
                "{} · {} ticks en {secs:.1} s ({:.0} ticks/s)",
                world.clock(),
                world.tick,
                world.tick as f64 / secs
            );
            if let Some(m) = world.economy.markets.first() {
                use hameaux_core::economy::Good;
                println!("— marché de {} —", world.villages[0].name);
                for g in Good::ALL {
                    let i = g as usize;
                    println!(
                        "  {:<8} prix {:>6.2}  offre {:>6.1}  demande {:>6.1}  vendu {:>6.1}",
                        g.label(),
                        m.price[i],
                        m.supply[i],
                        m.demand[i],
                        m.volume[i]
                    );
                }
            }
            println!("— intentions des adultes en âge de travailler (10 h et 15 h, jours ouvrés) —");
            for (l, v) in hameaux_core::sim::INTENT_LABELS.iter().zip(intents) {
                if v > 0.0 {
                    println!("  {l:<12} {:>5.1} %", 100.0 * v / intent_samples.max(1.0));
                }
            }
            let tick = world.tick;
            let (mut idle, mut adults) = (0, 0);
            for (i, p) in world.people.iter().enumerate() {
                if !p.alive() || p.child(tick) || p.elder(tick) {
                    continue;
                }
                let (Some(job), Some(vi)) = (p.job, world.village_index(p.village)) else {
                    continue;
                };
                adults += 1;
                if job != hameaux_core::people::Job::Builder
                    && world.job_value(vi, job) < 0.6
                    && world
                        .home_household(i)
                        .is_some_and(|h| h.coins > 10.0 && h.rations() > 42.0)
                {
                    idle += 1;
                }
            }
            println!("  métier qui ne rapporte rien (chômage) : {idle} sur {adults}");
            println!("— événements par type —");
            let mut kinds: Vec<_> = tally.into_iter().collect();
            kinds.sort_by_key(|k| std::cmp::Reverse(k.1));
            for (k, n) in kinds.iter().take(60) {
                println!("  {n:>6}  {k}…");
            }
            println!("— derniers événements —");
            let n = world.chronicle.events.len();
            for e in world.chronicle.events.iter().skip(n.saturating_sub(30)) {
                println!("  [{}] {}", Clock::at(e.tick), e.text);
            }
        }
        _ => usage(),
    }
}

fn weekly(world: &World, day: u64) {
    use hameaux_core::economy::Good;
    let pop = world.alive().count();
    println!("jour {day:>3} · {} · {pop} habitants", world.clock());
    let tick = world.tick;
    for (vi, v) in world.villages.iter().enumerate() {
        let houses = v
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Inhabited)
            .count();
        let homes: Vec<_> = v
            .houses
            .iter()
            .filter_map(|h| world.economy.households.get(&h.id))
            .collect();
        let food: f32 = homes.iter().map(|h| h.rations()).sum();
        let coins: f32 = homes.iter().map(|h| h.coins).sum();
        let vpop = world.population(v.id).max(1) as f32;
        let price = |g: Good| world.economy.markets.get(vi).map_or(0.0, |m| m.price[g as usize]);
        let mut jobs = std::collections::BTreeMap::new();
        for p in world
            .alive()
            .filter(|p| p.village == v.id && !p.child(tick) && !p.elder(tick))
        {
            if let Some(j) = p.job {
                *jobs.entry(j.label(false)).or_insert(0) += 1;
            }
        }
        let shops: Vec<&str> = v
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Inhabited)
            .filter_map(|h| world.economy.workshops.get(&h.id).map(|w| w.label()))
            .collect();
        println!(
            "    {:<22} {:>3} hab. {:>2} maisons · vivres {:>3.0} j · {:>4.0} sous/foyer · caisse {:>4.0} · grain {:.2} pain {:.2} bois {:.2} outils {:.1}{}",
            v.name,
            world.population(v.id),
            houses,
            food / vpop,
            coins / homes.len().max(1) as f32,
            world.economy.treasury.get(vi).copied().unwrap_or(0.0),
            price(Good::Grain),
            price(Good::Bread),
            price(Good::Wood),
            price(Good::Tools),
            if v.hungry_days > 0 { " · FAIM" } else { "" }
        );
        let jobs: Vec<String> = jobs.iter().map(|(j, n)| format!("{n} {j}")).collect();
        println!("        {} | {}", jobs.join(", "), shops.join(", "));
    }
}

fn report(world: &World) {
    let m = &world.map;
    let mut counts = [0usize; 13];
    for &b in &m.biome {
        counts[b as usize] += 1;
    }
    for b in Biome::ALL {
        let c = counts[b as usize];
        if c > 0 {
            println!("  {:<20} {:>5.1} %", b.label(), c as f64 * 100.0 / m.len() as f64);
        }
    }
    let rivers = m.flags.iter().filter(|&&f| f & 3 > 0).count();
    println!("  rivières : {rivers} cases");
    for v in &world.villages {
        println!(
            "  village {:>2} {:<28} ({}, {}) {} hab.",
            v.id,
            v.name,
            v.x,
            v.y,
            world.population(v.id)
        );
    }
}

fn write_png(world: &World, path: &str) {
    let m = &world.map;
    let mut rgb = vec![0u8; m.len() * 3];
    let mut put = |i: usize, c: u32, k: f32| {
        for (ch, s) in [16, 8, 0].into_iter().enumerate() {
            rgb[i * 3 + ch] = (((c >> s) & 0xff) as f32 * k).min(255.0) as u8;
        }
    };
    for i in 0..m.len() {
        let mut c = BIOME_COLORS[m.biome[i] as usize];
        if m.road_at(i) {
            c = ROAD;
        } else if m.river_at(i) > 0 && !m.biome_at(i).is_water() {
            c = RIVER;
        }
        put(i, c, m.shade[i] as f32 / 128.0);
    }
    for v in &world.villages {
        for dy in -3i64..=3 {
            for dx in -3i64..=3 {
                let (x, y) = (v.x as i64 + dx, v.y as i64 + dy);
                if !m.inside(x, y) {
                    continue;
                }
                let c = if dx.abs() == 3 || dy.abs() == 3 {
                    0xffffff
                } else {
                    v.color
                };
                put((y * m.width as i64 + x) as usize, c, 1.0);
            }
        }
    }
    let file = BufWriter::new(File::create(path).expect("création de l'image"));
    let mut enc = png::Encoder::new(file, m.width, m.height);
    enc.set_color(png::ColorType::Rgb);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgb))
        .expect("écriture de l'image");
}

fn usage() -> ! {
    eprintln!("usage : hameaux-cli map|run [--seed N] [--size N] [--days N] [--out fichier.png]");
    std::process::exit(2)
}
