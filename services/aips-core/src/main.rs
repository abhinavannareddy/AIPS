//! aips-core: AIPS intelligence core and AIPS Command API.
//!
//! Phase 0 demonstrator: ground-based, advisory-only. Nothing in this service has write
//! access to any aircraft interface (PRD section 12, architecture constraint 3).
mod api;
mod api_ext;
mod bus;
mod engine;
mod flight;
mod graph;
mod integrity;
mod metrics;
mod predict;
mod state;
mod store;

use aips_platform::{telemetry, vault, Authenticator};
use chrono::{Duration, Utc};
use engine::{Clock, Engine, WorkflowMode};
use parking_lot::RwLock;
use state::AppState;
use std::sync::Arc;
use std::time::Instant;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _otel = telemetry::init("aips-core");
    let secrets = vault::load(&env("AIPS_VAULT_PATH", "aips/core")).await;
    tracing::info!(source = secrets.source, "configuration secrets resolved");

    let auth = Authenticator::from_env().await?;
    let workflow = match env("AIPS_WORKFLOW_ENGINE", "embedded").as_str() {
        "temporal" => WorkflowMode::Temporal,
        _ => WorkflowMode::Embedded,
    };

    // RUL model through ONNX Runtime (C++).
    // Model registry: versioned, hash-verified artefacts; rollback = activate an older version.
    let registry_dir = env("AIPS_MODEL_REGISTRY", "ml/models/registry");
    let wanted = env("AIPS_RUL_VERSION", "latest");
    let entries = predict::registry(std::path::Path::new(&registry_dir));
    let pick = if wanted == "latest" {
        entries.last()
    } else {
        entries.iter().find(|e| e.version == wanted)
    };
    let rul_model = match pick.map(predict::load) {
        Some(Ok(m)) => {
            tracing::info!(model = %m.label(), "RUL model loaded from registry (ONNX Runtime)");
            Some(Arc::new(m))
        }
        Some(Err(e)) => {
            tracing::warn!(error = %e, "RUL model failed integrity/load; remaining-life estimates disabled");
            None
        }
        None => {
            tracing::warn!(dir = %registry_dir, version = %wanted, "no RUL model in registry; remaining-life estimates disabled");
            None
        }
    };

    // Persistence: PostgreSQL + TimescaleDB when configured.
    let store = match secrets.get("DATABASE_URL") {
        Some(url) => Some(store::Store::connect(&url).await?),
        None => {
            tracing::warn!("DATABASE_URL not set: running without persistence (demonstrator mode)");
            None
        }
    };
    let seed: u64 = env("AIPS_REPLAY_SEED", "20261030")
        .parse()
        .unwrap_or(20261030);
    let (dataset, truth, decisions, audit, saved_clock) = match &store {
        Some(st) if st.has_dataset().await? => {
            let (ds, truth) = st.load_dataset().await?;
            tracing::info!(
                flights = ds.flights.len(),
                events = ds.events.len(),
                "dataset loaded from PostgreSQL"
            );
            (
                ds,
                truth,
                st.load_decisions().await?,
                st.load_audit().await?,
                st.load_clock().await?,
            )
        }
        _ => {
            let fleet: usize = env("AIPS_REPLAY_FLEET", "50").parse().unwrap_or(50);
            let g = aips_replay::generate_fleet(seed, Utc::now(), fleet);
            tracing::info!(
                seed,
                flights = g.dataset.flights.len(),
                events = g.dataset.events.len(),
                scenarios = g.truth.len(),
                "synthetic fleet generated (Replay Lab)"
            );
            if let Some(st) = &store {
                st.save_dataset(&g.dataset, &g.truth).await?;
                tracing::info!("dataset persisted to PostgreSQL");
            }
            (
                g.dataset,
                g.truth,
                Default::default(),
                Default::default(),
                None,
            )
        }
    };
    if let Some(broken) = aips_domain::audit::verify_entries(&audit.entries) {
        tracing::error!(
            seq = broken,
            "AUDIT CHAIN BROKEN at load; investigate before trusting history"
        );
    }

    let data_end = dataset.generated_at.unwrap_or_else(Utc::now);
    let warmup_h: i64 = env("AIPS_REPLAY_WARMUP_HOURS", "3").parse().unwrap_or(3);
    // Resume the persisted clock after a restart so recent data never disappears from view.
    let clock = match saved_clock {
        Some(c) => {
            tracing::info!(sim_now = %c.sim_now, "simulation clock resumed from PostgreSQL");
            c
        }
        None => Clock {
            sim_now: data_end - Duration::hours(warmup_h),
            data_end,
            speed: env("AIPS_REPLAY_SPEED", "30").parse().unwrap_or(30.0),
            running: true,
        },
    };
    // NFR-10: an administrator's activation (e.g. a rollback) survives restarts unless the
    // deployment pins a version explicitly with AIPS_RUL_VERSION.
    let mut rul_model = rul_model;
    if std::env::var("AIPS_RUL_VERSION").is_err() {
        if let Some(st) = &store {
            if let Some((_, v)) = st
                .load_docs("model_activation")
                .await?
                .into_iter()
                .find(|(k, _)| k == "rul")
            {
                let want = v["version"].as_str().unwrap_or_default().to_string();
                if rul_model.as_ref().map(|m| m.version.as_str()) != Some(want.as_str()) {
                    match entries
                        .iter()
                        .find(|e| e.version == want)
                        .map(predict::load)
                    {
                        Some(Ok(m)) => {
                            tracing::info!(model = %m.label(), "restored administrator-activated RUL model");
                            rul_model = Some(Arc::new(m));
                        }
                        _ => {
                            tracing::warn!(version = %want, "persisted model activation not loadable; keeping default")
                        }
                    }
                }
            }
        }
    }
    let mut engine = Engine::new(dataset, truth, clock, rul_model, workflow);
    engine.decisions = decisions;
    engine.audit = audit;

    #[cfg(feature = "neo4j")]
    let neo = match secrets.get("NEO4J_URI") {
        Some(uri) => match graph::neo::Neo::connect(
            &uri,
            &secrets.get("NEO4J_USER").unwrap_or_else(|| "neo4j".into()),
            &secrets.get("NEO4J_PASSWORD").unwrap_or_default(),
        )
        .await
        {
            Ok(n) => {
                tracing::info!("Neo4j connected");
                Some(n)
            }
            Err(e) => {
                tracing::error!(error = %e, "Neo4j unavailable; memory graph served in-process");
                None
            }
        },
        None => None,
    };
    #[cfg(feature = "kafka")]
    let bus = match secrets.get("KAFKA_BROKERS") {
        Some(b) => match bus::Bus::connect(&b) {
            Ok(x) => {
                tracing::info!(brokers = %b, "Kafka producer ready");
                Some(x)
            }
            Err(e) => {
                tracing::error!(error = %e, "Kafka unavailable");
                None
            }
        },
        None => None,
    };

    let ui_config = serde_json::json!({
        "authMode": auth.mode_name(),
        "keycloak": { "url": env("AIPS_UI_KEYCLOAK_URL", "http://localhost:8081"), "realm": env("AIPS_UI_KEYCLOAK_REALM", "aips"),
                      "clientId": env("AIPS_UI_KEYCLOAK_CLIENT", "aips-command") },
        "emergencyApi": env("AIPS_UI_EMERGENCY_API", "http://localhost:8091"),
        "environment": env("AIPS_ENV", "development"),
    });
    let (tx, _) = tokio::sync::broadcast::channel(256);
    // Single audit writer: entries are persisted strictly in chain order and retried until
    // PostgreSQL accepts them, so a transient outage or pool timeout never creates a gap.
    let audit_writer = store.clone().map(|st| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<aips_domain::audit::AuditEntry>();
        tokio::spawn(async move {
            while let Some(entry) = rx.recv().await {
                let mut backoff = 1u64;
                while let Err(e) = st.append_audit(&entry).await {
                    if e.to_string().contains("duplicate key") {
                        break; // already persisted by an earlier attempt that timed out client-side
                    }
                    tracing::error!(error = %e, seq = entry.seq, retry_s = backoff, "audit not yet durable; retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(30);
                }
                crate::metrics::AUDIT_PERSISTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
        tx
    });
    let state = Arc::new(AppState {
        engine: RwLock::new(engine),
        auth: auth.clone(),
        store,
        #[cfg(feature = "neo4j")]
        neo,
        #[cfg(feature = "kafka")]
        bus,
        tx,
        ui_config,
        recompute_lock: parking_lot::Mutex::new(()),
        dirty: std::sync::atomic::AtomicBool::new(false),
        audit_tx: audit_writer,
        requested: std::sync::atomic::AtomicU64::new(0),
        completed: tokio::sync::watch::channel(0).0,
        kick: tokio::sync::Notify::new(),
        response_cache: parking_lot::Mutex::new(std::collections::HashMap::new()),
    });
    #[cfg(feature = "kafka")]
    if let Some(b) = &state.bus {
        b.spawn_ingest_consumer(state.clone())?;
    }

    state.recompute_and_publish();
    {
        let snap = state.engine.read().snap.clone();
        tracing::info!(
            alerts = snap.alerts.len(),
            hypotheses = snap.hypotheses.len(),
            cases = snap.cases.len(),
            recall = snap.metrics.recall,
            ms = snap.recompute_ms,
            "initial analysis complete"
        );
    }

    // Simulation clock + periodic projection to PostgreSQL and Neo4j.
    let ticker = state.clone();
    tokio::spawn(async move {
        let mut last = Instant::now();
        let mut last_projection = Instant::now() - std::time::Duration::from_secs(60);
        let mut last_pass = Instant::now();
        let mut iv = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            // Wake on the regular tick, or immediately when a decision requests a pass.
            tokio::select! {
                _ = iv.tick() => {}
                _ = ticker.kick.notified() => {}
            }
            let dt = last.elapsed().as_secs_f64();
            last = Instant::now();
            let before = ticker.engine.read().clock.sim_now;
            ticker.engine.write().clock.tick(dt);
            let moved = ticker.engine.read().clock.sim_now != before;
            let dirty = ticker.dirty.load(std::sync::atomic::Ordering::SeqCst);
            // Analysis runs on the blocking pool so it never stalls the async API workers.
            if dirty || (moved && last_pass.elapsed().as_millis() >= 2000) {
                last_pass = Instant::now();
                let t = ticker.clone();
                if let Err(e) = tokio::task::spawn_blocking(move || t.recompute_and_publish()).await
                {
                    tracing::error!(error = %e, "analysis pass panicked; previous snapshot kept");
                }
            }
            if last_projection.elapsed().as_secs() >= 15 {
                last_projection = Instant::now();
                let snap = ticker.engine.read().snap.clone();
                if let Some(st) = &ticker.store {
                    if let Err(e) = st.project(&snap).await {
                        tracing::warn!(error = %e, "PostgreSQL projection failed");
                    }
                    if let Err(e) = st.save_clock(&snap.clock).await {
                        tracing::warn!(error = %e, "clock persistence failed");
                    }
                }
                #[cfg(feature = "neo4j")]
                if let Some(n) = &ticker.neo {
                    if let Err(e) = n.project(&snap).await {
                        tracing::warn!(error = %e, "Neo4j projection failed");
                    }
                }
            }
        }
    });

    let ui_dir = env("AIPS_UI_DIR", "apps/command/dist");
    let spa =
        ServeDir::new(&ui_dir).not_found_service(ServeFile::new(format!("{ui_dir}/index.html")));
    let cors = tower_http::cors::CorsLayer::permissive();
    let app = api::router(state.clone())
        .with_state(state)
        .fallback_service(spa)
        .layer(
            TraceLayer::new_for_http()
                // Request spans at INFO so they pass the default filter and reach OTLP (bug B18:
                // the DEBUG default meant no traces were ever exported at RUST_LOG=info).
                .make_span_with(aips_platform::telemetry::request_span)
                .on_response(
                    tower_http::trace::DefaultOnResponse::new().level(tracing::Level::DEBUG),
                ),
        )
        .layer(cors);

    let addr = env("AIPS_CORE_ADDR", "0.0.0.0:8090");
    // NFR-05: optional in-process TLS 1.3 (TLS 1.2 refused). In Kubernetes TLS normally terminates at
    // the ingress; set AIPS_TLS_CERT and AIPS_TLS_KEY to encrypt the hop to the pod as well.
    if let (Ok(cert), Ok(key)) = (
        std::env::var("AIPS_TLS_CERT"),
        std::env::var("AIPS_TLS_KEY"),
    ) {
        let cfg = tls_config(&cert, &key)?;
        let handle = axum_server::Handle::new();
        let h = handle.clone();
        tokio::spawn(async move {
            aips_platform::shutdown_signal().await;
            h.graceful_shutdown(Some(std::time::Duration::from_secs(10)));
        });
        tracing::info!(%addr, "aips-core listening (TLS 1.3 only)");
        axum_server::bind_rustls(
            addr.parse()?,
            axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(cfg)),
        )
        .handle(handle)
        .serve(app.into_make_service())
        .await?;
        drain_audit().await;
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "aips-core listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            aips_platform::shutdown_signal().await;
        })
        .await?;
    drain_audit().await;
    Ok(())
}

/// On shutdown, give the audit writer up to 10 s to make queued entries durable (P7); a
/// shortfall is logged loudly rather than lost silently with the runtime.
async fn drain_audit() {
    use std::sync::atomic::Ordering::Relaxed;
    let pending = || {
        crate::metrics::AUDIT_QUEUED
            .load(Relaxed)
            .saturating_sub(crate::metrics::AUDIT_PERSISTED.load(Relaxed))
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while pending() > 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    match pending() {
        0 => tracing::info!("audit queue drained"),
        n => tracing::error!(pending = n, "shutdown with audit entries not yet durable"),
    }
}

/// TLS 1.3-only server configuration from PEM files, using the audited `ring` provider.
fn tls_config(cert: &str, key: &str) -> anyhow::Result<rustls::ServerConfig> {
    use anyhow::Context;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let certs = CertificateDer::pem_file_iter(cert)
        .context("opening TLS certificate")?
        .collect::<Result<Vec<_>, _>>()
        .context("parsing TLS certificate")?;
    let key = PrivateKeyDer::from_pem_file(key).context("reading TLS private key")?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(cfg)
}
