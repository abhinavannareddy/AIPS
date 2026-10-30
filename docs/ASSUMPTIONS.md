# Assumptions and decisions

1. Phase 0 scope follows PRD section 14: Integrity, Predict, Prevent, Workflow, Cross-Flight Memory, Passport, Command, and Guardian in training mode. Flight was added in the second iteration on synthetic trajectory summaries. Autoland integration (Phase 8) is not built; Autoland exists only as a capability boundary.
2. Synthetic data only (PRD section 11). Parameter samples are one summary per flight per parameter, not full QAR streams.
3. The Guardian emergency path runs on its own Rust state machine with a local journal instead of Temporal, so that it has no dependency on the shared workflow cluster (EMG-01). Temporal handles investigation SLAs.
4. Temporal has no production Rust SDK, so the workflow worker is TypeScript (in the approved stack).
5. The UI is served by aips-core to avoid adding a web server outside the approved stack.
6. "AIPS Autopilot" from the PRD is presented as "Investigations" and workflow automation in the UI to avoid confusion with the flight autopilot (PRD open question Q3).
7. Drill times use wall-clock time; fleet analysis uses the Replay Lab simulation clock. Both are shown in UTC.
8. Python is used only to train and evaluate the model and for one security test harness (about 1.3% of the code).
9. Weather (METAR-style) and NOTAMs come from a deterministic synthetic provider seeded by airport and time. No live aviation weather or NOTAM feed is connected.
10. Flight trajectories are per-flight summaries from the Replay Lab (ADS-B/QAR substitute); detailed tracks are reconstructed from them for display.
11. Kafka integration was verified against Tansu, a broker that implements the Apache Kafka wire protocol, because Apache Kafka binaries could not be downloaded. Behaviour specific to Apache Kafka (for example KRaft failover) is not covered.
12. Guardian procedure cards shipped with the code are training examples. Real content must come from the operator's approved operations manual through `AIPS_GUARDIAN_PROCEDURES`.
13. Response caching is per analysis snapshot; at very large histories, payload size rather than computation limits throughput, and pagination would require an approved API change.
14. PRD open question Q4 (Neo4j or a graph layer on PostgreSQL): for Phase 0 the Cross-Flight Memory graph is computed in process from PostgreSQL data, and Neo4j is an optional projection for exploration. At 50 to 500 aircraft the in-process graph is fast enough (the memory stage takes 34 to 108 ms per analysis pass), so we recommend PostgreSQL as the system of record and keeping Neo4j optional until a design partner's data shows a need for graph queries across fleets. This should be confirmed by the engineering lead.
15. PRD open question Q3: the UI calls the workflow module "Investigations" rather than "Autopilot". The product owner still has to decide on the final name.
