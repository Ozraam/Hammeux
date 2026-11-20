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
            for day in 1..=days {
                for _ in 0..TICKS_PER_DAY {
                    world.step();
                }
                for e in world.chronicle.since(seen) {
                    let kind: String = e.text.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
                    *tally.entry(kind).or_default() += 1;
                }
                seen = world.chronicle.next_seq;
                if day % 7 == 0 || day == days {
                    weekly(&world, day);
                }
            }
            let secs = start.elapsed().as_secs_f64();
            println!(
                "{} · {} ticks en {secs:.1} s ({:.0} ticks/s)",
                world.clock(),
                world.tick,
                world.tick as f64 / secs
            );
            println!("— événements par type —");
            let mut kinds: Vec<_> = tally.into_iter().collect();
            kinds.sort_by_key(|k| std::cmp::Reverse(k.1));
            for (k, n) in kinds.iter().take(25) {
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
    let pop = world.alive().count();
    println!("jour {day:>3} · {} · {pop} habitants", world.clock());
    for v in &world.villages {
        let houses = v
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Inhabited)
            .count();
        let sites = v
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Building)
            .count();
        println!(
            "    {:<26} {:>3} hab. {:>2} maisons (+{sites}) {:>2} champs · nourriture {:>5.0} bois {:>3} pierre {:>3}",
            v.name,
            world.population(v.id),
            houses,
            v.fields.len(),
            v.stock.food,
            v.stock.wood,
            v.stock.stone
        );
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
