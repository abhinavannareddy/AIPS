//! aips-emergency: AIPS Guardian emergency workflow service.
//!
//! Deployed and stored separately from the analytics platform (EMG-01). State is journalled
//! to local disk on every change (EMG-05) and, when configured, to its own PostgreSQL database.
//! Phase 0: training drills only; no real notification is ever transmitted.
mod engine;

use aips_platform::auth::roles::*;
use aips_platform::{require_auth, telemetry, vault, ApiError, Authenticator, Principal};
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use engine::{Channel, Incident, Scenario, Timers};
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

const MIGRATION: &str = include_str!("../../../db/migrations/emergency/0001_init.sql");

struct App {
    incidents: RwLock<BTreeMap<String, Incident>>,
    data_dir: PathBuf,
    timers: Timers,
    auth: Arc<Authenticator>,
    db: Option<sqlx::PgPool>,
    tx: broadcast::Sender<String>,
    procedures: Vec<engine::ProcedureCard>,
}
type Shared = Arc<App>;
type ApiResult = Result<Json<Value>, ApiError>;

impl App {
    /// Atomic snapshot to local disk (write temp, fsync, rename) plus optional database copy.
    fn persist(self: &Arc<Self>, inc: &Incident) {
        let path = self.data_dir.join(format!("{}.json", inc.id));
        let tmp = self.data_dir.join(format!(".{}.tmp", inc.id));
        match serde_json::to_vec_pretty(inc) {
            Ok(bytes) => {
                let res = (|| -> std::io::Result<()> {
                    use std::io::Write;
                    let mut f = std::fs::File::create(&tmp)?;
                    f.write_all(&bytes)?;
                    f.sync_all()?;
                    std::fs::rename(&tmp, &path)
                })();
                if let Err(e) = res {
                    tracing::error!(error = %e, id = %inc.id, "local journal write failed");
                }
            }
            Err(e) => tracing::error!(error = %e, "serialise incident"),
        }
        if let Some(db) = self.db.clone() {
            let inc = inc.clone();
            tokio::spawn(async move {
                let res = sqlx::query("INSERT INTO incidents (id, doc, updated) VALUES ($1, $2, now()) ON CONFLICT (id) DO UPDATE SET doc = EXCLUDED.doc, updated = now() WHERE jsonb_array_length(incidents.doc->'journal'->'entries') <= jsonb_array_length(EXCLUDED.doc->'journal'->'entries')")
                    .bind(&inc.id)
                    .bind(serde_json::to_value(&inc).unwrap_or_default())
                    .execute(&db)
                    .await;
                if let Err(e) = res {
                    tracing::warn!(error = %e, "emergency DB copy failed; local journal remains authoritative");
                }
            });
        }
        let _ = self.tx.send(json!({ "type": "incident", "id": inc.id, "state": inc.state, "escalation_level": inc.escalation_level }).to_string());
    }

    fn mutate<R>(
        self: &Arc<Self>,
        id: &str,
        f: impl FnOnce(&mut Incident) -> Result<R, String>,
    ) -> Result<(R, Incident), ApiError> {
        let mut map = self.incidents.write();
        let inc = map
            .get_mut(id)
            .ok_or_else(|| ApiError::not_found("unknown incident"))?;
        let r = f(inc).map_err(ApiError::bad_request)?;
        let snapshot = inc.clone();
        // Persist under the lock: a later snapshot must never be overwritten by an earlier one.
        self.persist(&snapshot);
        drop(map);
        Ok((r, snapshot))
    }
}

fn load_journal(dir: &PathBuf) -> BTreeMap<String, Incident> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                match std::fs::read(&p)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Incident>(&b).ok())
                {
                    Some(inc) => {
                        if let Some(seq) = inc.journal.verify() {
                            tracing::error!(id = %inc.id, seq, "incident journal integrity check FAILED");
                        }
                        out.insert(inc.id.clone(), inc);
                    }
                    None => tracing::error!(path = %p.display(), "unreadable incident journal"),
                }
            }
        }
    }
    out
}

