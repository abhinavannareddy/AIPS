//! PostgreSQL / TimescaleDB persistence: operational dataset, human decisions,
//! the append-only audit chain, and read-model projections.
use crate::engine::{CaseHuman, Decisions, GradeDecision, Snapshot};
use aips_domain::audit::{AuditChain, AuditEntry};
use aips_domain::intel::AlertFeedback;
use aips_domain::model::*;
use aips_domain::truth::ScenarioTruth;
use anyhow::Context;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{de::DeserializeOwned, Serialize};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;

/// Ordered, idempotent, additive migrations.
const MIGRATIONS: [(&str, &str); 4] = [
    (
        "0001_init",
        include_str!("../../../db/migrations/core/0001_init.sql"),
    ),
    (
        "0002_clock",
        include_str!("../../../db/migrations/core/0002_clock.sql"),
    ),
    (
        "0003_flight",
        include_str!("../../../db/migrations/core/0003_flight.sql"),
    ),
    (
        "0004_decisions",
        include_str!("../../../db/migrations/core/0004_decisions.sql"),
    ),
];

fn s<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|x| x.as_str().map(str::to_string))
        .unwrap_or_default()
}
fn e<T: DeserializeOwned>(v: &str) -> anyhow::Result<T> {
    serde_json::from_value(serde_json::Value::String(v.to_string()))
        .with_context(|| format!("bad enum value {v}"))
}

#[derive(Clone)]
pub struct Store {
    pub pool: PgPool,
}

