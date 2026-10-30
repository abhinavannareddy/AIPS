//! Neo4j projection of Cross-Flight Memory: Aircraft, Component, Event, WorkOrder,
//! Hypothesis and Case nodes with their relationships (PRD section 9 entity model).
use crate::engine::Snapshot;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GraphView {
    pub source: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// In-process graph for one aircraft, derived from the current snapshot.
pub fn local_view(snap: &Snapshot, tail: &str) -> GraphView {
    let mut v = GraphView {
        source: "aips-core (in-process)".into(),
        ..Default::default()
    };
    let ds = &snap.visible;
    let mut seen = std::collections::HashSet::new();
    let mut node =
        |v: &mut GraphView, id: String, kind: &str, label: String, detail: serde_json::Value| {
            if seen.insert(id.clone()) {
                v.nodes.push(GraphNode {
                    id,
                    kind: kind.into(),
                    label,
                    detail,
                });
            }
        };
    let ac = format!("ac:{tail}");
    node(
        &mut v,
        ac.clone(),
        "aircraft",
        tail.to_string(),
        serde_json::json!({}),
    );
    for h in snap.hypotheses.iter().filter(|h| h.tail == tail) {
        let hid = format!("h:{}", h.id);
        node(
            &mut v,
            hid.clone(),
            "hypothesis",
            h.title.clone(),
            serde_json::json!({ "grade": h.grade, "kind": h.kind, "id": h.id }),
        );
        v.edges.push(GraphEdge {
            from: ac.clone(),
            to: hid.clone(),
            kind: "HAS_HYPOTHESIS".into(),
        });
        for eid in &h.event_ids {
            if let Some(e) = ds.events.iter().find(|e| &e.id == eid) {
                let en = format!("e:{eid}");
                node(
                    &mut v,
                    en.clone(),
                    "event",
                    e.code.clone(),
                    serde_json::json!({ "ts": e.ts, "message": e.message, "system": e.system, "flight": e.flight_id }),
                );
                v.edges.push(GraphEdge {
                    from: hid.clone(),
                    to: en.clone(),
                    kind: "LINKS".into(),
                });
                if let Some(c) = &e.component_id {
                    let cn = format!("c:{c}");
                    let label = ds
                        .components
                        .iter()
                        .find(|x| &x.id == c)
                        .map(|x| x.description.clone())
                        .unwrap_or_else(|| c.clone());
                    node(
                        &mut v,
                        cn.clone(),
                        "component",
                        label,
                        serde_json::json!({ "id": c }),
                    );
                    v.edges.push(GraphEdge {
                        from: en.clone(),
                        to: cn,
                        kind: "INVOLVES".into(),
                    });
                }
            }
        }
        for wid in &h.work_order_ids {
            if let Some(w) = ds.work_orders.iter().find(|w| &w.id == wid) {
                let wn = format!("w:{wid}");
                node(
                    &mut v,
                    wn.clone(),
                    "work_order",
                    format!("{:?}", w.action),
                    serde_json::json!({ "closed": w.closed, "note": w.note, "signed_off": w.signed_off }),
                );
                for eid in &w.event_ids {
                    v.edges.push(GraphEdge {
                        from: wn.clone(),
                        to: format!("e:{eid}"),
                        kind: "ADDRESSES".into(),
                    });
                }
            }
        }
    }
    for c in snap.cases.iter().filter(|c| c.tail == tail) {
        let cn = format!("k:{}", c.id);
        node(
            &mut v,
            cn.clone(),
            "case",
            c.id.clone(),
            serde_json::json!({ "status": c.status, "title": c.title }),
        );
        for h in &c.hypothesis_ids {
            v.edges.push(GraphEdge {
                from: cn.clone(),
                to: format!("h:{h}"),
                kind: "INVESTIGATES".into(),
            });
        }
    }
    let ids: std::collections::HashSet<&str> = v.nodes.iter().map(|n| n.id.as_str()).collect();
    let edges: Vec<GraphEdge> = v
        .edges
        .iter()
        .filter(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()))
        .cloned()
        .collect();
    v.edges = edges;
    v
}

#[cfg(feature = "neo4j")]
pub mod neo {
    use super::*;
    use neo4rs::{query, BoltType, Graph};
    use std::collections::HashMap;

    pub struct Neo {
        graph: Graph,
    }

    type Row = HashMap<String, BoltType>;

