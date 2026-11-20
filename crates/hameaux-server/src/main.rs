//! Serveur Hameaux : fait tourner le monde en continu et le sert au navigateur.
//!
//! `hameaux-server` lance le serveur ; `hameaux-server healthcheck` interroge `/health` (pour Docker).

mod config;
mod sim;
mod store;
mod web;

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use hameaux_core::save::Loaded;
use hameaux_worldgen::Params;
use tokio::sync::oneshot;
use tracing_subscriber::EnvFilter;

use crate::{config::Config, sim::Command, store::Store};

fn main() -> Result<()> {
    let config = Config::from_env()?;
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        std::process::exit(if healthcheck(config.bind) { 0 } else { 1 });
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        // Pas de couleurs dans `docker compose logs` ni dans un fichier.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();
    tracing::info!(?config, "démarrage de Hameaux {}", env!("CARGO_PKG_VERSION"));

    let store = Store::open(&config.data_dir, config.keep_saves, config.keep_daily)?;
    let params = Params {
        width: config.map_size,
        height: config.map_size,
        villages: config.villages,
        ..Params::default()
    };
    let world = match store.load_latest()? {
        Some(Loaded::Current(w)) => {
            tracing::info!(seed = w.seed, tick = w.tick, "monde rechargé");
            w
        }
        Some(Loaded::V1 { seed, tick }) => {
            // Sauvegarde de la phase 0 (sans carte) : on génère la carte, on garde le temps.
            tracing::info!(seed, tick, "ancienne sauvegarde (v1) : génération de la carte");
            hameaux_worldgen::generate_at(seed, &params, tick)
        }
        Some(Loaded::V2 {
            seed,
            tick,
            map,
            sites,
        }) => {
            // Sauvegarde de la phase 1 : même carte, on fonde les villages et on garde le temps.
            tracing::info!(
                seed,
                tick,
                "sauvegarde v2 : fondation des villages sur la carte existante"
            );
            hameaux_core::World::from_map_at(seed, map, &sites, tick)
        }
        None => {
            let seed = config.seed.unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64
            });
            tracing::info!(seed, size = config.map_size, "aucune sauvegarde : nouveau monde");
            hameaux_worldgen::generate(seed, &params)
        }
    };
    tracing::info!(villages = world.villages.len(), "monde prêt");

    let sim = sim::spawn(&config, store, world);
    let state = web::AppState {
        commands: sim.commands.clone(),
        snapshots: sim.snapshots.clone(),
    };
    let app = web::router(state, &config.web_dir);

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(config.bind)
            .await
            .with_context(|| format!("écoute sur {}", config.bind))?;
        tracing::info!("en écoute sur http://{}", config.bind);
        let commands = sim.commands.clone();
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_signal().await;
                tracing::info!("arrêt demandé, sauvegarde du monde…");
                let (tx, rx) = oneshot::channel();
                if commands.send(Command::Shutdown(tx)).is_ok() {
                    let _ = rx.await;
                }
            })
            .await
            .context("serveur HTTP")
    })?;
    let _ = sim.thread.join();
    tracing::info!("arrêté");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("écoute de SIGTERM");
        tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}

/// `GET /health` en HTTP brut (l'image Docker n'a ni curl ni wget).
fn healthcheck(bind: SocketAddr) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], bind.port()));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(3)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    if s.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 16];
    s.read(&mut buf)
        .map(|n| buf[..n].starts_with(b"HTTP/1.1 200") || buf[..n].starts_with(b"HTTP/1.0 200"))
        .unwrap_or(false)
}
