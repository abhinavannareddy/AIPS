//! Apache Kafka integration. Topics:
//!   aips.ingest  : inbound fault events from airline/OEM connectors (INT-01, consumed here)
//!   aips.alerts  : new alerts (outbound)
//!   aips.cases   : case lifecycle events; the Temporal worker starts SLA workflows from these
use serde::Serialize;

pub const TOPIC_INGEST: &str = "aips.ingest";
pub const TOPIC_ALERTS: &str = "aips.alerts";
pub const TOPIC_CASES: &str = "aips.cases";

#[cfg(feature = "kafka")]
pub use imp::Bus;

#[cfg(feature = "kafka")]
mod imp {
    use super::*;
    use crate::state::Shared;
    use aips_domain::model::FaultEvent;
    use rdkafka::config::ClientConfig;
    use rdkafka::consumer::{Consumer, StreamConsumer};
    use rdkafka::message::Message;
    use rdkafka::producer::{FutureProducer, FutureRecord};
    use std::time::Duration;

    pub struct Bus {
        producer: FutureProducer,
        brokers: String,
    }

    impl Bus {
        pub fn connect(brokers: &str) -> anyhow::Result<Self> {
            let producer: FutureProducer = ClientConfig::new()
                .set("bootstrap.servers", brokers)
                .set("client.id", "aips-core")
                .set("acks", "all")
                .set("enable.idempotence", "true")
                .set("message.timeout.ms", "10000")
                .create()?;
            Ok(Self {
                producer,
                brokers: brokers.to_string(),
            })
        }

        pub async fn publish<T: Serialize>(
            &self,
            topic: &str,
            key: &str,
            value: &T,
        ) -> Result<(), String> {
            self.publish_with_headers(topic, key, value, &[]).await
        }

        pub async fn publish_with_headers<T: Serialize>(
            &self,
            topic: &str,
            key: &str,
            value: &T,
            headers: &[(&str, &str)],
        ) -> Result<(), String> {
            let payload = serde_json::to_vec(value).map_err(|e| e.to_string())?;
            let mut h = rdkafka::message::OwnedHeaders::new();
            for (k, v) in headers {
                h = h.insert(rdkafka::message::Header {
                    key: k,
                    value: Some(*v),
                });
            }
            let rec = FutureRecord::to(topic)
                .key(key)
                .payload(&payload)
                .headers(h);
            self.producer
                .send(rec, Duration::from_secs(5))
                .await
                .map(|_| ())
                .map_err(|(e, _)| {
                    tracing::warn!(error = %e, topic, "kafka publish failed");
                    e.to_string()
                })
        }

        /// For messages that start durable workflows (case SLA, pre-departure escalation): in
        /// Temporal mode nothing else escalates, so a lost message is a case that never escalates.
        /// Retries with backoff (about 1 minute in total), then logs loudly.
        pub async fn publish_with_retry<T: Serialize>(&self, topic: &str, key: &str, value: &T) {
            let mut backoff = 1;
            for _ in 0..6 {
                if self.publish(topic, key, value).await.is_ok() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff *= 2;
            }
            tracing::error!(topic, key, "workflow-start message not published after retries; escalation will not start for it");
        }

        /// INT-01 consumer. At-least-once with manual commits: an offset is committed only after
        /// the event is durable (PostgreSQL) and applied, so a crash or database outage never loses
        /// an event; redelivery is harmless because ingestion is idempotent by event id (B7).
        /// Malformed, wrong-schema or unknown-tail messages are rejected, logged and skipped.
        pub fn spawn_ingest_consumer(&self, state: Shared) -> anyhow::Result<()> {
            use rdkafka::consumer::CommitMode;
            use rdkafka::message::Headers;
            let mut cc = ClientConfig::new();
            cc.set("bootstrap.servers", &self.brokers)
                .set("group.id", "aips-core-ingest")
                .set("enable.auto.commit", "false")
                .set("auto.offset.reset", "earliest")
                // Fast failover if a consumer dies without leaving the group.
                .set("session.timeout.ms", "10000")
                .set("heartbeat.interval.ms", "3000");
            if let Ok(d) = std::env::var("AIPS_KAFKA_DEBUG") {
                cc.set("debug", d)
                    .set_log_level(rdkafka::config::RDKafkaLogLevel::Debug);
            }
            let consumer: StreamConsumer = cc.create()?;
            consumer.subscribe(&[TOPIC_INGEST])?;
            tokio::spawn(async move {
                loop {
                    let m = match consumer.recv().await {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::warn!(error = %e, "kafka consumer error");
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    };
                    // Set by the REST ingest endpoint so the audit names the submitting user.
                    let submitted_by = m.headers().and_then(|h| {
                        h.iter()
                            .find(|x| x.key == "x-aips-submitted-by")
                            .and_then(|x| x.value)
                            .and_then(|v| std::str::from_utf8(v).ok())
                            .map(str::to_string)
                    });
                    let schema_ok = m.headers().is_none_or(|h| {
                        h.iter()
                            .find(|x| x.key == "x-aips-schema")
                            .is_none_or(|x| x.value == Some(crate::api::INGEST_SCHEMA.as_bytes()))
                    });
                    let parsed = m
                        .payload_view::<str>()
                        .and_then(|r| r.ok())
                        .ok_or_else(|| "non-UTF-8 or empty payload".to_string())
                        .and_then(|t| {
                            serde_json::from_str::<FaultEvent>(t).map_err(|e| e.to_string())
                        });
                    match (schema_ok, parsed) {
                        (false, _) => {
                            crate::metrics::INGEST_REJECTED
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            tracing::warn!(
                                offset = m.offset(),
                                "rejected ingest message with unsupported schema header"
                            );
                        }
                        (true, Err(e)) => {
                            crate::metrics::INGEST_REJECTED
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            tracing::warn!(offset = m.offset(), error = %e, "rejected malformed ingest message");
                        }
                        (true, Ok(mut ev)) => {
                            ev.normalise();
                            let known = state
                                .engine
                                .read()
                                .full
                                .aircraft
                                .iter()
                                .any(|a| a.tail == ev.tail);
                            if !known || ev.id.trim().is_empty() || ev.code.trim().is_empty() {
                                crate::metrics::INGEST_REJECTED
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                tracing::warn!(offset = m.offset(), tail = %ev.tail, "rejected ingest message: unknown tail or missing fields");
                            } else {
                                // Durable first; retry until the database accepts it (no commit meanwhile).
                                if let Some(st) = &state.store {
                                    let mut backoff = 1;
                                    while let Err(e) = st.insert_event(&ev).await {
                                        tracing::error!(error = %e, retry_s = backoff, "ingest not durable; retrying before commit");
                                        tokio::time::sleep(Duration::from_secs(backoff)).await;
                                        backoff = (backoff * 2).min(30);
                                    }
                                }
                                let actor = submitted_by
                                    .map(|s| format!("api:{s} via kafka"))
                                    .unwrap_or_else(|| "kafka:aips.ingest".into());
                                if !state.apply_ingest(ev, &actor) {
                                    crate::metrics::INGEST_DUPLICATES
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                        }
                    }
                    if let Err(e) = consumer.commit_message(&m, CommitMode::Async) {
                        tracing::warn!(error = %e, "offset commit failed; message may be redelivered (idempotent)");
                    }
                }
            });
            Ok(())
        }
    }
}
