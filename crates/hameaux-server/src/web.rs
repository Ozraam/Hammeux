//! API HTTP, WebSocket et fichiers du client web.

use std::{path::Path, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{
        Path as AxumPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hameaux_protocol::{
    ClientMsg, ClockView, EventView, Health, MapChanges, MapInfo, PersonSheet, Scene, ServerMsg,
    VillageSheet, VillageView, encode_chunk,
};
use serde::Deserialize;
use tokio::sync::{oneshot, watch};
use tower_http::services::{ServeDir, ServeFile};

use crate::sim::{Command, Snapshot};

/// Intervalle minimal entre deux messages `Tick` envoyés à un navigateur.
const PUSH_EVERY: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub struct AppState {
    pub commands: std::sync::mpsc::Sender<Command>,
    pub snapshots: watch::Receiver<Arc<Snapshot>>,
}

pub fn router(state: AppState, web_dir: &Path) -> Router {
    let index = web_dir.join("index.html");
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws))
        .route("/api/map", get(map_info))
        .route("/api/map/chunk/{cx}/{cy}", get(map_chunk))
        .route("/api/map/changes", get(map_changes))
        .route("/api/events", get(events))
        .route("/api/person/{id}", get(person))
        .route("/api/village/{id}", get(village))
        .route("/api/admin/pause", post(pause))
        .route("/api/admin/resume", post(resume))
        .route("/api/admin/speed", post(speed))
        .route("/api/admin/save", post(save_now))
        .fallback_service(ServeDir::new(web_dir).not_found_service(ServeFile::new(index)))
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}

async fn map_info(State(s): State<AppState>) -> Json<MapInfo> {
    let snap = s.snapshots.borrow().clone();
    Json(MapInfo::of(&snap.world))
}

async fn map_chunk(State(s): State<AppState>, AxumPath((cx, cy)): AxumPath<(u32, u32)>) -> Response {
    let map = s.snapshots.borrow().world.map.clone();
    match encode_chunk(&map, cx, cy) {
        Some(bytes) => ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response(),
        None => (StatusCode::NOT_FOUND, "tronçon hors carte").into_response(),
    }
}

async fn health(State(s): State<AppState>) -> Json<Health> {
    let snap = s.snapshots.borrow().clone();
    Json(Health {
        status: "ok".into(),
        tick: snap.world.tick,
        run: snap.run.clone(),
    })
}

fn send(s: &AppState, cmd: Command) -> Result<(), (StatusCode, String)> {
    s.commands
        .send(cmd)
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "simulation arrêtée".to_string()))
}

async fn pause(State(s): State<AppState>) -> impl IntoResponse {
    send(&s, Command::Pause).map(|_| StatusCode::NO_CONTENT)
}

