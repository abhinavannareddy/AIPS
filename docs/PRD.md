# AIPS (Aircraft Intelligence & Protection System): Product Requirements Document

Oct 29, 2026 · @Carbonlenz

## 1. Executive summary

AIPS is a ground-first, advisory aviation safety platform that joins aircraft health, maintenance history, flight data and emergency coordination into one long-term record per aircraft. Version 1 (Phases 0–4) never touches flight controls and never authorises or prohibits a departure; those decisions stay with legally authorised people.

**Mission:** Prevent what can be prevented. Detect what goes wrong. Respond when people cannot.

**Vision:** An aircraft should never depend on a single person or a disconnected information system to recognise that something is wrong and start an appropriate response.

| Item | Value |
| --- | --- |
| Product | Aircraft Intelligence & Protection System (AIPS) |
| Former working name | AEGIS ONE (all AEGIS module names are now AIPS names, e.g. AEGIS PREVENT → AIPS Prevent) |
| Document status | Draft v0.1 for review |
| Product owner | To be assigned |
| Intended readers | Founders, investors, engineering, safety and certification leads, pilot customers |
| Scope of v1 | Ground-based advisory and workflow platform on authorised data interfaces, validated on simulated and licensed historical data |

**Five decisions this PRD makes**

1. **Ground-first, advisory-only.** Airborne or flight-control integration is a later, partner-led programme (Phase 8), not a v1 feature.
2. **Cross-Flight Memory is the lead differentiator.** It carries the first demonstrator, together with recurring-defect intelligence and pre-flight assurance.
3. **Autoland means integration, not invention.** AIPS connects to emergency landing systems that are already certified on a given aircraft. It does not land aircraft.
4. **Suspected is not established.** Every AIPS finding carries an evidence grade, and the product never presents a correlation as an engineering cause.
5. **Accidents drive requirements, not marketing claims.** AIPS will not state that it would have prevented a specific accident (see section 2).

**Module name map**

| AIPS module | Former name | One-line purpose |
| --- | --- | --- |
| AIPS Integrity | AEGIS INTEGRITY | Aircraft health intelligence |
| AIPS Predict | AEGIS PREDICT | Predictive maintenance and recurring defects |
| AIPS Flight | AEGIS FLIGHT | Flight anomaly detection |
| AIPS Guardian | AEGIS GUARDIAN | Cockpit and cabin emergency intelligence |
| AIPS Autoland | AEGIS AUTOLAND | Integration with certified emergency landing systems |
| AIPS Autopilot | AEGIS AUTOPILOT | Digital workflow automation (not flight control) |
| AIPS Prevent | AEGIS PREVENT | Pre-flight safety assurance |
| AIPS Passport | AEGIS PASSPORT | Digital aircraft safety record |
| AIPS Command | AEGIS COMMAND | Enterprise dashboard |

**Naming note:** "Autopilot" in AIPS Autopilot means workflow automation, and it is easy to confuse with the aircraft autopilot. Consider renaming it "AIPS Workflow" before launch (open question Q3).

## 2. Problem statement and market context

Safety-relevant information about one aircraft sits in separate systems owned by the OEM, the airline, the MRO and the lessor, and nobody sees the full history at the moment a decision is made. Separately, a small number of in-flight events (incapacitation, cockpit attack, onboard fire) leave very little time and depend on one or two people.

**Problems AIPS addresses**

1. **Fragmented history.** A fault, its repair and its recurrence are recorded in different systems, so repeat defects and related faults across systems are found late.
2. **Late recognition before departure.** Outstanding defects, MEL items and recent anomalies are reviewed manually and inconsistently.
3. **Single points of failure in emergencies.** One pilot, one crew member, one sensor or one channel can be the only path by which an emergency is recognised and escalated.
4. **Manual operational paperwork.** Pre-flight, post-flight and defect-follow-up workflows are repetitive and error-prone.

**What recent accidents do and do not tell us**