/// The database copy is an independent anchor for the local journal: it only ever moves forward
/// (see `persist`). A local journal that is missing, unreadable or shorter than a verified database
/// copy was lost or truncated, so the database copy is restored and the event logged (EMG-03/05).
async fn reconcile_with_db(pool: &sqlx::PgPool, local: &mut BTreeMap<String, Incident>) {
    let rows: Vec<(String, Value)> = match sqlx::query_as("SELECT id, doc FROM incidents")
        .fetch_all(pool)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "emergency DB copy unreadable; journal not reconciled");
            return;
        }
    };
    for (id, doc) in rows {
        let Ok(db_inc) = serde_json::from_value::<Incident>(doc) else {
            tracing::error!(%id, "unreadable incident in emergency DB copy");
            continue;
        };
        if db_inc.journal.verify().is_some() {
            tracing::error!(%id, "emergency DB copy fails its integrity check; local journal kept");
            continue;
        }
        let local_len = local.get(&id).map(|i| i.journal.entries.len());
        if local_len.is_none_or(|n| n < db_inc.journal.entries.len()) {
            tracing::error!(%id, local_entries = ?local_len, db_entries = db_inc.journal.entries.len(),
                "local incident journal missing or truncated; restored from the database copy");
            local.insert(id, db_inc);
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _otel = telemetry::init("aips-emergency");
    let secrets =
        vault::load(&std::env::var("AIPS_VAULT_PATH").unwrap_or_else(|_| "aips/emergency".into()))
            .await;
    anyhow::ensure!(
        std::env::var("AIPS_EMERGENCY_LIVE_ENABLED")
            .map(|v| v != "true")
            .unwrap_or(true),
        "live emergency mode is not available in Phase 0; refusing to start"
    );
    let auth = Authenticator::from_env().await?;
    let data_dir = PathBuf::from(
        std::env::var("AIPS_EMERGENCY_DATA").unwrap_or_else(|_| ".data/emergency".into()),
    );
    std::fs::create_dir_all(&data_dir)?;
    let mut incidents = load_journal(&data_dir);
    tracing::info!(recovered = incidents.len(), dir = %data_dir.display(), "incident journal recovered");

    let db = match secrets.get("EMERGENCY_DATABASE_URL") {
        Some(url) => match sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
        {
            Ok(pool) => {
                sqlx::raw_sql(MIGRATION).execute(&pool).await?;
                tracing::info!("emergency PostgreSQL connected (separate from analytics)");
                Some(pool)
            }
            Err(e) => {
                tracing::error!(error = %e, "emergency DB unavailable; continuing on local journal (EMG-01)");
                None
            }
        },
        None => None,
    };
    if let Some(pool) = &db {
        reconcile_with_db(pool, &mut incidents).await;
    }
    let (tx, _) = broadcast::channel(256);
    let ack_s: i64 = std::env::var("AIPS_EMERGENCY_ACK_TIMEOUT_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    // GRD-08: procedure cards come from configuration (operator-approved content in service);
    // without a file the built-in TRAINING EXAMPLE cards are used.
    let procedures = match std::env::var("AIPS_GUARDIAN_PROCEDURES") {
        Ok(path) => {
            let txt = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
            let cards: Vec<engine::ProcedureCard> =
                serde_json::from_str(&txt).map_err(|e| anyhow::anyhow!("parsing {path}: {e}"))?;
            tracing::info!(path = %path, cards = cards.len(), "procedure cards loaded from configuration");
            cards
        }
        Err(_) => engine::default_procedures(),
    };
    let app: Shared = Arc::new(App {
        incidents: RwLock::new(incidents),
        data_dir,
        timers: Timers {
            ack_timeout: chrono::Duration::seconds(ack_s),
            second_escalation: chrono::Duration::seconds(ack_s + 5),
        },
        auth: auth.clone(),
        db,
        tx,
        procedures,
    });

    // Escalation timers resume after a restart because deadlines are stored in the journal.
    let ticker = app.clone();
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            iv.tick().await;
            let mut rng = rand::thread_rng();
            let mut map = ticker.incidents.write();
            for inc in map.values_mut() {
                // Persist under the lock so a concurrent ack/cancel cannot be overwritten.
                if inc.tick(&ticker.timers, &mut rng) {
                    ticker.persist(inc);
                }
            }
        }
    });

    let protected = Router::new()
        .route("/api/emergency/drills", get(list).post(create))
        .route("/api/emergency/drills/{id}", get(one))
        .route("/api/emergency/drills/{id}/ack", post(ack))
        .route("/api/emergency/drills/{id}/simulate", post(simulate))
        .route("/api/emergency/drills/{id}/cancel", post(cancel))
        .route("/api/emergency/drills/{id}/resolve", post(resolve))
        .route("/api/emergency/drills/{id}/verify", get(verify))
        .route("/api/emergency/capabilities/{type_code}", get(capability))
        .route("/api/emergency/procedures", get(procedure_cards))
        .route("/api/emergency/stream", get(stream))
        .layer(axum::middleware::from_fn_with_state(auth, require_auth));
    let router = Router::new()
        .route("/api/emergency/health", get(health))
        .route("/livez", get(|| async { "ok" }))
        .route("/readyz", get(health))
        .route("/metrics", get(prom))
        .merge(protected)
        .with_state(app)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                // Request spans at INFO so they pass the default filter and reach OTLP (bug B18:
                // the DEBUG default meant no traces were ever exported at RUST_LOG=info).
                .make_span_with(aips_platform::telemetry::request_span)
                .on_response(
                    tower_http::trace::DefaultOnResponse::new().level(tracing::Level::DEBUG),
                ),
        );

    let addr = std::env::var("AIPS_EMERGENCY_ADDR").unwrap_or_else(|_| "0.0.0.0:8091".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "aips-emergency listening (TRAINING MODE ONLY)");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            aips_platform::shutdown_signal().await;
        })
        .await?;
    Ok(())
}

