//! Shared application state and side-effect plumbing (audit persistence, projections, events).
use crate::engine::{Engine, Outcome};
use crate::store::Store;
use aips_domain::model::FaultEvent;
use aips_platform::Authenticator;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct AppState {
    pub engine: RwLock<Engine>,
    pub auth: Arc<Authenticator>,
    pub store: Option<Store>,
    #[cfg(feature = "neo4j")]
    pub neo: Option<crate::graph::neo::Neo>,
    #[cfg(feature = "kafka")]
    pub bus: Option<crate::bus::Bus>,
    pub tx: broadcast::Sender<String>,
    pub ui_config: serde_json::Value,
    /// Serialises analysis passes so commits never interleave.
    pub recompute_lock: parking_lot::Mutex<()>,
    /// Set by ingestion; the scheduler coalesces many events into one analysis pass.
    pub dirty: std::sync::atomic::AtomicBool,
    /// Ordered, lossless audit persistence (single writer; retries until durable).
    pub audit_tx: Option<tokio::sync::mpsc::UnboundedSender<aips_domain::audit::AuditEntry>>,
    /// Read-your-writes for decisions: requests bump `requested`; each analysis pass publishes
    /// the highest request generation it included on `completed`.
    pub requested: std::sync::atomic::AtomicU64,
    pub completed: tokio::sync::watch::Sender<u64>,
    pub kick: tokio::sync::Notify,
    /// Serialized responses of heavy read endpoints, valid for one snapshot version.
    pub response_cache:
        parking_lot::Mutex<std::collections::HashMap<String, (u64, axum::body::Bytes)>>,
}

pub type Shared = Arc<AppState>;

