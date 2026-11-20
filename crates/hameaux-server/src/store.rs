//! Sauvegardes sur disque.
//!
//! - `saves/world-<tick>.hameau` : les plus récentes (rotation à `keep_saves`).
//! - `saves/daily/<AAAA-MM-JJ>.hameau` : une par jour réel (rotation à `keep_daily`).
//!
//! Chaque écriture passe par un fichier temporaire puis un renommage : une coupure de
//! courant laisse l'ancienne sauvegarde intacte.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use hameaux_core::save::{self, Loaded};

pub struct Store {
    dir: PathBuf,
    daily: PathBuf,
    keep: usize,
    keep_daily: usize,
}

impl Store {
    pub fn open(data_dir: &Path, keep: usize, keep_daily: usize) -> Result<Self> {
        let dir = data_dir.join("saves");
        let daily = dir.join("daily");
        fs::create_dir_all(&daily).with_context(|| format!("création de {}", daily.display()))?;
        Ok(Store {
            dir,
            daily,
            keep,
            keep_daily,
        })
    }

    /// Charge la sauvegarde valide la plus récente (les fichiers abîmés sont ignorés).
    pub fn load_latest(&self) -> Result<Option<Loaded>> {
        for path in self
            .list(&self.dir)?
            .into_iter()
            .rev()
            .chain(self.list(&self.daily)?.into_iter().rev())
        {
            match fs::read(&path)
                .map_err(anyhow::Error::from)
                .and_then(|b| Ok(save::decode(&b)?))
            {
                Ok(loaded) => {
                    tracing::info!(file = %path.display(), "sauvegarde lue");
                    return Ok(Some(loaded));
                }
                Err(e) => tracing::warn!(file = %path.display(), "sauvegarde illisible : {e:#}"),
            }
        }
        Ok(None)
    }

    /// Écrit une sauvegarde déjà encodée, puis fait tourner les anciennes.
    pub fn write(&self, tick: u64, bytes: &[u8]) -> Result<()> {
        let path = self.dir.join(format!("world-{tick:012}.hameau"));
        atomic_write(&path, bytes)?;
        let daily = self.daily.join(format!("{}.hameau", today()));
        if self.keep_daily > 0 && !daily.exists() {
            atomic_write(&daily, bytes)?;
        }
        self.rotate(&self.dir, self.keep)?;
        self.rotate(&self.daily, self.keep_daily.max(1))?;
        tracing::debug!(file = %path.display(), size = bytes.len(), "sauvegarde écrite");
        Ok(())
    }

    /// Fichiers `.hameau` d'un dossier, du plus ancien au plus récent (les noms se trient).
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "hameau"))
            .collect();
        files.sort();
        Ok(files)
    }

    fn rotate(&self, dir: &Path, keep: usize) -> Result<()> {
        let files = self.list(dir)?;
        for old in &files[..files.len().saturating_sub(keep)] {
            match fs::remove_file(old) {
                // Une autre écriture a pu passer avant nous.
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(e).with_context(|| format!("suppression de {}", old.display()));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp).with_context(|| format!("écriture de {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        // Rend le renommage durable (sans effet sous macOS, utile sous Linux).
        let _ = fs::File::open(parent).and_then(|d| d.sync_all());
    }
    Ok(())
}

/// Date UTC du jour au format AAAA-MM-JJ (algorithme « civil from days » de H. Hinnant).
fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use hameaux_core::World;

    fn tick_of(l: Loaded) -> u64 {
        match l {
            Loaded::Current(w) => w.tick,
            Loaded::V1 { tick, .. } | Loaded::V2 { tick, .. } => tick,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hameaux-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn writes_rotates_and_reloads_newest() {
        let dir = temp_dir("rotate");
        let store = Store::open(&dir, 3, 2).unwrap();
        let mut w = World::empty(7);
        for _ in 0..5 {
            w.step();
            store.write(w.tick, &save::encode(&w).unwrap()).unwrap();
        }
        assert_eq!(store.list(&store.dir).unwrap().len(), 3);
        assert_eq!(tick_of(store.load_latest().unwrap().unwrap()), 5);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skips_corrupt_newest() {
        let dir = temp_dir("corrupt");
        let store = Store::open(&dir, 10, 0).unwrap();
        let w = World::empty(1);
        store.write(1, &save::encode(&w).unwrap()).unwrap();
        fs::write(store.dir.join("world-000000000009.hameau"), b"abime").unwrap();
        assert_eq!(tick_of(store.load_latest().unwrap().unwrap()), w.tick);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn date_format() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
    }
}