    fn row(pairs: &[(&str, BoltType)]) -> Row {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    impl Neo {
        pub async fn connect(uri: &str, user: &str, pass: &str) -> anyhow::Result<Self> {
            let graph = Graph::new(uri, user, pass).await?;
            for c in [
                "CREATE CONSTRAINT aircraft_tail IF NOT EXISTS FOR (a:Aircraft) REQUIRE a.tail IS UNIQUE",
                "CREATE CONSTRAINT component_id IF NOT EXISTS FOR (c:Component) REQUIRE c.id IS UNIQUE",
                "CREATE CONSTRAINT event_id IF NOT EXISTS FOR (e:Event) REQUIRE e.id IS UNIQUE",
                "CREATE CONSTRAINT workorder_id IF NOT EXISTS FOR (w:WorkOrder) REQUIRE w.id IS UNIQUE",
                "CREATE CONSTRAINT hypothesis_id IF NOT EXISTS FOR (h:Hypothesis) REQUIRE h.id IS UNIQUE",
                "CREATE CONSTRAINT case_id IF NOT EXISTS FOR (k:Case) REQUIRE k.id IS UNIQUE",
            ] {
                graph.run(query(c)).await?;
            }
            Ok(Self { graph })
        }

        /// Idempotent MERGE projection of the current snapshot.
        pub async fn project(&self, snap: &Snapshot) -> anyhow::Result<()> {
            let ds = &snap.visible;
            let aircraft: Vec<Row> = ds
                .aircraft
                .iter()
                .map(|a| {
                    row(&[
                        ("tail", a.tail.clone().into()),
                        ("type", a.type_code.clone().into()),
                        ("operator", a.operator.clone().into()),
                    ])
                })
                .collect();
            self.graph
                .run(query("UNWIND $rows AS r MERGE (a:Aircraft {tail: r.tail}) SET a.type = r.type, a.operator = r.operator").param("rows", aircraft))
                .await?;
            let comps: Vec<Row> = ds
                .components
                .iter()
                .map(|c| {
                    row(&[
                        ("id", c.id.clone().into()),
                        ("tail", c.tail.clone().into()),
                        ("pn", c.part_number.clone().into()),
                        ("sn", c.serial.clone().into()),
                        ("desc", c.description.clone().into()),
                    ])
                })
                .collect();
            self.graph
                .run(query("UNWIND $rows AS r MERGE (c:Component {id: r.id}) SET c.pn = r.pn, c.sn = r.sn, c.description = r.desc
                            WITH c, r MATCH (a:Aircraft {tail: r.tail}) MERGE (a)-[:HAS_COMPONENT]->(c)").param("rows", comps))
                .await?;
            // Events referenced by hypotheses (the memory graph), not every nuisance message.
            let linked: std::collections::HashSet<&str> = snap
                .hypotheses
                .iter()
                .flat_map(|h| h.event_ids.iter().map(|s| s.as_str()))
                .collect();
            let events: Vec<Row> = ds
                .events
                .iter()
                .filter(|e| linked.contains(e.id.as_str()))
                .map(|e| {
                    row(&[
                        ("id", e.id.clone().into()),
                        ("tail", e.tail.clone().into()),
                        ("code", e.code.clone().into()),
                        ("ts", e.ts.to_rfc3339().into()),
                        ("msg", e.message.clone().into()),
                        ("comp", e.component_id.clone().unwrap_or_default().into()),
                    ])
                })
                .collect();
            self.graph
                .run(query("UNWIND $rows AS r MERGE (e:Event {id: r.id}) SET e.code = r.code, e.ts = datetime(r.ts), e.message = r.msg
                            WITH e, r MATCH (a:Aircraft {tail: r.tail}) MERGE (a)-[:REPORTED]->(e)
                            WITH e, r WHERE r.comp <> '' MATCH (c:Component {id: r.comp}) MERGE (e)-[:INVOLVES]->(c)").param("rows", events))
                .await?;
            let wos: Vec<Row> = ds
                .work_orders
                .iter()
                .filter(|w| w.event_ids.iter().any(|e| linked.contains(e.as_str())))
                .map(|w| {
                    row(&[
                        ("id", w.id.clone().into()),
                        ("action", format!("{:?}", w.action).into()),
                        ("events", w.event_ids.clone().into()),
                        ("signed", w.signed_off.into()),
                    ])
                })
                .collect();
            self.graph
                .run(query("UNWIND $rows AS r MERGE (w:WorkOrder {id: r.id}) SET w.action = r.action, w.signed_off = r.signed
                            WITH w, r UNWIND r.events AS eid MATCH (e:Event {id: eid}) MERGE (w)-[:ADDRESSES]->(e)").param("rows", wos))
                .await?;
            let hyps: Vec<Row> = snap
                .hypotheses
                .iter()
                .map(|h| {
                    row(&[
                        ("id", h.id.clone().into()),
                        ("tail", h.tail.clone().into()),
                        ("title", h.title.clone().into()),
                        ("grade", h.grade.label().to_string().into()),
                        ("events", h.event_ids.clone().into()),
                    ])
                })
                .collect();
            self.graph
                .run(query("UNWIND $rows AS r MERGE (h:Hypothesis {id: r.id}) SET h.title = r.title, h.grade = r.grade
                            WITH h, r MATCH (a:Aircraft {tail: r.tail}) MERGE (a)-[:HAS_HYPOTHESIS]->(h)
                            WITH h, r UNWIND r.events AS eid MATCH (e:Event {id: eid}) MERGE (h)-[:LINKS]->(e)").param("rows", hyps))
                .await?;
            let cases: Vec<Row> = snap
                .cases
                .iter()
                .map(|c| {
                    row(&[
                        ("id", c.id.clone().into()),
                        ("status", format!("{:?}", c.status).into()),
                        ("hyps", c.hypothesis_ids.clone().into()),
                    ])
                })
                .collect();
            self.graph
                .run(query("UNWIND $rows AS r MERGE (k:Case {id: r.id}) SET k.status = r.status
                            WITH k, r UNWIND r.hyps AS hid MATCH (h:Hypothesis {id: hid}) MERGE (k)-[:INVESTIGATES]->(h)").param("rows", cases))
                .await?;
            Ok(())
        }

        /// DAT-03 erasure: remove the aircraft and everything projected through it (components,
        /// events, hypotheses, and the work orders and cases attached to those). MERGE never
        /// deletes, so without this an erased aircraft would stay in the graph indefinitely.
        pub async fn erase_tail(&self, tail: &str) -> anyhow::Result<()> {
            self.graph
                .run(
                    query(
                        "MATCH (a:Aircraft {tail: $tail})
                            OPTIONAL MATCH (a)-[:HAS_COMPONENT|REPORTED|HAS_HYPOTHESIS]->(n)
                            OPTIONAL MATCH (w:WorkOrder)-[:ADDRESSES]->(n)
                            OPTIONAL MATCH (k:Case)-[:INVESTIGATES]->(n)
                            DETACH DELETE a, n, w, k",
                    )
                    .param("tail", tail),
                )
                .await?;
            Ok(())
        }

        /// Read the memory subgraph for one aircraft straight from Neo4j.
        pub async fn view(&self, tail: &str) -> anyhow::Result<GraphView> {
            let mut v = GraphView {
                source: "neo4j".into(),
                ..Default::default()
            };
            let mut rs = self
                .graph
                .execute(
                    query("MATCH (a:Aircraft {tail: $tail})-[:HAS_HYPOTHESIS]->(h:Hypothesis)-[:LINKS]->(e:Event)
                           OPTIONAL MATCH (e)-[:INVOLVES]->(c:Component)
                           RETURN h.id AS hid, h.title AS title, h.grade AS grade, e.id AS eid, e.code AS code, c.id AS cid, c.description AS cdesc")
                        .param("tail", tail),
                )
                .await?;
            let mut seen = std::collections::HashSet::new();
            let ac = format!("ac:{tail}");
            v.nodes.push(GraphNode {
                id: ac.clone(),
                kind: "aircraft".into(),
                label: tail.into(),
                detail: serde_json::json!({}),
            });
            while let Some(r) = rs.next().await? {
                let hid: String = r.get("hid").unwrap_or_default();
                let eid: String = r.get("eid").unwrap_or_default();
                let hn = format!("h:{hid}");
                let en = format!("e:{eid}");
                if seen.insert(hn.clone()) {
                    v.nodes.push(GraphNode { id: hn.clone(), kind: "hypothesis".into(), label: r.get("title").unwrap_or_default(),
                                             detail: serde_json::json!({ "grade": r.get::<String>("grade").unwrap_or_default(), "id": hid }) });
                    v.edges.push(GraphEdge {
                        from: ac.clone(),
                        to: hn.clone(),
                        kind: "HAS_HYPOTHESIS".into(),
                    });
                }
                if seen.insert(en.clone()) {
                    v.nodes.push(GraphNode {
                        id: en.clone(),
                        kind: "event".into(),
                        label: r.get("code").unwrap_or_default(),
                        detail: serde_json::json!({}),
                    });
                }
                v.edges.push(GraphEdge {
                    from: hn,
                    to: en.clone(),
                    kind: "LINKS".into(),
                });
                if let Ok(cid) = r.get::<String>("cid") {
                    let cn = format!("c:{cid}");
                    if seen.insert(cn.clone()) {
                        v.nodes.push(GraphNode {
                            id: cn.clone(),
                            kind: "component".into(),
                            label: r.get("cdesc").unwrap_or_default(),
                            detail: serde_json::json!({}),
                        });
                    }
                    v.edges.push(GraphEdge {
                        from: en,
                        to: cn,
                        kind: "INVOLVES".into(),
                    });
                }
            }
            Ok(v)
        }
    }
}
