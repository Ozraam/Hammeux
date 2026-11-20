//! Réglages lus dans les variables d'environnement (`.env` avec docker compose).

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    /// Adresse d'écoute (`HAMEAUX_BIND`, défaut `0.0.0.0:8080`).
    pub bind: SocketAddr,
    /// Dossier des sauvegardes (`HAMEAUX_DATA_DIR`, défaut `./data`).
    pub data_dir: PathBuf,
    /// Client web compilé (`HAMEAUX_WEB_DIR`, défaut `./web/dist`).
    pub web_dir: PathBuf,
    /// Graine d'un nouveau monde (`HAMEAUX_SEED`) ; ignorée si une sauvegarde existe.
    pub seed: Option<u64>,
    /// Côté de la carte d'un nouveau monde, en cases (`HAMEAUX_MAP_SIZE`, défaut 1024).
    pub map_size: u32,
    /// Nombre de villages d'un nouveau monde (`HAMEAUX_VILLAGES`, défaut 12).
    pub villages: usize,
    /// Secondes de jeu par seconde réelle (`HAMEAUX_SPEED`, défaut 60 : un jour en 24 min).
    pub speed: f64,
    /// Intervalle des sauvegardes automatiques (`HAMEAUX_SAVE_EVERY`, en secondes, défaut 300).
    pub save_every: Duration,
    /// Nombre de sauvegardes récentes gardées (`HAMEAUX_KEEP_SAVES`, défaut 48).
    pub keep_saves: usize,
    /// Nombre de sauvegardes quotidiennes gardées (`HAMEAUX_KEEP_DAILY`, défaut 30).
    pub keep_daily: usize,
}

fn var<T: std::str::FromStr>(name: &str, default: T) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("{name}={v} invalide")),
        _ => Ok(default),
    }
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let seed = match std::env::var("HAMEAUX_SEED") {
            Ok(v) if !v.trim().is_empty() => Some(v.trim().parse().context("HAMEAUX_SEED invalide")?),
            _ => None,
        };
        let speed: f64 = var("HAMEAUX_SPEED", 60.0)?;
        anyhow::ensure!(
            speed > 0.0 && speed <= 1e6,
            "HAMEAUX_SPEED doit être entre 0 et 1 000 000"
        );
        Ok(Config {
            bind: var("HAMEAUX_BIND", "0.0.0.0:8080".parse().unwrap())?,
            data_dir: var("HAMEAUX_DATA_DIR", PathBuf::from("./data"))?,
            web_dir: var("HAMEAUX_WEB_DIR", PathBuf::from("./web/dist"))?,
            seed,
            map_size: var("HAMEAUX_MAP_SIZE", 1024u32)?.clamp(128, 4096),
            villages: var("HAMEAUX_VILLAGES", 12usize)?.clamp(1, 64),
            speed,
            save_every: Duration::from_secs(var("HAMEAUX_SAVE_EVERY", 300u64)?.max(10)),
            keep_saves: var("HAMEAUX_KEEP_SAVES", 48usize)?.max(1),
            keep_daily: var("HAMEAUX_KEEP_DAILY", 30usize)?,
        })
    }
}