async fn health(State(s): State<Shared>) -> Json<Value> {
    let m = s.incidents.read();
    Json(
        json!({ "status": "ok", "service": "aips-emergency", "mode": "training_only", "auth_mode": s.auth.mode_name(),
                 "incidents": m.len(), "active": m.values().filter(|i| i.is_active()).count(),
                 "database": s.db.is_some(), "journal": s.data_dir.display().to_string() }),
    )
}

async fn prom(State(s): State<Shared>) -> String {
    let m = s.incidents.read();
    let active = m.values().filter(|i| i.is_active()).count();
    let escalated = m.values().filter(|i| i.escalation_level > 0).count();
    format!(
        "# HELP aips_emergency_incidents Incidents in the journal\n# TYPE aips_emergency_incidents gauge\naips_emergency_incidents {}\n\
         # HELP aips_emergency_active Active incidents\n# TYPE aips_emergency_active gauge\naips_emergency_active {active}\n\
         # HELP aips_emergency_escalated Incidents that escalated\n# TYPE aips_emergency_escalated gauge\naips_emergency_escalated {escalated}\n",
        m.len()
    )
}

const TRAINERS: [&str; 4] = [SAFETY, OPS, ENGINEER, MCC];

async fn list(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[SAFETY, OPS, ENGINEER, MCC, CABIN_CREW])?;
    let m = s.incidents.read();
    let mut v: Vec<&Incident> = m.values().collect();
    v.sort_by_key(|i| std::cmp::Reverse(i.created_at));
    Ok(Json(json!(v)))
}

async fn one(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.require(&[SAFETY, OPS, ENGINEER, MCC, CABIN_CREW])?;
    let m = s.incidents.read();
    Ok(Json(json!(m
        .get(&id)
        .ok_or_else(|| ApiError::not_found("unknown incident"))?)))
}

async fn procedure_cards(State(s): State<Shared>, Extension(p): Extension<Principal>) -> ApiResult {
    p.require(&[SAFETY, OPS, ENGINEER, MCC, CABIN_CREW])?;
    Ok(Json(json!(s.procedures)))
}

#[derive(Deserialize)]
struct CreateBody {
    tail: String,
    type_code: String,
    #[serde(default)]
    flight: String,
    /// Destination airport (AUT-03 message context). Optional for backwards compatibility.
    #[serde(default)]
    dest: String,
    scenario: Scenario,
    #[serde(default = "yes")]
    training: bool,
}
fn yes() -> bool {
    true
}