async fn resume(State(s): State<AppState>) -> impl IntoResponse {
    send(&s, Command::Resume).map(|_| StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SpeedBody {
    speed: f64,
}

async fn speed(State(s): State<AppState>, Json(body): Json<SpeedBody>) -> impl IntoResponse {
    if !(body.speed > 0.0 && body.speed <= 1e6) {
        return Err((
            StatusCode::BAD_REQUEST,
            "speed doit être entre 0 et 1 000 000".to_string(),
        ));
    }
    send(&s, Command::SetSpeed(body.speed)).map(|_| StatusCode::NO_CONTENT)
}

async fn save_now(State(s): State<AppState>) -> impl IntoResponse {
    let (tx, rx) = oneshot::channel();
    send(&s, Command::SaveNow(tx))?;
    match rx.await {
        Ok(Ok(tick)) => Ok(Json(serde_json::json!({ "tick": tick }))),
        Ok(Err(e)) => Err((StatusCode::INTERNAL_SERVER_ERROR, e)),
        Err(_) => Err((StatusCode::SERVICE_UNAVAILABLE, "simulation arrêtée".to_string())),
    }
}

async fn ws(State(s): State<AppState>, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| stream(socket, s.snapshots))
}

async fn stream(mut socket: WebSocket, snapshots: watch::Receiver<Arc<Snapshot>>) {
    let hello = {
        let snap = snapshots.borrow().clone();
        ServerMsg::Hello {
            server_version: env!("CARGO_PKG_VERSION").into(),
            seed: snap.world.seed,
            run: snap.run.clone(),
            clock: ClockView::of(&snap.world),
            map_rev: snap.world.map_rev,
            events_seq: snap.world.chronicle.next_seq,
        }
    };
    if push(&mut socket, &hello).await.is_err() {
        return;
    }
    let mut view: Option<(i32, i32, i32, i32)> = None;
    let mut every = tokio::time::interval(PUSH_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut beat = 0u64;
    let mut last_tick = u64::MAX;
    loop {
        tokio::select! {
            _ = every.tick() => {
                // La simulation s'est arrêtée : on ferme.
                if snapshots.has_changed().is_err() { break; }
                let snap = snapshots.borrow().clone();
                let w = &snap.world;
                let mut out = Vec::new();
                if w.tick != last_tick {
                    last_tick = w.tick;
                    out.push(ServerMsg::Tick {
                        clock: ClockView::of(w),
                        run: snap.run.clone(),
                        map_rev: w.map_rev,
                        events_seq: w.chronicle.next_seq,
                    });
                    if let Some((x0, y0, x1, y1)) = view {
                        out.push(ServerMsg::Scene(Scene::of(w, x0, y0, x1, y1)));
                    }
                }
                if beat.is_multiple_of(8) {
                    out.push(ServerMsg::Villages { villages: w.villages.iter().map(|v| VillageView::of(w, v)).collect() });
                }
                beat += 1;
                for msg in &out {
                    if push(&mut socket, msg).await.is_err() { return; }
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(ClientMsg::View { x0, y0, x1, y1, detail }) = serde_json::from_str(&text) {
                        // Zone limitée : pas de scène pour une vue de toute la carte.
                        let small = (x1 - x0) <= 400 && (y1 - y0) <= 400;
                        view = (detail && small).then_some((x0, y0, x1, y1));
                        last_tick = u64::MAX; // renvoyer la scène tout de suite
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

#[derive(Deserialize)]
struct EventsQuery {
    since: Option<u64>,
    village: Option<u32>,
    limit: Option<usize>,
}

async fn events(State(s): State<AppState>, Query(q): Query<EventsQuery>) -> Json<Vec<EventView>> {
    let snap = s.snapshots.borrow().clone();
    let w = &snap.world;
    let limit = q.limit.unwrap_or(50).min(500);
    let mut list: Vec<EventView> = w
        .chronicle
        .events
        .iter()
        .rev()
        .filter(|e| q.since.is_none_or(|n| e.seq >= n))
        .filter(|e| q.village.is_none_or(|v| e.village == Some(v)))
        .take(limit)
        .map(|e| EventView::of(w, e))
        .collect();
    list.reverse();
    Json(list)
}

async fn person(State(s): State<AppState>, AxumPath(id): AxumPath<u32>) -> Response {
    let snap = s.snapshots.borrow().clone();
    match PersonSheet::of(&snap.world, id) {
        Some(sheet) => Json(sheet).into_response(),
        None => (StatusCode::NOT_FOUND, "habitant inconnu").into_response(),
    }
}

async fn village(State(s): State<AppState>, AxumPath(id): AxumPath<u32>) -> Response {
    let snap = s.snapshots.borrow().clone();
    match VillageSheet::of(&snap.world, id) {
        Some(sheet) => Json(sheet).into_response(),
        None => (StatusCode::NOT_FOUND, "village inconnu").into_response(),
    }
}

#[derive(Deserialize)]
struct ChangesQuery {
    since: u64,
}

async fn map_changes(State(s): State<AppState>, Query(q): Query<ChangesQuery>) -> Json<MapChanges> {
    let snap = s.snapshots.borrow().clone();
    Json(MapChanges::since(&snap.world, q.since))
}

async fn push(socket: &mut WebSocket, msg: &ServerMsg) -> Result<(), axum::Error> {
    let text = serde_json::to_string(msg).expect("sérialisation d'un message");
    socket.send(Message::Text(text.into())).await
}