impl AppState {
    /// Append to the audit chain and persist it (NFR-08). Never blocks the caller on I/O.
    pub fn audit(
        self: &Arc<Self>,
        actor: &str,
        action: &str,
        subject: &str,
        detail: serde_json::Value,
    ) {
        // Append and enqueue under the same lock so the persistence order equals the chain order
        // (regression: bug B9, fire-and-forget inserts could fail under load and leave gaps).
        let mut e = self.engine.write();
        let entry = e.audit.append(actor, action, subject, detail);
        if let Some(tx) = &self.audit_tx {
            if tx.send(entry).is_err() {
                tracing::error!("audit writer stopped; entry kept in memory only");
            } else {
                crate::metrics::AUDIT_QUEUED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    /// Return the cached JSON for `key` at snapshot `version`, or build, cache and return it.
    /// Readers of an unchanged snapshot then share one serialization (bug B21: multi-megabyte
    /// responses were rebuilt per request, collapsing throughput under large histories).
    pub fn cached_json(
        &self,
        key: String,
        version: u64,
        build: impl FnOnce() -> serde_json::Value,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        if let Some((v, b)) = self.response_cache.lock().get(&key) {
            if *v == version {
                return (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    b.clone(),
                )
                    .into_response();
            }
        }
        let bytes = axum::body::Bytes::from(serde_json::to_vec(&build()).unwrap_or_default());
        let mut c = self.response_cache.lock();
        c.retain(|_, (v, _)| *v == version);
        if c.len() < 512 {
            c.insert(key, (version, bytes.clone()));
        }
        (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            bytes,
        )
            .into_response()
    }

    /// Ask for an analysis pass. Human decisions no longer recompute on the request thread:
    /// the scheduler coalesces them (regression: bug B10, synchronous recomputes under bursts of
    /// decisions starved the async runtime and exhausted the database pool).
    pub fn request_recompute(&self) -> u64 {
        let g = self
            .requested
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
        self.kick.notify_one();
        g
    }

    /// Request a pass and wait (bounded) until a pass that includes this decision has committed,
    /// so the caller's next read reflects it. Many concurrent callers share one coalesced pass.
    pub async fn recompute_wait(&self) {
        let g = self.request_recompute();
        let mut rx = self.completed.subscribe();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), rx.wait_for(|c| *c >= g))
            .await;
    }

    pub fn recompute_and_publish(self: &Arc<Self>) {
        let span = tracing::info_span!(
            "analysis_pass",
            recompute_ms = tracing::field::Empty,
            alerts = tracing::field::Empty
        );
        let _enter = span.enter();
        let _serial = self.recompute_lock.lock();
        self.dirty.store(false, std::sync::atomic::Ordering::SeqCst);
        // Every request generation observed now is applied to state already, so this pass includes it.
        let included = self.requested.load(std::sync::atomic::Ordering::SeqCst);
        let view = self.engine.read().pass_view();
        let computed = view.compute();
        let outcome = self.engine.write().commit(computed);
        self.completed.send_replace(included);
        {
            let snap = &self.engine.read().snap;
            span.record("recompute_ms", snap.recompute_ms);
            span.record("alerts", snap.alerts.len());
        }
        drop(_serial);
        self.after_recompute(outcome);
    }

    pub fn after_recompute(self: &Arc<Self>, mut outcome: Outcome) {
        // Escalations are recomputed from scratch after a restart. One already recorded in the
        // reloaded audit chain must not be audited or published again with a new timestamp (P7).
        if !(outcome.escalated_alerts.is_empty()
            && outcome.escalated_preflight.is_empty()
            && outcome.escalated_cases.is_empty())
        {
            let done: std::collections::HashSet<(String, String)> = self
                .engine
                .read()
                .audit
                .entries
                .iter()
                .filter(|e| e.actor == "aips-workflow")
                .map(|e| (e.action.clone(), e.subject.clone()))
                .collect();
            let fresh =
                |action: &str, id: &str| !done.contains(&(action.to_string(), id.to_string()));
            outcome
                .escalated_alerts
                .retain(|a| fresh("alert.escalated", &a.id));
            outcome
                .escalated_preflight
                .retain(|f| fresh("preflight.escalated", f));
            outcome
                .escalated_cases
                .retain(|c| fresh("case.escalated", &c.id));
        }
        let snap = self.engine.read().snap.clone();
        let msg = serde_json::json!({
            "type": "snapshot",
            "version": snap.version,
            "sim_now": snap.clock.sim_now,
            "live": snap.clock.is_live(),
            "alerts": snap.alerts.len(),
            "new_alerts": outcome.new_alerts.iter().map(|a| serde_json::json!({"id": a.id, "tail": a.tail, "title": a.title, "severity": a.severity})).collect::<Vec<_>>(),
            "opened_cases": outcome.opened_cases.len(),
        });
        let _ = self.tx.send(msg.to_string());
        for a in &outcome.escalated_alerts {
            self.audit(
                "aips-workflow",
                "alert.escalated",
                &a.id,
                serde_json::json!({ "reason": "warning not acknowledged within window", "tail": a.tail }),
            );
        }
        for f in &outcome.escalated_preflight {
            self.audit(
                "aips-workflow",
                "preflight.escalated",
                f,
                serde_json::json!({ "reason": "significant concern not reviewed before departure" }),
            );
        }
        for c in &outcome.escalated_cases {
            self.audit(
                "aips-workflow",
                "case.escalated",
                &c.id,
                serde_json::json!({ "reason": "sla_breach" }),
            );
        }
        #[cfg(feature = "kafka")]
        if let Some(_bus) = &self.bus {
            let me = self.clone();
            tokio::spawn(async move {
                let Some(bus) = &me.bus else { return };
                for a in &outcome.new_alerts {
                    let _ = bus.publish(crate::bus::TOPIC_ALERTS, &a.tail, a).await;
                }
                for p in &outcome.new_preflight_concerns {
                    bus.publish_with_retry(crate::bus::TOPIC_CASES, &p.flight_id, &serde_json::json!({
                        "type": "preflight.concern", "flight_id": p.flight_id, "tail": p.tail, "std": p.std,
                        "escalate_before_min": crate::engine::PREFLIGHT_ESCALATE_BEFORE_MIN,
                    })).await;
                }
                for a in &outcome.escalated_alerts {
                    let _ = bus
                        .publish(
                            crate::bus::TOPIC_ALERTS,
                            &a.tail,
                            &serde_json::json!({ "type": "alert.escalated", "alert": a }),
                        )
                        .await;
                }
                for c in &outcome.opened_cases {
                    bus.publish_with_retry(crate::bus::TOPIC_CASES, &c.id, &serde_json::json!({
                        "type": "case.opened", "case_id": c.id, "tail": c.tail, "priority": c.priority,
                        "sla_due": c.sla_due, "opened_at": c.opened_at, "team": c.team,
                    })).await;
                }
            });
        }
    }

    /// INT-01: apply an inbound event (from Kafka or the REST ingestion endpoint).
    /// Returns false when the event id was already known (duplicate delivery, ignored).
    pub fn apply_ingest(self: &Arc<Self>, ev: FaultEvent, via: &str) -> bool {
        let id = ev.id.clone();
        let tail = ev.tail.clone();
        if !self.engine.write().ingest(ev) {
            return false;
        }
        self.audit(
            via,
            "event.ingested",
            &id,
            serde_json::json!({ "tail": tail }),
        );
        // Coalesced: the scheduler runs one analysis pass for a burst of events (<= 500 ms)
        // instead of one full pass per event (performance fix B2).
        self.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
        true
    }
}
