# AIPS Phase 0: Preliminary hazard analysis

Status: draft for review by the safety lead. Not approved.

The PRD asks for a first hazard analysis in Phase 0, using ARP4761A as a template. This is that
first pass. It is a functional hazard assessment at the level of what AIPS shows and does, not a
full system safety assessment. AIPS is advisory and has no write access to any aircraft system, so
the hazards below are about people acting on wrong, missing or misleading information.

## Severity and likelihood scales

Severity follows the ARP4761A classes. Because AIPS never acts on the aircraft, its worst credible
contribution is to mislead a person who then acts.

| Class | Meaning here |
|---|---|
| Major | Misleads a person in a way that could lead to an unsafe decision if other barriers also fail |
| Minor | Extra workload, delay or a missed opportunity to catch something earlier |
| No safety effect | Nuisance only |

Likelihood is a qualitative estimate for Phase 0 (synthetic data, no live operation): *probable*,
*remote*, *extremely remote*. It has to be redone with operational data in Phase 1.

## Hazards

| ID | Function | Failure condition | Effect | Severity | Likelihood | Mitigation in the build | Verified by |
|---|---|---|---|---|---|---|---|
| H1 | Pre-departure report | Stale data presented as "no findings" | False reassurance before departure | Major | Remote | Stale feeds give *Insufficient data*, never a clear state; disclaimer on every report and screen | Property test `prevent_never_clears_and_flags_stale_data`; integration PRV-06 |
| H2 | Pre-departure report | Report read as a dispatch or airworthiness decision | Responsibility shifts from authorised staff to the tool | Major | Remote | No approve or deny actions; review acknowledgement is worded as a review only; claims review of all UI text | Integration (no "safe to fly" wording); UI review |
| H3 | Cross-Flight Memory | Correlation shown as root cause | Wrong corrective action, real cause left in service | Major | Remote | Machine grades stop at *Corroborated*; only engineers confirm, with a written reason; every link is explained | Property test `memory_invariants`; integration CFM-03 |
| H4 | Integrity / Predict | Missed degradation (false negative) | Opportunity to act earlier is lost | Minor | Remote | Several detectors per parameter; recall measured on labelled scenarios (19 of 19) | Detection-quality metrics |
| H5 | Integrity / Predict | Too many false alerts | Alert fatigue, real alerts ignored | Minor | Probable | Evidence grades, engineer feedback, precision tracked (87% alerts, 100% hypotheses on synthetic data), alerts per aircraft per week 0.3 | Metrics page; perf/metrics report |
| H6 | Predict (RUL) | Point estimate trusted over its uncertainty | Maintenance deferred too long | Major | Remote | Only p10/p50/p90 ranges are shown; drift monitoring; rollback to a previous model | Model tests; integration NFR-10 |
| H7 | Guardian | False incapacitation alert | Distracts a working crew | Major | Remote | Needs 90% fused confidence from at least two independent indicator classes; contradicting evidence blocks notification and is shown | Unit and integration EMG-04, GRD-01 |
| H8 | Guardian | Real emergency not notified (channel or service failure) | Delayed response | Major | Extremely remote | Two independent channels per recipient, escalation on no available acknowledgement, separate deployment and local journal | Reliability tests; integration GRD-06, GRD-07, EMG-01 |
| H9 | Guardian | Drill reaches real recipients | Real-world response to a fake event | Major | Extremely remote | Live mode refused at start-up and per request; ATC channel never transmits in training | Integration EMG-07, AUT-03 |
| H10 | Autoland boundary | Landing function triggered by AIPS | Hazardous aircraft state | Catastrophic if it existed | Not possible in this build | No command interface exists; capability registry shows *not available* | Code review; integration AUT-01 |
| H11 | Audit / Passport | Records altered without trace | Investigations rely on wrong history | Major | Extremely remote | Hash chain, append-only database triggers, versioned corrections | Property tests on the chain; reliability restore test |
| H12 | Workflow | Escalation lost on restart | Open issue not followed up | Minor | Remote | Temporal durable timers; embedded path resumes from the database | Live Temporal worker-restart test; integration after-restart |

## Assumptions behind this analysis

- Authorised people make every airworthiness, dispatch and flight-deck decision, as the PRD requires.
- Existing operator barriers (maintenance release, MEL procedures, crew procedures) stay in place.
- Data in Phase 0 is synthetic, so the likelihood column is an engineering judgement only.

## Next steps

1. Review with the safety lead and record the decision on each severity class.
2. Redo likelihoods with design-partner data in Phase 1.
3. Before any Guardian pilot: add hazards for real sensors and crew devices, and complete the DPIA
   and crew-representative consultation required by NFR-09.