- **Air India AI171 (Boeing 787-8, 12 June 2025).** The preliminary AAIB report found that the fuel control switches of both engines moved from RUN to CUTOFF about a second apart, shortly after take-off, cutting fuel to both engines ([Business Today](https://www.businesstoday.in/amp/latest/corporate/story/air-india-ai-171-crash-no-abnormality-found-in-fuel-switch-says-aviation-ministry-546138-2026-07-30)). The report did not establish how or why the switches moved, and [CBS News](https://www.cbsnews.com/amp/news/air-india-plane-fuel-switches-cut-off-crash-report) notes that no defect on the fuel control switch had been reported since 2023. In July 2026 the Indian government told Parliament that OEM testing of the switch found no abnormality and that the final report was still pending ([Outlook Business](https://www.outlookbusiness.com/news/air-india-ai-171-crash-fuel-control-switch-tests-find-no-defects-centre-informs-parliament)).
- **Germanwings 9525 (2015).** The co-pilot deliberately crashed the aircraft after locking the captain out, which led to the two-person cockpit rule and later to a risk-based approach in Europe ([Free Press Journal](https://freepressjournal.in/world/egyptair-and-germanwings-crashes-exposed-cockpit-risk-prompting-renewed-two-person-flight-deck-rules)). Door security is a trade-off, so AIPS must never weaken it.

**Implication for AIPS.** With the AI171 cause still unestablished in the sources retrieved (latest dated 31 July 2026; check for the final report), AIPS must not claim it would have prevented that accident. Nothing public shows a pre-departure technical warning sign that a monitoring platform could have used. AI171 is useful for requirements instead: capture cockpit control-state events, make them available to investigators, and shorten the time from an abnormal event to ATC and operations notification.

**Market size (directional only).** Analyst estimates differ widely, so AIPS uses them for scale and not for planning.

| Segment | Estimate | Source |
| --- | --- | --- |
| Aircraft predictive maintenance | USD 6.9B (2026) to USD 16.8B (2035), 10.4% CAGR | [Global Market Insights](https://www.gminsights.com/industry-analysis/aircraft-predictive-maintenance-market) |
| Predictive MRO | USD 4.28B (2025) to USD 19.59B (2034), 18.4% CAGR | [TrendX Insights](https://trendxinsights.com/syndicated-market-research-reports/predictive-mro-market/) |
| Aircraft health monitoring systems | USD 6.3B (2023) to USD 8.6B (2028), 6.5% CAGR | [MarketsandMarkets](https://marketsandmarkets.com/ResearchInsight/aircraft-health-monitoring-systems-market.asp) |
| Aircraft MRO (all services) | USD 90.09B (2026) to USD 128.12B (2034), 4.5% CAGR | [Straits Research](https://straitsresearch.com/report/Aircraft-Maintenance-Repair-and-Overhaul-Market) |

The two predictive-maintenance estimates for 2025 differ by about 50% (USD 6.3B vs USD 4.28B), which shows how definition-dependent these numbers are. The serviceable market for AIPS is a subset of these segments and needs bottom-up sizing in Phase 0 (open question Q6).

## 3. Competitive landscape

Health monitoring and predictive maintenance are mature, OEM-backed categories, so AIPS cannot win by being another dashboard. No researched competitor combines cross-organisation aircraft memory, pre-departure evidence review and emergency coordination in one product.

| Player | What it does today | Gap AIPS targets |
| --- | --- | --- |
| [Airbus Skywise](https://www.journal-aviation.com/en/news/42304-mroen1-airbus-s-skywise-platform-now-extends-to-operational-fleet-improvement) (Health Monitoring, Predictive Maintenance+, Reliability) | Airbus-hosted data platform; in 2021 it had over 60 customer airlines, and it ties alerts to maintenance history | Airbus-centred; no cockpit or cabin emergency layer; no cross-OEM lessor or MRO record |
| [Boeing Airplane Health Management](https://services.boeing.com/maintenance-engineering/maintenance-optimization/airplane-health-management-ahm) | Monitors systems in flight and sends real-time faults and predictive alerts before landing | Boeing-centred; focused on dispatch reliability and not on a cross-fleet safety record |
| [Air France KLM Prognos](https://imd.org/research-knowledge/operations/articles/how-industry-4-0-is-transforming-the-aircraft-industry) | Operator-led predictive maintenance for Airbus and Boeing fleets | Predictive focus; no pre-flight evidence assurance or emergency coordination |
| [Garmin Autoland](https://seekingalpha.com/pr/20212732-garmin-autoland-autothrottle-now-certified-for-retrofit-installations-in-select) | Certified emergency autonomous landing; King Air 350 certification in Aug 2025 was the eleventh certified aircraft model | Small turboprops and light jets; no airliner coverage and no ground coordination layer |
| [Airbus DragonFly / ATTOL](https://ngtest.aviationweek.com/aerospace/emerging-technologies/airbus-adds-autonomous-emergency-diversion-capability-flight-tests) | A350-1000 flight-test demonstrators for automated emergency diversion and vision-based landing | Research programme tied to Airbus; not a product available to others |
| Engine OEM services (Rolls-Royce, GE Aerospace) and MRO software vendors | Engine health and maintenance planning | Not researched for this draft; to be profiled in Phase 0 |

**Differentiation thesis.** AIPS competes on three things that OEM platforms are structurally unlikely to offer: (1) a neutral, multi-OEM record that lessors, MROs and airlines can share under permissions; (2) evidence-graded cross-flight reasoning with an investigation workflow; (3) an emergency coordination layer that tracks acknowledgement and escalation across crew, ATC and operations.

**Strategic risk.** OEMs can extend their own platforms into the same space. AIPS should treat OEMs as integration partners and data sources first, and as competitors second.

**Important distinction.** The certified autoland systems above were each certified on specific aircraft by the manufacturer and the avionics supplier. AIPS cannot replicate that on its own (section 5).

## 4. Goals, non-goals, success metrics and users

The v1 goal is to show, on simulated and licensed historical data, that AIPS finds recurring and related defects earlier than manual review, with an alert volume engineers will accept. All numeric targets below are proposals to be validated in the Phase 0 demonstrator.

**Goals**

1. Detect recurring defects and cross-system correlations across flights and maintenance records.
2. Produce a prioritised, evidence-graded pre-departure report for every flight.
3. Automate defect-investigation, follow-up and documentation-completeness workflows.
4. Give each aircraft a permissioned, shareable digital safety record.
5. Coordinate emergency notifications and acknowledgements reliably, including when someone does not answer.

**Non-goals (v1)**

- Controlling any aircraft system or flying any aircraft.
- Declaring an aircraft airworthy, or authorising or prohibiting take-off.
- Inferring hostile intent from flight behaviour, or classifying people by face, ethnicity, appearance or personality.
- Remote control of the cockpit door.
- Replacing mandatory inspections, maintenance release or pilot responsibilities.

**Success metrics (proposed)**

| Metric | Proposed v1 target | How measured |
| --- | --- | --- |
| Recurrence detection recall (known repeat defects in test data) | 90% or higher | Replay of labelled historical defect sequences |
| Alert precision (alerts engineers rate as useful) | 70% or higher | Engineer feedback on each alert |
| Alerts per aircraft per week | 5 or fewer needing human review | Pilot telemetry |
| Time from new fault to investigation case | Under 15 minutes | Workflow timestamps |
| Pre-flight report generation time | Under 60 seconds | System logs |
| Emergency alert acknowledgement tracking | 100% of alerts logged with state | Audit trail |
| Unsupported claims shipped | 0 | Claims review before each release |

**Primary users**

| Persona | Job to be done | Main modules |
| --- | --- | --- |
| Reliability or fleet engineer | Find repeat and related defects, decide on investigations | Integrity, Predict, Passport |
| Maintenance control (MCC) controller | Triage alerts, assign cases, track closure | Autopilot, Command |
| Flight operations and dispatch | Review pre-departure readiness and open issues | Prevent, Command |
| Safety and quality manager | Investigate events, audit evidence and decisions | Passport, Command |
| Cabin crew (emergency mode only) | Receive and acknowledge emergency alerts | Guardian |
| Lessor technical representative | Verify aircraft condition and records at lease events | Passport |
| MRO planner | Share defect history, track repeat repairs | Predict, Passport |

**Customers.** Airlines, MROs and lessors first; OEMs and engine manufacturers are partners and later customers. They are potential targets, not confirmed buyers.

## 5. Safety boundaries and operating principles

AIPS is advisory: it informs authorised people and records evidence, and it never decides. These principles are binding product requirements, and each release is reviewed against them.

| # | Principle | What it means in the product |
| --- | --- | --- |
| P1 | Human authority | Airworthiness, dispatch, maintenance release and flight-deck decisions stay with authorised people; AIPS has no approve or deny action |
| P2 | Evidence before assertion | Every finding shows its data sources, freshness and an evidence grade: Suspected, Corroborated or Engineering-confirmed |
| P3 | No single point of failure | No dependence on one pilot, crew member, sensor, channel or cloud service in any emergency path |
| P4 | Fail safe and fail visible | When data is stale or a service is down, AIPS says so loudly and never shows a silent "all clear" |
| P5 | Observable events only | Detection uses observable safety events; no facial recognition, no ethnicity, appearance or personality-based classification, no intent inference |
| P6 | Minimal personal data | Crew physiological or camera data is used only if approved by the operator, regulator and crew representatives, with a documented purpose and retention limit |
| P7 | Full audit trail | Every alert, acknowledgement, override and model version is recorded and tamper-evident |
| P8 | Honest capability claims | AIPS states what it supports per aircraft type; it never promises emergency landing on every aircraft |

**What AIPS must never do**

- Send commands to flight controls, engines, or any aircraft system.
- Unlock, override or remotely influence the cockpit door.
- Tell a crew or operator that an aircraft "is safe to fly".
- Trigger an emergency landing function directly; it may only forward information to a system that is certified for that function.
- Present a correlation as a root cause.

**Engineering reality on autoland.** Aircraft control and landing must be done by aircraft-specific certified avionics. Garmin's certified system shows what that takes: it is certified per aircraft model, it weighs aircraft performance, terrain, obstacles and weather, and [a pilot can deactivate it at any time](https://generalaviationnews.com/2020/05/19/garmin-autoland-certified-for-general-aviation-aircraft/). For airliners, [Airbus is still testing automated emergency diversion on a research A350-1000](https://ngtest.aviationweek.com/aerospace/emerging-technologies/airbus-adds-autonomous-emergency-diversion-capability-flight-tests). AIPS Autoland therefore stays an information and coordination layer until an OEM partner and a regulator say otherwise.

## 6. Regulatory and certification strategy

As a ground-based advisory platform, v1 sits outside airborne certification but inside the customer's safety management and continuing-airworthiness processes. Any airborne function triggers the full airborne assurance set, and any machine-learning component in it triggers EASA's learning-assurance expectations.

| Framework | Why it matters to AIPS | When it applies |
| --- | --- | --- |
| DO-178C / ED-12C (airborne software) | Required for software that is part of an airborne function at a given assurance level | Phase 8 and any onboard component |
| DO-254 / ED-80 (airborne hardware) | Required for complex airborne electronic hardware | Any dedicated onboard hardware |
| [ARP4754B](https://saemobilus.sae.org/content/arp4754b) (revised Dec 2023) | Aircraft and systems development process; the source document used ARP4754A, so this PRD targets the current revision | Phase 8; also a useful discipline for requirements in earlier phases |
| ARP4761A / ED-135 | System safety assessment (hazard, failure and fault-tree analysis) | Phase 6 onward and Phase 8; the hazard analysis in Phase 0 uses it as a template |
| [DO-326A / ED-202A](https://afuzion.com/?p=3204) and DO-356A / ED-203A | Airworthiness security process and methods; the only acceptable means of compliance for aircraft cybersecurity certification in the US and Europe, per a vendor source | Any onboard link; also guides ground-system security design |
| [DO-355 / ED-204](https://afuzion.com/?p=29078) | Information security guidance for continuing airworthiness (operations and maintenance) | Directly relevant to v1 ground services handling aircraft data |
| [EASA AI Roadmap 2.0 and Concept Paper Issue 02](https://www.easa.europa.eu/en/node/139506) | Level 1 AI (assisting humans) and Level 2 (human-AI teaming); learning assurance, explainability, ethics-based assessment | All ML features; v1 is designed as Level 1 |
| [EASA Concept Paper proposed Issue 03](https://www.icao.int/sites/default/files/APAC/Meetings/2026/2026%20DGCA61/Agenda%20Item03-Aviation%20Safety/61-DP-03-06_EASA_EASA-APPROACH-TO-ARTIFICIAL-INTELLIGENCE-IN-AVIATION.pdf) (3 June 2026) and NPA 2025-07 | Newest EASA direction on AI trustworthiness; still proposed | Track from Phase 0; plan review at each phase gate |
| [EU AI Act and Digital Omnibus](https://spicyadvisory.com/blog/digital-omnibus-ai-act-2026-what-changed) | High-risk duties now apply from 2 Dec 2027 (stand-alone) and 2 Aug 2028 (safety components in regulated products); the Omnibus also amends the Basic Aviation Regulation | Legal review needed on how AIPS is classified |
| GDPR and national labour law | Crew physiological and camera data are personal data and may be special-category data; crew representatives usually must be consulted | Before any Guardian sensing pilot |
| Operator SMS and continuing-airworthiness rules (ICAO Annex 19, EASA Part-CAMO and Part-145, FAA Part 5) | AIPS outputs become inputs to the customer's SMS and records | Not researched in detail here; confirm in Phase 0 |

**Strategy**

1. **Stay advisory in v1.** Document the intended function and the "no safety-critical decision" boundary in a Phase 0 regulatory position paper, and confirm it with a competent authority or aviation counsel.
2. **Build with certification in mind.** Use traceable requirements, independent verification and configuration control from Phase 0, so later airborne work does not mean a rewrite.
3. **Adopt EASA's learning-assurance concept early.** The [W-shaped process](https://easa.europa.eu/nl/downloads/139926/en) adapts the V-cycle to data-driven models; apply its data-management and generalisation evidence to every production model.
4. **Partner for airborne work.** Phase 8 is led with an OEM or avionics supplier that holds the type certificate.
5. **Track a moving target.** Names and dates here (for example, a DO-326 revision some vendors call DO-326B) should be re-verified at each phase gate.

## 7. Product architecture overview

AIPS has three layers: data ingestion, an intelligence core shared by all modules, and role-based applications. Only the last phase reaches into the aircraft.

**Layers**

1. **Ingestion and normalisation.** Connectors bring in flight, health, maintenance and operational data, tag every record with source and freshness, and write to a time-series store, a relational store and a relationship graph.
2. **Intelligence core.** Anomaly detection, cross-system correlation, degradation models, an evidence-grading service and a case engine. Models run as versioned, monitored services.
3. **Workflow and escalation.** A durable workflow engine runs cases, acknowledgements, timers and escalations, so a restart never loses a pending emergency step.
4. **Applications.** AIPS Command (web), Passport sharing, mobile alerts for crew and engineers, and APIs for customers' own systems.
5. **Aircraft integration (Phase 8 only).** Read and forward interfaces to certified onboard systems, built with the aircraft manufacturer.

**Module overview**

| Module | Layer | Phase | EASA AI level (intended) | Certification exposure |
| --- | --- | --- | --- | --- |
| Integrity | Ground | 1 | Level 1 | Low (advisory) |
| Predict | Ground | 2 | Level 1 | Low (advisory) |
| Prevent | Ground | 3 | Level 1 | Medium: must not read as a dispatch decision |
| Autopilot (workflow) | Ground | 4 | Level 1 | Low to medium: records may be used as maintenance evidence |
| Flight | Ground | 5 | Level 1 | Low (advisory) |
| Guardian | Ground, then airborne data | 6 | Level 1 to 2 | High: safety-critical alerting and personal data |
| Passport and Command | Ground | 7 | Not applicable (no ML decisions) | Medium: data integrity and access control |
| Autoland | Integration with certified aircraft systems | 8 | Level 2 or higher | Very high: partner-led, aircraft-specific |

**Deployment tiers**

| Tier | Description | Typical customer |
| --- | --- | --- |
| A: Managed cloud | Multi-tenant SaaS in EU and US regions | Smaller airlines, MROs, lessors |
| B: Sovereign or on-premises | Kubernetes deployment inside the customer's environment | Large airlines, regulated operators |
| C: Aircraft-integrated | Onboard interfaces with an OEM partner | Phase 8 only |

**Design rule for emergencies.** The Guardian and emergency-workflow path must work if the multi-tenant cloud is unavailable, so it has its own deployment, its own data store and at least two independent notification channels (section 10).

## 8. Functional requirements

Each requirement has an ID for traceability. Priority: P0 = needed for the phase to ship, P1 = needed for general availability, P2 = later. "Shall" marks a binding requirement.

### 8.1 AIPS Integrity (Phase 1)

| ID | Requirement | Priority |
| --- | --- | --- |
| INT-01 | The system shall ingest aircraft health data (faults, parameters, sensor status) for engines, electrical, hydraulic, fuel, landing gear, flight controls and cabin pressure and environment, from authorised sources | P0 |
| INT-02 | The system shall detect abnormal behaviour per system and per component using baselines learned per aircraft type and per tail number | P0 |
| INT-03 | The system shall monitor sensor reliability and flag readings that are frozen, out of range or inconsistent with related sensors | P0 |
| INT-04 | The system shall correlate faults across systems within a configurable time window and present the evidence to engineers with an evidence grade | P0 |
| INT-05 | Each alert shall show source data, data age, model version and the reason it fired | P0 |
| INT-06 | Engineers shall be able to confirm, reject or comment on any alert, and the feedback shall be stored for model evaluation | P1 |

### 8.2 AIPS Predict (Phase 2)

| ID | Requirement | Priority |
| --- | --- | --- |
| PRD-01 | The system shall estimate remaining useful life per monitored component with an uncertainty range, and shall not display a single number without it | P0 |
| PRD-02 | The system shall track degradation trends per component and fleet-wide | P0 |
| PRD-03 | The system shall identify recurring defects by matching fault codes, ATA chapter, component and symptom text across flights | P0 |
| PRD-04 | The system shall detect repeated unsuccessful repairs by comparing defect reappearance after each corrective action | P0 |
| PRD-05 | The system shall open an investigation case automatically when a recurrence threshold is met, and attach the related history | P1 |
| PRD-06 | The system shall report fleet-wide component reliability and maintenance-effectiveness views | P1 |

### 8.3 AIPS Prevent (Phase 3)

| ID | Requirement | Priority |
| --- | --- | --- |
| PRV-01 | Before each departure the system shall assemble available evidence for engines, electrical, fuel, hydraulics, flight controls, configuration, maintenance, restrictions and documentation, and compare it with the aircraft's history | P0 |
| PRV-02 | The report shall classify each item as Complete, Missing, Inconsistent, Outstanding or Requires authorised approval | P0 |
| PRV-03 | The report shall rank concerns by severity and evidence grade, and shall show data freshness for every source | P0 |
| PRV-04 | Significant concerns shall be escalated to named authorised engineering and operations roles, with acknowledgement tracking | P0 |
| PRV-05 | The report and every screen shall state that AIPS does not declare airworthiness or authorise take-off | P0 |
| PRV-06 | The report shall mark an aircraft with stale or missing critical data as "Insufficient data", never as clear | P0 |
| PRV-07 | Where an approved cockpit interface exists, the system may forward alerts during engine start, taxi and take-off | P2 |

### 8.4 AIPS Autopilot, the workflow engine (Phase 4)

| ID | Requirement | Priority |
| --- | --- | --- |
| WRK-01 | Pre-flight: retrieve technical status, maintenance history, open defects, MEL items, documents, weather and NOTAMs, and check digital record completeness | P0 |
| WRK-02 | Maintenance: collect fault reports, create cases, assign them to authorised teams, track corrective actions and check documentation completeness | P0 |
| WRK-03 | In-flight: generate prioritised alerts from available data, record safety events, notify approved ground personnel, track acknowledgement and escalation | P1 |
| WRK-04 | Post-flight: collect flight reports, identify new defects, compare with history, open cases, produce a flight safety summary and update the Passport | P0 |
| WRK-05 | Workflows shall survive restarts and shall escalate on timeout according to configurable rules | P0 |
| WRK-06 | Humans shall be able to override, reassign or close any automated step, with the reason recorded | P0 |

### 8.5 AIPS Flight (Phase 5)

| ID | Requirement | Priority |
| --- | --- | --- |
| FLT-01 | The system shall monitor trajectory, speed, altitude and vertical speed from available feeds (for example ADS-B and ACARS) and flag observable anomalies | P0 |
| FLT-02 | The system shall correlate flight events with aircraft-system events and with weather and operational context | P0 |
| FLT-03 | The system shall record a time-stamped safety event timeline | P0 |
| FLT-04 | The system shall not infer intent; it shall present observable facts and evidence for authorised review | P0 |

### 8.6 AIPS Guardian (Phase 6)

| ID | Requirement | Priority |
| --- | --- | --- |
| GRD-01 | The system shall evaluate possible pilot incapacitation or cockpit emergency by combining multiple independent indicators (approved physiological signals, cockpit activity, crew reports, aircraft-system data) and shall never rely on one indicator | P0 |
| GRD-02 | Detection shall report a confidence level and the indicators that contributed | P0 |
| GRD-03 | The system shall integrate read-only with authorised cockpit-door and security status (door state, access attempts, alerts) | P1 |
| GRD-04 | The system shall support detection and reporting of smoke and fire, medical emergencies, falls, physical altercations and restricted-area access | P1 |
| GRD-05 | Any camera analysis shall be limited to observable events and shall not perform face recognition or classify people by appearance | P0 |
| GRD-06 | Designated cabin crew shall receive an immediate emergency notification on at least two independent channels, and each crew member shall be able to acknowledge | P0 |
| GRD-07 | The system shall track which crew members have acknowledged, shall not assume anyone is available because they are on board, and shall escalate when nobody acknowledges within the configured time | P0 |
| GRD-08 | The system shall present authorised emergency information and procedures to crew who acknowledge | P1 |
| GRD-09 | A false-trigger path shall exist: crew or pilots can cancel an alert, and every cancel is recorded | P0 |

### 8.7 AIPS Autoland (Phase 8)

| ID | Requirement | Priority |
| --- | --- | --- |
| AUT-01 | The system shall integrate only with emergency landing capability that is certified on the specific aircraft, and shall state the supported aircraft types and conditions in product documentation | P0 |
| AUT-02 | The system shall forward qualifying incapacitation events to the certified system through its approved interface and shall not itself command the aircraft | P0 |
| AUT-03 | The system shall provide emergency destination, weather and airport information to ATC and operations through approved channels | P1 |
| AUT-04 | The system shall preserve the complete emergency timeline for investigators | P0 |
| AUT-05 | Any airborne component shall be developed under the standards in section 6 with the aircraft manufacturer | P0 |

### 8.8 AIPS Passport (Phase 7)

| ID | Requirement | Priority |
| --- | --- | --- |
| PSP-01 | Each aircraft shall have a record of configuration, component history, maintenance, recurring defects, flight anomalies, incidents, open investigations, corrective actions, engineering assessments and data-quality information | P0 |
| PSP-02 | Every entry shall keep its source, author, timestamp and evidence grade, and shall be append-only with versioned corrections | P0 |
| PSP-03 | The record shall be shareable with airlines, manufacturers, MROs and lessors under per-field permissions that follow data ownership | P0 |
| PSP-04 | The record shall export in an open, documented format and shall support transfer at lease return or sale | P1 |

### 8.9 AIPS Command (Phase 7)

| ID | Requirement | Priority |
| --- | --- | --- |
| CMD-01 | Fleet view with technical health, alerts, predictive overview and pre-flight readiness | P0 |
| CMD-02 | Emergency event management view with live acknowledgement state and the incident timeline | P0 |
| CMD-03 | Maintenance workflow monitoring and case management | P0 |
| CMD-04 | Role-based access control with least privilege and per-organisation isolation | P0 |
| CMD-05 | Configurable reports and analytics, with export | P1 |
| CMD-06 | Investigation timeline view combining flights, faults, maintenance actions and alerts for one aircraft | P0 |

## 9. Cross-Flight Memory

Cross-Flight Memory lets an aircraft's past events inform its next safety assessment. It is the lead differentiator, so it carries the first demonstrator and the strictest evidence rules.

**Worked example.** Flight 101 records an electrical irregularity; maintenance investigates and documents a fix. Flight 102 shows a similar irregularity. AIPS recognises the recurrence, retrieves the earlier case, finds possibly related abnormalities in other systems, opens a case, notifies engineering, and then tracks the corrective action and the following flights.

**Entities and relationships** (stored as a graph, with time-series data kept in the time-series store)

| Entity | Key links |
| --- | --- |
| Aircraft (tail number) | has Components, flies Flights, has Cases |
| Component (part number, serial, position) | installed on Aircraft, removed or replaced by Work Orders |
| Event (fault, parameter excursion, pilot report) | occurs in Flight, relates to Component and ATA chapter |
| Work Order (corrective action) | addresses Event, touches Component |
| Case (investigation) | groups Events and Work Orders, holds Hypotheses |
| Hypothesis (suspected relationship) | has an evidence grade and the data that supports it |

**Requirements**

| ID | Requirement | Priority |
| --- | --- | --- |
| CFM-01 | The system shall match new events against prior events on the same aircraft, same component and same type by fault code, ATA chapter, symptom text and parameter signature | P0 |
| CFM-02 | The system shall link each event to maintenance actions taken afterward and shall flag recurrence after a documented fix | P0 |
| CFM-03 | Each relationship shall carry an evidence grade (Suspected, Corroborated, Engineering-confirmed) and only authorised engineers shall be able to promote it | P0 |
| CFM-04 | The system shall show why two events were linked, including the matching fields and the time gap | P0 |
| CFM-05 | Fleet-level patterns shall be computed without exposing one customer's data to another unless that customer has opted in | P0 |
| CFM-06 | The system shall keep tracking an open case across later flights until engineering closes it | P0 |
| CFM-07 | The system shall measure its own correlation precision using engineer feedback and shall report it in Command | P1 |

**Guard against false patterns.** Many unrelated faults co-occur by chance, so CFM-03 and CFM-07 are the product's main defence against alert fatigue and wrong causal stories.

## 10. Emergency response and degraded modes

The emergency path is built so that no single failure stops notification. Timings below are proposed design targets and need validation with airlines, crew and regulators.

**Reference scenario:** both pilots become incapacitated after a cockpit attack.

| Stage | AIPS action | Owner of the decision | Proposed target | Fallback if it fails |
| --- | --- | --- | --- | --- |
| 1. Detection | Combine independent indicators and compute a confidence level | AIPS flags; crew and operations confirm | Alert within 5 seconds of qualifying indicators | Manual crew report; ground operations raise it |
| 2. Activation | Start the approved emergency workflow | Operator policy | Immediate | Manual start by operations |
| 3. Cabin notification | Notify designated cabin crew on two channels | Cabin crew | Under 3 seconds | Escalate to additional crew, then to operations |
| 4. Acknowledgement | Track who acknowledged and who is available | Cabin crew | Escalate after 30 seconds without acknowledgement (configurable) | Repeat on all channels; notify operations |
| 5. ATC notification | Send emergency information through approved aircraft communication systems | Pilots, or approved automated procedure | Per approved procedure | Operations contacts ATC by phone |
| 6. Ground coordination | Notify airline operations, and emergency personnel where approved | Airline operations | Under 10 seconds from activation | Alternate contact list |
| 7. Emergency landing | Forward the event to a certified landing system if installed and its conditions are met | The certified aircraft system | Not an AIPS function | Standard emergency procedures |
| 8. Landing coordination | Share relevant information with ATC and the airport | ATC and operations | Continuous | Voice coordination |
| 9. Post-landing | Notify emergency services, preserve the full timeline | Operations and investigators | Immediate | Manual log export |

**Degraded-mode requirements**

| ID | Requirement | Priority |
| --- | --- | --- |
| EMG-01 | The emergency path shall run on its own deployment and data store, separate from the multi-tenant analytics platform | P0 |
| EMG-02 | At least two independent notification channels shall be used per recipient (for example app push and an aircraft or airline messaging channel) | P0 |
| EMG-03 | Loss of data from the aircraft shall raise a "data lost" state with the last known values and their age, and shall not clear an active emergency | P0 |
| EMG-04 | When indicators disagree, the system shall show the disagreement and lower confidence, not hide it | P0 |
| EMG-05 | The emergency timeline shall be written to tamper-evident storage with a local buffer in case the network is down | P0 |
| EMG-06 | A pilot or authorised crew member shall always be able to cancel or downgrade an alert | P0 |
| EMG-07 | Emergency drills shall be possible in a clearly marked training mode that cannot notify real recipients | P1 |

**Hazard to manage.** A false incapacitation alert could distract a working crew, and a false trigger of a landing function could be hazardous. That is why detection uses several indicators, shows its confidence, and why AIPS never commands landing.

## 11. Data, integrations and ownership

AIPS is only as good as the data it is allowed to read, so v1 uses interfaces customers already have and never needs access to avionics buses. Access to onboard data (ARINC 429 or 664) comes only through an OEM or avionics partner.

| Source | Examples | Access route | Used by | Notes |
| --- | --- | --- | --- | --- |
| Aircraft health messages | ACARS fault and report messages, central maintenance computer or condition-monitoring data | Airline or OEM service feed | Integrity, Predict, Prevent | Boeing AHM [collects from the central maintenance computer or condition-monitoring system](https://boeing.mediaroom.com/2003-05-09-Always-Fit-to-Fly); Skywise [uses ACARS from the aircraft](https://www.mroglobal-online.com/?p=45119) |
| Flight data files | QAR or FDM exports | Airline FDM programme | Flight, Integrity | Large files; batch after flight; strong privacy rules for flight data monitoring |
| Surveillance | ADS-B and similar position feeds | Public or commercial feeds | Flight | Position, speed and altitude only; no system health |
| Maintenance records | Maintenance information system, technical log, work orders, MEL | API or database export from the airline or MRO | Predict, Prevent, Autopilot, Passport | Quality varies; free text needs cleaning |
| Operational context | Weather (METAR, TAF), NOTAM, schedules, crew rostering | Public and commercial APIs | Prevent, Flight, Autopilot | Crew readiness confirmations only as sign-offs, not personal data |
| Cockpit and cabin events | Door status, alerts, crew reports, approved sensors | Aircraft integration or crew app | Guardian | Phase 6 onward; subject to approval and consultation |
| Component data | Part numbers, serials, shop-visit reports | OEM, MRO and lessor systems | Predict, Passport | Needed for fleet-wide reliability |

**Data governance requirements**

| ID | Requirement | Priority |
| --- | --- | --- |
| DAT-01 | Every record shall carry its source, owner, timestamp and ingestion time, so freshness can be computed | P0 |
| DAT-02 | The data owner (normally the airline, or the lessor or OEM for their own records) shall control who sees each record, per field | P0 |
| DAT-03 | Customers shall be able to export and delete their data at contract end | P0 |
| DAT-04 | Fleet-wide models shall train only on data the customer has agreed to share, in an anonymised form | P0 |
| DAT-05 | Data residency shall be configurable (EU, US, on-premises) | P1 |
| DAT-06 | Connectors shall be versioned and monitored, and a broken feed shall raise a data-freshness alert | P0 |

**Demonstrator data.** The first demonstrator uses synthetic data and appropriately licensed historical data only, as the source document proposes. Who can license maintenance-record datasets is open question Q5.

## 12. Technology stack and rationale

The stack below follows the source document and adds rationale and caveats. A single language is not the aim; each layer uses what suits it, and anything that could become airborne is isolated.

| Technology | Purpose | Rationale and caveat |
| --- | --- | --- |
| C++ | High-performance signal processing for flight data | Needed for speed on large time series; confine it to isolated, well-tested libraries because of memory-safety risk |
| Rust | Backend services and safety-supporting workflows | Memory safety for services that handle alerts; check toolchain qualification before any airborne use under DO-178C |
| Python | Model development and evaluation | Fastest route to research; production inference runs in a separate runtime |
| CUDA C++ | GPU acceleration | Only where profiling shows it is needed |
| ONNX Runtime | Portable model inference | Keeps models portable across cloud, on-premises and later embedded targets |
| React + TypeScript | Command dashboard | Standard for enterprise web applications |
| PostgreSQL | Operational records | Cases, work orders, permissions |
| TimescaleDB | Time-series data | Parameter streams and flight data |
| Neo4j | Aircraft and component relationships | Powers Cross-Flight Memory; compare with a graph layer on PostgreSQL in Phase 0 before committing (open question Q4) |
| Apache Kafka | Event streaming | Decouples ingestion from analytics; replayable for evaluation |
| Temporal | Durable workflows and escalation | Survives restarts, supports timers and retries for the acknowledgement-and-escalation logic |
| Docker and Kubernetes | Packaging and deployment | Supports cloud and on-premises tiers |
| OpenTelemetry | Observability | Traces and metrics for every pipeline stage |
| Keycloak | Identity and access | Single sign-on, roles per organisation |
| HashiCorp Vault | Secrets | Keys and credentials for connectors |

**Architecture constraints**

1. The emergency path (section 10) is deployed separately and uses its own Kafka topics, database and workflow workers.
2. Models are packaged with their training data version, evaluation report and approval record, following the learning-assurance idea of tracing data to model to result.
3. Nothing in the intelligence core has write access to any aircraft interface; the Phase 8 integration is a separate component with its own release process.
4. Each customer's data is isolated logically in tier A and physically in tier B.

## 13. Non-functional requirements

Targets are proposals for the demonstrator and first pilot, to be tightened with customers.

| ID | Area | Requirement | Proposed target |
| --- | --- | --- | --- |
| NFR-01 | Availability | Command and alerting services | 99.9% for tier A analytics; 99.95% for the emergency path |
| NFR-02 | Latency | Ingest-to-alert for in-flight data, once data reaches AIPS | p95 under 5 seconds |
| NFR-03 | Latency | Emergency alert to cabin crew devices | p95 under 3 seconds |
| NFR-04 | Scale | Fleet size supported per tenant | 500 aircraft in v1, designed to scale 10 times |
| NFR-05 | Security | Encryption of data in transit and at rest; keys in Vault | Mandatory |
| NFR-06 | Security | Security process aligned with DO-355 for continuing-airworthiness data, and with DO-326A principles for any onboard link | Documented threat model before Phase 1 pilot |
| NFR-07 | Access | Role-based access, least privilege, single sign-on, multi-factor sign-in | Mandatory |
| NFR-08 | Audit | Append-only, tamper-evident log of alerts, acknowledgements, overrides, model versions | Mandatory; retention set by customer policy |
| NFR-09 | Privacy | Data minimisation, purpose limits, retention limits, and a data protection impact assessment for Guardian | Before any Guardian pilot |
| NFR-10 | AI assurance | Per model: data lineage, evaluation on held-out data, performance by aircraft type, drift monitoring and a rollback plan | Required to ship any model |
| NFR-11 | Explainability | Every ML alert shall show contributing signals in plain terms an engineer can check | Mandatory |
| NFR-12 | Resilience | Loss of any single component or region shall not stop the emergency path | Tested by failure drills each quarter |
| NFR-13 | Observability | Traces, metrics and logs for every pipeline stage; data-freshness shown to users | Mandatory |
| NFR-14 | Accessibility and language | WCAG 2.2 AA for web screens; English first, with localisation support | P1 |

**AI-specific safeguards**

1. No model output is shown without its evidence grade and data age.
2. A model that falls below its agreed precision on engineer feedback is flagged and can be rolled back without a release.
3. Models never trigger external notifications to ATC or emergency services on their own; they raise alerts to people who decide, except in pre-approved emergency workflows reviewed by the customer's safety manager.

## 14. Roadmap and release criteria

The source roadmap has eight phases. This PRD adds a Phase 0 for the demonstrator and puts a gate between phases. Durations are indicative and assume a small, funded team; they should be replaced with a resourced plan.

| Phase | Scope | Exit gate | Indicative duration |
| --- | --- | --- | --- |
| 0. Foundations and demonstrator | Data pipeline on synthetic and licensed data; Cross-Flight Memory; recurring-defect detection; pre-flight report prototype; regulatory position paper; first hazard analysis | Demonstrator meets proposed recall and precision targets on replayed data; two design partners agree to a pilot | 6 to 9 months |
| 1. Integrity | System monitoring, sensor reliability, cross-system correlation | Pilot with one airline or MRO; alert precision at or above 70% | 6 months |
| 2. Predict | Remaining useful life with uncertainty, recurrence and repeat-repair detection, auto-created cases | Engineers confirm that cases are useful in a blind review | 6 months |
| 3. Prevent | Pre-flight safety assurance report and escalation | Zero use of airworthiness wording; reviewed by the pilot customer's safety manager | 4 to 6 months |
| 4. Autopilot (workflow) | Pre-flight, maintenance, in-flight and post-flight workflows | Workflow completion and override rates measured in live use | 6 months |
| 5. Flight | Trajectory and flight anomaly detection and correlation | False-alert rate accepted by operations control | 6 months |
| 6. Guardian | Incapacitation and cockpit or cabin event detection; crew notification | Privacy impact assessment, crew-representative agreement and regulator engagement complete before any live pilot | 9 to 12 months |
| 7. Command and Passport | Enterprise dashboard, shareable safety record | Multi-organisation sharing live; security assessment passed | 6 months |
| 8. Autoland integration | Forwarding to certified emergency landing systems with an OEM partner | OEM agreement, type-specific safety assessment, regulator acceptance | Partner-led; not scheduled |

**Order note.** Phase 3 (Prevent) depends on the history built in Phases 1 and 2, and Phase 7's Passport is the natural home for Cross-Flight Memory data. If the demonstrator succeeds, a minimal Passport should move forward into Phase 1 to avoid rework.

**Release criteria for every phase**

- All P0 requirements for the phase are verified, with traceability to tests.
- Claims review passed: no unsupported safety claims in product, sales or documentation.
- Security review and penetration test passed.
- Model evaluation report approved by the safety lead.
- Incident and rollback procedures rehearsed.

## 15. Risks, assumptions, dependencies and open questions

The biggest risks are access to real data, over-claiming safety value, and false-alert fatigue; the plan below addresses each.

| ID | Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- | --- |
| R1 | No access to real maintenance and flight data | High | High | Synthetic data generator in Phase 0; seek design-partner data agreements early; use licensed datasets |
| R2 | Product is read as a dispatch or airworthiness decision | Medium | Very high | Principle P1, requirement PRV-05, claims review before each release |
| R3 | Alert fatigue from false correlations | High | High | Evidence grades, engineer feedback, precision targets (CFM-03, CFM-07) |
| R4 | OEM platforms extend into the same space | Medium | High | Treat OEMs as partners; lead with a neutral multi-OEM record |
| R5 | Guardian raises privacy and labour objections | High | High | Observable-event design (P5, P6), DPIA, crew-representative consultation before any pilot |
| R6 | Regulatory guidance on AI shifts (EASA Issue 03, EU AI Act timing) | Medium | Medium | Track at every phase gate; keep learning-assurance evidence from day one |
| R7 | Emergency path fails under a cloud outage | Low | Very high | Separate deployment, two channels, quarterly failure drills (EMG-01, EMG-02, NFR-12) |
| R8 | Scope too wide for one team | High | High | Phase gates; Phase 8 is partner-led and not scheduled |
| R9 | Public accident claims damage credibility | Medium | High | No accident-specific claims (section 2); use accidents only to justify requirements |

**Assumptions**

- Airlines, MROs or lessors will share data under agreements that allow model training on anonymised data.
- Required feeds (ACARS, maintenance system exports, FDM files) are available through the customer.
- OEM and avionics partners will engage on Phase 8 only after v1 proves value.

**Dependencies**

- At least two design partners with real operational data for Phase 1.
- Aviation safety and certification advisers from Phase 0.
- Legal advice on the EU AI Act, GDPR and aviation rules.

**Open questions**

| # | Question | Owner |
| --- | --- | --- |
| Q1 | Is "AIPS" clear for trademark and domain use? The acronym is already used in other fields | Founders, counsel |
| Q2 | Which customer segment first: airline, MRO or lessor? | Product owner |
| Q3 | Rename "AIPS Autopilot" to "AIPS Workflow" to avoid confusion with the flight autopilot? | Product owner |
| Q4 | Neo4j or a graph layer on PostgreSQL for Cross-Flight Memory? | Engineering lead, decided in Phase 0 |
| Q5 | Which licensed historical maintenance and flight datasets can the demonstrator use? | Data lead |
| Q6 | What is the bottom-up serviceable market (aircraft count times price per aircraft) per segment? | Product owner |
| Q7 | Will the AI171 final report change any requirement in sections 8 and 10? | Safety lead, review on publication |

## 16. Sources

Findings come from web searches run on 29 October 2026; the pages were read as search extracts, and not every page was opened in full. Re-verify figures before using them externally.

- [Business Today, AI-171 fuel switch statement (30 Jul 2026)](https://www.businesstoday.in/amp/latest/corporate/story/air-india-ai-171-crash-no-abnormality-found-in-fuel-switch-says-aviation-ministry-546138-2026-07-30)
- [CBS News, AI171 preliminary report](https://www.cbsnews.com/amp/news/air-india-plane-fuel-switches-cut-off-crash-report)
- [Outlook Business, AI-171 switch tests](https://www.outlookbusiness.com/news/air-india-ai-171-crash-fuel-control-switch-tests-find-no-defects-centre-informs-parliament)
- [Free Press Journal, Germanwings and cockpit rules (1 Oct 2026)](https://freepressjournal.in/world/egyptair-and-germanwings-crashes-exposed-cockpit-risk-prompting-renewed-two-person-flight-deck-rules)
- [EASA, AI Concept Paper Issue 2](https://www.easa.europa.eu/en/node/139506)
- [EASA at ICAO APAC DGCA 61 (Sep 2026), AI approach and proposed Issue 03](https://www.icao.int/sites/default/files/APAC/Meetings/2026/2026%20DGCA61/Agenda%20Item03-Aviation%20Safety/61-DP-03-06_EASA_EASA-APPROACH-TO-ARTIFICIAL-INTELLIGENCE-IN-AVIATION.pdf)
- [EASA MLEAP final report (May 2024)](https://easa.europa.eu/nl/downloads/139926/en)
- [SAE ARP4754B](https://saemobilus.sae.org/content/arp4754b)
- [AFuzion, DO-326A / ED-202A overview](https://afuzion.com/?p=3204) and [DO-355 context](https://afuzion.com/?p=29078)
- [Spicy Advisory, Digital Omnibus on AI](https://spicyadvisory.com/blog/digital-omnibus-ai-act-2026-what-changed)
- [Garmin Autoland King Air 350 certification (Aug 2025)](https://seekingalpha.com/pr/20212732-garmin-autoland-autothrottle-now-certified-for-retrofit-installations-in-select) and [General Aviation News](https://generalaviationnews.com/2020/05/19/garmin-autoland-certified-for-general-aviation-aircraft/)
- [Aviation Week, Airbus DragonFly and ATTOL](https://ngtest.aviationweek.com/aerospace/emerging-technologies/airbus-adds-autonomous-emergency-diversion-capability-flight-tests)
- [Boeing Airplane Health Management](https://services.boeing.com/maintenance-engineering/maintenance-optimization/airplane-health-management-ahm) and [Boeing 2003 AHM release](https://boeing.mediaroom.com/2003-05-09-Always-Fit-to-Fly)
- [Journal de l'Aviation, Skywise](https://www.journal-aviation.com/en/news/42304-mroen1-airbus-s-skywise-platform-now-extends-to-operational-fleet-improvement) and [MRO Global, Skywise Health Monitoring](https://www.mroglobal-online.com/?p=45119)
- [IMD, Skywise and Prognos](https://imd.org/research-knowledge/operations/articles/how-industry-4-0-is-transforming-the-aircraft-industry)
- Market sizing: [Global Market Insights](https://www.gminsights.com/industry-analysis/aircraft-predictive-maintenance-market), [TrendX](https://trendxinsights.com/syndicated-market-research-reports/predictive-mro-market/), [MarketsandMarkets](https://marketsandmarkets.com/ResearchInsight/aircraft-health-monitoring-systems-market.asp), [Straits Research](https://straitsresearch.com/report/Aircraft-Maintenance-Repair-and-Overhaul-Market)