async fn create(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Json(b): Json<CreateBody>,
) -> ApiResult {
    p.require(&TRAINERS)?;
    if !b.training {
        return Err(ApiError::forbidden(
            "live emergency mode is not available in Phase 0; drills must run in training mode",
        ));
    }
    if b.tail.is_empty()
        || b.tail.len() > 16
        || !b
            .tail
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(ApiError::bad_request("invalid tail"));
    }
    let id = format!(
        "DRILL-{}",
        uuid::Uuid::new_v4().simple().to_string()[..10].to_uppercase()
    );
    let mut inc = Incident::new(
        id.clone(),
        b.tail,
        b.type_code,
        b.flight,
        b.scenario,
        &p.audit_actor(),
    );
    if b.dest.len() <= 4 && b.dest.chars().all(|c| c.is_ascii_alphanumeric()) {
        inc.destination = b.dest.to_ascii_uppercase();
    }
    inc.procedure = s
        .procedures
        .iter()
        .find(|c| c.scenario == b.scenario)
        .cloned();
    inc.activate(&s.timers, &mut rand::thread_rng());
    s.incidents.write().insert(id.clone(), inc.clone());
    s.persist(&inc);
    Ok(Json(json!(inc)))
}

#[derive(Deserialize)]
struct AckBody {
    crew_id: String,
    #[serde(default = "yes")]
    available: bool,
}

async fn ack(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<AckBody>,
) -> ApiResult {
    p.require(&[CABIN_CREW, SAFETY, OPS])?;
    let by = p.audit_actor();
    // Only cabin crew answer for themselves; ops or safety relaying a response is a proxy.
    let proxy = !p.roles.iter().any(|r| r == CABIN_CREW);
    let (_, inc) = s.mutate(&id, |i| i.acknowledge(&b.crew_id, b.available, &by, proxy))?;
    Ok(Json(json!(inc)))
}

#[derive(Deserialize)]
struct SimBody {
    action: String,
    channel: Option<Channel>,
}

async fn simulate(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<SimBody>,
) -> ApiResult {
    p.require(&TRAINERS)?;
    let by = p.audit_actor();
    let (_, inc) = s.mutate(&id, |i| {
        match b.action.as_str() {
            "feed_loss" => i.set_feed(true, &by),
            "feed_restore" => i.set_feed(false, &by),
            "channel_failure" => {
                let ch = b.channel.unwrap_or(Channel::CrewDevicePush);
                i.failed_channel = Some(ch);
                i.log(
                    &by,
                    "drill.inject_channel_failure",
                    json!({ "channel": ch }),
                );
            }
            _ => return Err("action must be feed_loss | feed_restore | channel_failure".into()),
        }
        Ok(())
    })?;
    Ok(Json(json!(inc)))
}

#[derive(Deserialize)]
struct CloseBody {
    reason: String,
}

async fn cancel(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<CloseBody>,
) -> ApiResult {
    // EMG-06 / GRD-09: an authorised person can always cancel; every cancel is recorded.
    p.require(&[CABIN_CREW, SAFETY, OPS])?;
    let by = p.audit_actor();
    let (_, inc) = s.mutate(&id, |i| i.close(&by, &b.reason, true))?;
    Ok(Json(json!(inc)))
}

async fn resolve(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(b): Json<CloseBody>,
) -> ApiResult {
    p.require(&[SAFETY, OPS])?;
    let by = p.audit_actor();
    let (_, inc) = s.mutate(&id, |i| i.close(&by, &b.reason, false))?;
    Ok(Json(json!(inc)))
}

async fn verify(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    p.require(&[SAFETY, OPS, ENGINEER])?;
    let m = s.incidents.read();
    let inc = m
        .get(&id)
        .ok_or_else(|| ApiError::not_found("unknown incident"))?;
    let broken = inc.journal.verify();
    Ok(Json(
        json!({ "entries": inc.journal.entries.len(), "intact": broken.is_none(), "first_broken_seq": broken,
                    "head": inc.journal.entries.last().map(|e| &e.hash) }),
    ))
}

async fn capability(Extension(p): Extension<Principal>, Path(t): Path<String>) -> ApiResult {
    p.require(&TRAINERS)?;
    Ok(Json(
        json!({ "type_code": t, "certified_emergency_landing": engine::autoland_capability(&t),
                    "aips_role": "information forwarding only; AIPS never commands the aircraft" }),
    ))
}

async fn stream(
    State(s): State<Shared>,
    Extension(p): Extension<Principal>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    // Same roles as reading drills: emergency events are not visible to e.g. lessors.
    p.require(&[SAFETY, OPS, ENGINEER, MCC, CABIN_CREW])?;
    let st = BroadcastStream::new(s.tx.subscribe())
        .filter_map(|m| m.ok().map(|t| Ok(Event::default().data(t))));
    Ok(Sse::new(st).keep_alive(KeepAlive::default()))
}
