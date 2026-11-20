//! Boucle de simulation, sur son propre fil.
//!
//! Elle avance le monde au rythme voulu, publie un instantané après chaque lot de ticks
//! (lu par le web sans jamais bloquer la simulation) et sauvegarde régulièrement.

use std::{
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use hameaux_core::{World, clock::TICK_SECONDS, save};
use hameaux_protocol::RunState;
use tokio::sync::{oneshot, watch};

use crate::{config::Config, store::Store};

/// Au-delà de ce retard (en ticks), on abandonne le rattrapage plutôt que de geler le serveur.
const MAX_BACKLOG_TICKS: f64 = 600.0;

pub enum Command {
    Pause,
    Resume,
    SetSpeed(f64),
    SaveNow(oneshot::Sender<Result<u64, String>>),
    /// Sauvegarde puis arrête la boucle ; répond quand c'est fait.
    Shutdown(oneshot::Sender<()>),
}

/// Ce que le web voit du monde.
pub struct Snapshot {
    pub world: World,
    pub run: RunState,
}

pub struct SimHandle {
    pub commands: mpsc::Sender<Command>,
    pub snapshots: watch::Receiver<Arc<Snapshot>>,
    pub thread: thread::JoinHandle<()>,
}

pub fn spawn(config: &Config, store: Store, world: World) -> SimHandle {
    let run = RunState {
        speed: config.speed,
        paused: false,
    };
    let (snap_tx, snap_rx) = watch::channel(Arc::new(Snapshot {
        world: world.clone(),
        run: run.clone(),
    }));
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let save_every = config.save_every;
    let thread = thread::Builder::new()
        .name("simulation".into())
        .spawn(move || {
            Loop {
                world,
                run,
                store: Arc::new(store),
                snap_tx,
                save_every,
            }
            .run(cmd_rx)
        })
        .expect("lancement du fil de simulation");
    SimHandle {
        commands: cmd_tx,
        snapshots: snap_rx,
        thread,
    }
}

struct Loop {
    world: World,
    run: RunState,
    store: Arc<Store>,
    snap_tx: watch::Sender<Arc<Snapshot>>,
    save_every: Duration,
}

impl Loop {
    fn run(mut self, commands: mpsc::Receiver<Command>) {
        let mut last = Instant::now();
        let mut last_save = Instant::now();
        let mut pending = 0.0f64;
        let mut last_publish = Instant::now();
        let mut wait = Duration::ZERO;
        loop {
            // Attente du prochain tick, interrompue par les commandes.
            let mut first = Some(commands.recv_timeout(wait));
            loop {
                let cmd = match first.take() {
                    Some(Ok(c)) => c,
                    Some(Err(mpsc::RecvTimeoutError::Timeout)) => break,
                    Some(Err(mpsc::RecvTimeoutError::Disconnected)) => return self.final_save(None),
                    None => match commands.try_recv() {
                        Ok(c) => c,
                        Err(_) => break,
                    },
                };
                match cmd {
                    Command::Pause => self.run.paused = true,
                    Command::Resume => self.run.paused = false,
                    Command::SetSpeed(s) => self.run.speed = s,
                    Command::SaveNow(reply) => {
                        let _ = reply.send(self.save_sync().map_err(|e| format!("{e:#}")));
                        last_save = Instant::now();
                    }
                    Command::Shutdown(reply) => return self.final_save(Some(reply)),
                }
                self.publish();
            }

            let now = Instant::now();
            let tick_secs = TICK_SECONDS as f64 / self.run.speed;
            if !self.run.paused {
                pending += (now - last).as_secs_f64() / tick_secs;
            }
            last = now;
            if pending > MAX_BACKLOG_TICKS {
                tracing::warn!(
                    retard = pending as u64,
                    "la simulation prend du retard, ticks abandonnés"
                );
                pending = MAX_BACKLOG_TICKS;
            }
            let due = pending.floor();
            for _ in 0..due as u64 {
                self.world.step();
            }
            pending -= due;
            // Instantané au plus 10 fois par seconde : copier le monde a un coût.
            if due > 0.0 && last_publish.elapsed() >= Duration::from_millis(100) {
                self.publish();
                last_publish = Instant::now();
            }

            if last_save.elapsed() >= self.save_every {
                self.save_async();
                last_save = Instant::now();
            }

            wait = if self.run.paused {
                Duration::from_millis(500)
            } else {
                Duration::from_secs_f64(((1.0 - pending) * tick_secs).clamp(0.001, 0.2))
            };
        }
    }

    fn publish(&self) {
        self.snap_tx.send_replace(Arc::new(Snapshot {
            world: self.world.clone(),
            run: self.run.clone(),
        }));
    }

    fn save_sync(&self) -> anyhow::Result<u64> {
        let bytes = save::encode(&self.world)?;
        self.store.write(self.world.tick, &bytes)?;
        Ok(self.world.tick)
    }

    /// Encode ici (l'état ne bouge pas pendant ce temps), écrit sur un autre fil.
    fn save_async(&self) {
        let tick = self.world.tick;
        match save::encode(&self.world) {
            Ok(bytes) => {
                let store = self.store.clone();
                thread::spawn(move || {
                    if let Err(e) = store.write(tick, &bytes) {
                        tracing::error!("sauvegarde automatique : {e:#}");
                    }
                });
            }
            Err(e) => tracing::error!("encodage de la sauvegarde : {e}"),
        }
    }

    fn final_save(self, reply: Option<oneshot::Sender<()>>) {
        match self.save_sync() {
            Ok(tick) => tracing::info!(tick, "monde sauvegardé avant l'arrêt"),
            Err(e) => tracing::error!("sauvegarde finale impossible : {e:#}"),
        }
        if let Some(r) = reply {
            let _ = r.send(());
        }
    }
}