impl Store {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(url)
            .await
            .context("connecting to PostgreSQL")?;
        for (name, sql) in MIGRATIONS {
            sqlx::raw_sql(sql)
                .execute(&pool)
                .await
                .with_context(|| format!("running migration {name}"))?;
        }
        let ts: Option<String> =
            sqlx::query_scalar("SELECT extversion FROM pg_extension WHERE extname = 'timescaledb'")
                .fetch_optional(&pool)
                .await?;
        match ts {
            Some(v) => {
                tracing::info!(timescaledb = %v, "PostgreSQL connected; param_samples is a hypertable")
            }
            None => tracing::warn!(
                "PostgreSQL connected WITHOUT TimescaleDB; param_samples is a plain table"
            ),
        }
        Ok(Self { pool })
    }

    pub async fn has_dataset(&self) -> anyhow::Result<bool> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM replay_meta")
                .fetch_one(&self.pool)
                .await?
                > 0,
        )
    }

    pub async fn clear(&self) -> anyhow::Result<()> {
        sqlx::raw_sql(
            "TRUNCATE decision_docs, flight_summaries, param_samples, mel_items, work_orders, fault_events, flights, components, aircraft, replay_meta,
             hypothesis_grades, alert_feedback, case_state, preflight_acks,
             projection_hypotheses, projection_alerts, projection_cases, projection_preflight",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn save_dataset(&self, ds: &Dataset, truth: &[ScenarioTruth]) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO replay_meta (seed, generated_at, truth, departures) VALUES ($1,$2,$3,$4)",
        )
        .bind(ds.seed as i64)
        .bind(ds.generated_at.unwrap_or_else(Utc::now))
        .bind(serde_json::to_value(truth)?)
        .bind(serde_json::to_value(&ds.departures)?)
        .execute(&mut *tx)
        .await?;
        for a in &ds.aircraft {
            sqlx::query("INSERT INTO aircraft VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                .bind(&a.tail)
                .bind(&a.type_code)
                .bind(a.msn as i32)
                .bind(&a.operator)
                .bind(&a.lessor)
                .bind(a.engines as i16)
                .bind(a.delivered)
                .bind(&a.base)
                .bind(a.data_feed_last)
                .execute(&mut *tx)
                .await?;
        }
        for c in &ds.components {
            sqlx::query("INSERT INTO components VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                .bind(&c.id)
                .bind(&c.tail)
                .bind(&c.part_number)
                .bind(&c.serial)
                .bind(c.ata as i16)
                .bind(&c.position)
                .bind(&c.description)
                .bind(c.installed_at)
                .bind(c.cycles_since_sv as i32)
                .execute(&mut *tx)
                .await?;
        }
        for chunk in ds.flights.chunks(5000) {
            sqlx::query(
                "INSERT INTO flights SELECT * FROM UNNEST($1::text[],$2::text[],$3::text[],$4::text[],$5::text[],$6::timestamptz[],$7::timestamptz[],$8::int[])",
            )
            .bind(chunk.iter().map(|f| f.id.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|f| f.tail.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|f| f.number.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|f| f.origin.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|f| f.dest.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|f| f.dep).collect::<Vec<_>>())
            .bind(chunk.iter().map(|f| f.arr).collect::<Vec<_>>())
            .bind(chunk.iter().map(|f| f.cycle as i32).collect::<Vec<_>>())
            .execute(&mut *tx)
            .await?;
        }
        for chunk in ds.events.chunks(5000) {
            sqlx::query(
                "INSERT INTO fault_events SELECT * FROM UNNEST($1::text[],$2::text[],$3::text[],$4::timestamptz[],$5::smallint[],$6::text[],$7::text[],$8::text[],$9::text[],$10::text[],$11::text[],$12::timestamptz[])",
            )
            .bind(chunk.iter().map(|x| x.id.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| x.tail.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| x.flight_id.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| x.ts).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| x.ata as i16).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| s(&x.system)).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| x.code.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| x.component_id.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| s(&x.severity)).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| x.message.clone()).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| s(&x.source)).collect::<Vec<_>>())
            .bind(chunk.iter().map(|x| x.ingested_at).collect::<Vec<_>>())
            .execute(&mut *tx)
            .await?;
        }
        for w in &ds.work_orders {
            sqlx::query("INSERT INTO work_orders VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
                .bind(&w.id)
                .bind(&w.tail)
                .bind(&w.event_ids)
                .bind(&w.component_id)
                .bind(w.ata as i16)
                .bind(s(&w.action))
                .bind(w.opened)
                .bind(w.closed)
                .bind(w.signed_off)
                .bind(&w.team)
                .bind(&w.note)
                .execute(&mut *tx)
                .await?;
        }
        for m in &ds.mel {
            sqlx::query("INSERT INTO mel_items VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
                .bind(&m.id)
                .bind(&m.tail)
                .bind(&m.reference)
                .bind(&m.title)
                .bind(m.category.to_string())
                .bind(m.ata as i16)
                .bind(m.opened)
                .bind(m.due)
                .execute(&mut *tx)
                .await?;
        }
        for chunk in ds.samples.chunks(20000) {
            sqlx::query("INSERT INTO param_samples SELECT * FROM UNNEST($1::timestamptz[],$2::text[],$3::text[],$4::text[],$5::float8[])")
                .bind(chunk.iter().map(|x| x.ts).collect::<Vec<_>>())
                .bind(chunk.iter().map(|x| x.tail.clone()).collect::<Vec<_>>())
                .bind(chunk.iter().map(|x| s(&x.param)).collect::<Vec<_>>())
                .bind(chunk.iter().map(|x| x.flight_id.clone()).collect::<Vec<_>>())
                .bind(chunk.iter().map(|x| x.value).collect::<Vec<_>>())
                .execute(&mut *tx)
                .await?;
        }
        for chunk in ds.flight_summaries.chunks(5000) {
            let docs: Vec<serde_json::Value> = chunk
                .iter()
                .map(|x| serde_json::to_value(x).unwrap_or_default())
                .collect();
            sqlx::query("INSERT INTO flight_summaries SELECT * FROM UNNEST($1::text[],$2::text[],$3::jsonb[],$4::timestamptz[])")
                .bind(chunk.iter().map(|x| x.flight_id.clone()).collect::<Vec<_>>())
                .bind(chunk.iter().map(|x| x.tail.clone()).collect::<Vec<_>>())
                .bind(docs)
                .bind(chunk.iter().map(|x| x.received_at).collect::<Vec<_>>())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_dataset(&self) -> anyhow::Result<(Dataset, Vec<ScenarioTruth>)> {
        let p = &self.pool;
        let meta = sqlx::query("SELECT seed, generated_at, truth, departures FROM replay_meta")
            .fetch_one(p)
            .await?;
        let mut ds = Dataset {
            seed: meta.try_get::<i64, _>("seed")? as u64,
            generated_at: Some(meta.try_get("generated_at")?),
            departures: serde_json::from_value(meta.try_get("departures")?)?,
            ..Default::default()
        };
        let truth: Vec<ScenarioTruth> = serde_json::from_value(meta.try_get("truth")?)?;
        for r in sqlx::query("SELECT * FROM aircraft ORDER BY tail")
            .fetch_all(p)
            .await?
        {
            ds.aircraft.push(Aircraft {
                tail: r.try_get("tail")?,
                type_code: r.try_get("type_code")?,
                msn: r.try_get::<i32, _>("msn")? as u32,
                operator: r.try_get("operator")?,
                lessor: r.try_get("lessor")?,
                engines: r.try_get::<i16, _>("engines")? as u8,
                delivered: r.try_get::<NaiveDate, _>("delivered")?,
                base: r.try_get("base")?,
                data_feed_last: r.try_get("data_feed_last")?,
            });
        }
        for r in sqlx::query("SELECT * FROM components").fetch_all(p).await? {
            ds.components.push(Component {
                id: r.try_get("id")?,
                tail: r.try_get("tail")?,
                part_number: r.try_get("part_number")?,
                serial: r.try_get("serial")?,
                ata: r.try_get::<i16, _>("ata")? as u8,
                position: r.try_get("position")?,
                description: r.try_get("description")?,
                installed_at: r.try_get("installed_at")?,
                cycles_since_sv: r.try_get::<i32, _>("cycles_since_sv")? as u32,
            });
        }
        for r in sqlx::query("SELECT * FROM flights ORDER BY dep")
            .fetch_all(p)
            .await?
        {
            ds.flights.push(Flight {
                id: r.try_get("id")?,
                tail: r.try_get("tail")?,
                number: r.try_get("number")?,
                origin: r.try_get("origin")?,
                dest: r.try_get("dest")?,
                dep: r.try_get("dep")?,
                arr: r.try_get("arr")?,
                cycle: r.try_get::<i32, _>("cycle")? as u32,
            });
        }
        for r in sqlx::query("SELECT * FROM fault_events ORDER BY ts")
            .fetch_all(p)
            .await?
        {
            ds.events.push(FaultEvent {
                id: r.try_get("id")?,
                tail: r.try_get("tail")?,
                flight_id: r.try_get("flight_id")?,
                ts: r.try_get("ts")?,
                ata: r.try_get::<i16, _>("ata")? as u8,
                system: e(&r.try_get::<String, _>("system")?)?,
                code: r.try_get("code")?,
                component_id: r.try_get("component_id")?,
                severity: e(&r.try_get::<String, _>("severity")?)?,
                message: r.try_get("message")?,
                source: e(&r.try_get::<String, _>("source")?)?,
                ingested_at: r.try_get("ingested_at")?,
            });
        }
        for r in sqlx::query("SELECT * FROM work_orders")
            .fetch_all(p)
            .await?
        {
            ds.work_orders.push(WorkOrder {
                id: r.try_get("id")?,
                tail: r.try_get("tail")?,
                event_ids: r.try_get("event_ids")?,
                component_id: r.try_get("component_id")?,
                ata: r.try_get::<i16, _>("ata")? as u8,
                action: e(&r.try_get::<String, _>("action")?)?,
                opened: r.try_get("opened")?,
                closed: r.try_get("closed")?,
                signed_off: r.try_get("signed_off")?,
                team: r.try_get("team")?,
                note: r.try_get("note")?,
            });
        }
        for r in sqlx::query("SELECT * FROM mel_items").fetch_all(p).await? {
            ds.mel.push(MelItem {
                id: r.try_get("id")?,
                tail: r.try_get("tail")?,
                reference: r.try_get("reference")?,
                title: r.try_get("title")?,
                category: r
                    .try_get::<String, _>("category")?
                    .chars()
                    .next()
                    .unwrap_or('D'),
                ata: r.try_get::<i16, _>("ata")? as u8,
                opened: r.try_get("opened")?,
                due: r.try_get("due")?,
            });
        }
        for r in
            sqlx::query("SELECT ts, tail, param, flight_id, value FROM param_samples ORDER BY ts")
                .fetch_all(p)
                .await?
        {
            ds.samples.push(ParamSample {
                ts: r.try_get("ts")?,
                tail: r.try_get("tail")?,
                param: e(&r.try_get::<String, _>("param")?)?,
                flight_id: r.try_get("flight_id")?,
                value: r.try_get("value")?,
            });
        }
        for r in sqlx::query("SELECT doc FROM flight_summaries")
            .fetch_all(p)
            .await?
        {
            ds.flight_summaries
                .push(serde_json::from_value(r.try_get("doc")?)?);
        }
        Ok((ds, truth))
    }

    pub async fn load_decisions(&self) -> anyhow::Result<Decisions> {
        let mut d = Decisions::default();
        for r in sqlx::query("SELECT * FROM hypothesis_grades")
            .fetch_all(&self.pool)
            .await?
        {
            d.grades.insert(
                r.try_get("hypothesis_id")?,
                GradeDecision {
                    grade: e(&r.try_get::<String, _>("grade")?)?,
                    by: r.try_get("graded_by")?,
                    at: r.try_get("graded_at")?,
                    note: r.try_get("note")?,
                },
            );
        }
        for r in sqlx::query("SELECT * FROM alert_feedback")
            .fetch_all(&self.pool)
            .await?
        {
            d.feedback.insert(
                r.try_get("alert_id")?,
                AlertFeedback {
                    useful: r.try_get("useful")?,
                    comment: r.try_get("comment")?,
                    by: r.try_get("by_user")?,
                    at: r.try_get("at")?,
                },
            );
        }
        for r in sqlx::query("SELECT case_id, state FROM case_state")
            .fetch_all(&self.pool)
            .await?
        {
            let st: CaseHuman = serde_json::from_value(r.try_get("state")?)?;
            d.cases.insert(r.try_get("case_id")?, st);
        }
        for (k, v) in self.load_docs("alert_ack").await? {
            d.alert_acks.insert(k, serde_json::from_value(v)?);
        }
        for (_, v) in self.load_docs("correction").await? {
            d.corrections.push(serde_json::from_value(v)?);
        }
        d.corrections
            .sort_by_key(|c: &aips_domain::intel::Correction| (c.at, c.version));
        for (k, v) in self.load_docs("preflight_escalation").await? {
            d.preflight_escalations
                .insert(k, serde_json::from_value(v)?);
        }
        for (_, v) in self.load_docs("erasure").await? {
            d.erasure_requests.push(serde_json::from_value(v)?);
        }
        for r in sqlx::query("SELECT * FROM preflight_acks")
            .fetch_all(&self.pool)
            .await?
        {
            d.preflight_acks.insert(
                r.try_get("flight_id")?,
                (r.try_get("by_user")?, r.try_get("at")?),
            );
        }
        Ok(d)
    }

    pub async fn load_audit(&self) -> anyhow::Result<AuditChain> {
        let mut c = AuditChain::default();
        for r in sqlx::query("SELECT * FROM audit_log ORDER BY seq")
            .fetch_all(&self.pool)
            .await?
        {
            c.entries.push(AuditEntry {
                seq: r.try_get::<i64, _>("seq")? as u64,
                at: r.try_get("at")?,
                actor: r.try_get("actor")?,
                action: r.try_get("action")?,
                subject: r.try_get("subject")?,
                detail: r.try_get("detail")?,
                prev_hash: r.try_get("prev_hash")?,
                hash: r.try_get("hash")?,
            });
        }
        Ok(c)
    }

    pub async fn append_audit(&self, a: &AuditEntry) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO audit_log VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(a.seq as i64)
            .bind(a.at)
            .bind(&a.actor)
            .bind(&a.action)
            .bind(&a.subject)
            .bind(&a.detail)
            .bind(&a.prev_hash)
            .bind(&a.hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Generic persisted decision (upsert, except corrections which are insert-only).
    pub async fn save_doc<T: Serialize>(
        &self,
        kind: &str,
        key: &str,
        doc: &T,
    ) -> anyhow::Result<()> {
        let v = serde_json::to_value(doc)?;
        if kind == "correction" {
            sqlx::query("INSERT INTO decision_docs (kind, key, doc) VALUES ($1,$2,$3)")
                .bind(kind)
                .bind(key)
                .bind(v)
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("INSERT INTO decision_docs (kind, key, doc) VALUES ($1,$2,$3) ON CONFLICT (kind, key) DO UPDATE SET doc = EXCLUDED.doc, at = now()")
                .bind(kind).bind(key).bind(v).execute(&self.pool).await?;
        }
        Ok(())
    }

    /// Insert-once: returns false when the key already exists. Atomic in PostgreSQL, so concurrent
    /// requests cannot both win (e.g. two simultaneous acknowledgements of one alert).
    pub async fn save_doc_once<T: Serialize>(
        &self,
        kind: &str,
        key: &str,
        doc: &T,
    ) -> anyhow::Result<bool> {
        let v = serde_json::to_value(doc)?;
        let r = sqlx::query("INSERT INTO decision_docs (kind, key, doc) VALUES ($1,$2,$3) ON CONFLICT (kind, key) DO NOTHING")
            .bind(kind).bind(key).bind(v).execute(&self.pool).await?;
        Ok(r.rows_affected() == 1)
    }

    /// Release a claim taken with `save_doc_once` (not allowed for corrections, which are append-only).
    pub async fn delete_doc(&self, kind: &str, key: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM decision_docs WHERE kind = $1 AND key = $2")
            .bind(kind)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn load_docs(&self, kind: &str) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
        let rows =
            sqlx::query("SELECT key, doc FROM decision_docs WHERE kind = $1 ORDER BY at, key")
                .bind(kind)
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|r| Ok((r.try_get("key")?, r.try_get("doc")?)))
            .collect()
    }

    /// DAT-03: erase every operational record of one aircraft (after two-person approval).
    pub async fn erase_tail(&self, tail: &str) -> anyhow::Result<u64> {
        let mut tx = self.pool.begin().await?;
        let mut n = 0;
        for t in [
            "flight_summaries",
            "param_samples",
            "mel_items",
            "work_orders",
            "fault_events",
            "flights",
            "components",
            "projection_hypotheses",
            "projection_alerts",
            "projection_cases",
            "projection_preflight",
            "aircraft",
        ] {
            n += sqlx::query(&format!("DELETE FROM {t} WHERE tail = $1"))
                .bind(tail)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        }
        tx.commit().await?;
        Ok(n)
    }

    pub async fn save_clock(&self, clock: &crate::engine::Clock) -> anyhow::Result<()> {
        sqlx::query("UPDATE replay_meta SET clock = $1")
            .bind(serde_json::to_value(clock)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn load_clock(&self) -> anyhow::Result<Option<crate::engine::Clock>> {
        let v: Option<serde_json::Value> = sqlx::query_scalar("SELECT clock FROM replay_meta")
            .fetch_optional(&self.pool)
            .await?
            .flatten();
        Ok(v.and_then(|x| serde_json::from_value(x).ok()))
    }

    pub async fn insert_event(&self, x: &FaultEvent) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO fault_events VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT (id) DO NOTHING")
            .bind(&x.id).bind(&x.tail).bind(&x.flight_id).bind(x.ts).bind(x.ata as i16).bind(s(&x.system))
            .bind(&x.code).bind(&x.component_id).bind(s(&x.severity)).bind(&x.message).bind(s(&x.source)).bind(x.ingested_at)
            .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn save_grade(&self, id: &str, g: &GradeDecision) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO hypothesis_grades VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (hypothesis_id) DO UPDATE SET grade=EXCLUDED.grade, graded_by=EXCLUDED.graded_by, graded_at=EXCLUDED.graded_at, note=EXCLUDED.note",
        )
        .bind(id).bind(s(&g.grade)).bind(&g.by).bind(g.at).bind(&g.note)
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn save_feedback(&self, id: &str, f: &AlertFeedback) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO alert_feedback VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (alert_id) DO UPDATE SET useful=EXCLUDED.useful, comment=EXCLUDED.comment, by_user=EXCLUDED.by_user, at=EXCLUDED.at",
        )
        .bind(id).bind(f.useful).bind(&f.comment).bind(&f.by).bind(f.at)
        .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn save_case(&self, id: &str, c: &CaseHuman) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO case_state (case_id, state) VALUES ($1,$2) ON CONFLICT (case_id) DO UPDATE SET state=EXCLUDED.state, updated=now()")
            .bind(id).bind(serde_json::to_value(c)?)
            .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn save_ack(
        &self,
        flight_id: &str,
        by: &str,
        at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO preflight_acks VALUES ($1,$2,$3) ON CONFLICT (flight_id) DO UPDATE SET by_user=EXCLUDED.by_user, at=EXCLUDED.at")
            .bind(flight_id).bind(by).bind(at)
            .execute(&self.pool).await?;
        Ok(())
    }

    /// Upsert read models so BI tools and integrations can query current intelligence in SQL.
    pub async fn project(&self, snap: &Snapshot) -> anyhow::Result<()> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        macro_rules! upsert {
            ($table:literal, $key:literal, $items:expr, $id:expr, $tail:expr) => {{
                let ids: Vec<String> = $items.iter().map($id).collect();
                let tails: Vec<String> = $items.iter().map($tail).collect();
                let docs: Vec<serde_json::Value> = $items.iter().map(|x| serde_json::to_value(x).unwrap_or_default()).collect();
                sqlx::query(concat!("DELETE FROM ", $table)).execute(&mut *tx).await?;
                sqlx::query(concat!(
                    "INSERT INTO ", $table, " (", $key, ", tail, doc, updated) SELECT k, t, d, $4 FROM UNNEST($1::text[], $2::text[], $3::jsonb[]) AS u(k, t, d)"
                ))
                .bind(ids).bind(tails).bind(docs).bind(now)
                .execute(&mut *tx).await?;
            }};
        }
        upsert!(
            "projection_hypotheses",
            "id",
            snap.hypotheses,
            |h| h.id.clone(),
            |h| h.tail.clone()
        );
        upsert!(
            "projection_alerts",
            "id",
            snap.alerts,
            |a| a.id.clone(),
            |a| a.tail.clone()
        );
        upsert!(
            "projection_cases",
            "id",
            snap.cases,
            |c| c.id.clone(),
            |c| c.tail.clone()
        );
        upsert!(
            "projection_preflight",
            "flight_id",
            snap.preflight,
            |p| p.flight_id.clone(),
            |p| p.tail.clone()
        );
        tx.commit().await?;
        Ok(())
    }
}
