// API contract types (mirrors the Rust domain model; snake_case enums as serialised).
export type Severity = 'info' | 'advisory' | 'caution' | 'warning';
export type Grade = 'rejected' | 'suspected' | 'corroborated' | 'engineering_confirmed';
export type SystemKey =
  | 'engine' | 'electrical' | 'hydraulic' | 'fuel' | 'landing_gear'
  | 'flight_controls' | 'cabin_pressure' | 'avionics' | 'other';
export type ItemStatus = 'complete' | 'requires_approval' | 'missing' | 'inconsistent' | 'outstanding' | 'insufficient_data';
export type Overall = 'insufficient_data' | 'review_required' | 'no_findings_in_available_data';
export type CaseStatus = 'open' | 'assigned' | 'in_progress' | 'escalated' | 'closed';
export type Role = 'engineer' | 'mcc_controller' | 'flight_ops' | 'safety_manager' | 'cabin_crew' | 'lessor' | 'admin';

export interface Principal { subject: string; name: string; roles: Role[]; org?: string | null; auth_mode: string }

export interface Clock { sim_now: string; data_end: string; speed: number; running: boolean }

export interface Departure { flight_id: string; tail: string; number: string; origin: string; dest: string; std: string }

export interface AircraftView {
  tail: string; type_code: string; operator: string; lessor: string | null; base: string; msn: number;
  health_score: number; status: 'nominal' | 'review' | 'attention' | 'insufficient_data';
  open_alerts: number; warnings: number; cautions: number; open_cases: number; hypotheses: number;
  next_departure: Departure | null; preflight: Overall | null; data_age_seconds: number; cycles: number;
  rul_min_p10: number | null; last_flight: string | null;
}

export interface SourceRef { source: string; data_age_seconds: number; record_ids: string[] }
export interface Alert {
  id: string; tail: string; module: string; system: SystemKey; severity: Severity; title: string; reason: string;
  sources: SourceRef[]; model_version: string; evidence_grade: Grade; hypothesis_id: string | null; created_at: string;
  feedback: { useful: boolean; comment: string; by: string; at: string } | null;
  acknowledged?: { by: string; at: string } | null;
  escalated_at?: string | null;
}

export interface Explanation {
  matching_fields: string[]; occurrences: number; first_seen: string; last_seen: string; max_gap_hours: number;
  median_gap_minutes: number | null; lift: number | null; expected_by_chance: number | null;
  symptom_similarity: number | null; fixes_between: number; narrative: string;
}
export interface Hypothesis {
  id: string; tail: string; kind: 'recurrence' | 'repeat_after_fix' | 'cross_system'; systems: SystemKey[]; codes: string[];
  event_ids: string[]; work_order_ids: string[]; machine_grade: Grade; grade: Grade; graded_by: string | null;
  graded_at: string | null; explanation: Explanation; title: string; model_version: string;
}
export interface FaultEvent {
  id: string; tail: string; flight_id: string | null; ts: string; ata: number; system: SystemKey; code: string;
  component_id: string | null; severity: Severity; message: string; source: string; ingested_at: string;
}
export interface WorkOrder {
  id: string; tail: string; event_ids: string[]; component_id: string | null; ata: number; action: string;
  opened: string; closed: string | null; signed_off: boolean; team: string; note: string;
}
export interface HypothesisBundle { hypothesis: Hypothesis; events: FaultEvent[]; work_orders: WorkOrder[] }

export interface CaseAction { at: string; actor: string; action: string; note: string }
export interface Case {
  id: string; tail: string; title: string; kind: string; system: SystemKey; priority: Severity; status: CaseStatus;
  team: string; assignee: string | null; opened_at: string; sla_due: string; hypothesis_ids: string[]; event_ids: string[];
  escalation_level: number; history: CaseAction[];
}

export interface Finding {
  category: string; status: ItemStatus; severity: Severity; title: string; detail: string;
  evidence_grade: Grade | null; record_ids: string[];
}
export interface CategorySummary { category: string; label: string; status: ItemStatus; findings: Finding[] }
export interface Freshness { source: string; last_received: string; age_seconds: number; stale: boolean }
export interface PreflightReport {
  id: string; flight_id: string; tail: string; type_code: string; number: string; origin: string; dest: string; std: string;
  generated_at: string; generation_ms: number; overall: Overall; counts: Record<string, number>;
  categories: CategorySummary[]; ranked: Finding[]; freshness: Freshness[]; disclaimer: string;
  acknowledged_by: string | null; acknowledged_at: string | null;
  escalated_at?: string | null;
  context?: { departure_weather: Weather | null; arrival_weather: Weather | null; notams: Notam[]; source: string };
}
export interface PreflightRow {
  id: string; flight_id: string; tail: string; type_code: string; number: string; origin: string; dest: string; std: string;
  overall: Overall; counts: Record<string, number>; acknowledged_by: string | null; generation_ms: number; top: string | null;
  categories: { category: string; label: string; status: ItemStatus }[];
}

export interface RulEstimate {
  tail: string; component_id: string; position: string; p10: number; p50: number; p90: number;
  margin_c: number; slope_per_100cyc: number; cycles_since_sv: number; model_version: string;
}
export interface Component {
  id: string; tail: string; part_number: string; serial: string; ata: number; position: string; description: string;
  installed_at: string; cycles_since_sv: number;
}
export interface ParamSummary {
  param: string; label: string; unit: string; lo: number; hi: number; system: SystemKey; last: number | null; spark: number[];
}
export interface MelItem { id: string; tail: string; reference: string; title: string; category: string; ata: number; opened: string; due: string }

export interface AircraftDetail {
  aircraft: { tail: string; type_code: string; msn: number; operator: string; lessor: string | null; base: string; delivered: string; data_feed_last: string };
  view: AircraftView;
  systems: { system: SystemKey; label: string; alerts: number; worst: Severity | null }[];
  params: ParamSummary[];
  components: Component[];
  rul: RulEstimate[];
  alerts: Alert[];
  hypotheses: Hypothesis[];
  cases: Case[];
  mel: MelItem[];
  preflight: PreflightReport | null;
}

export interface TimelineItem { kind: string; ts: string; title: string; detail: string; ref: string; severity?: Severity; grade?: Grade }

export interface GraphNode { id: string; kind: string; label: string; detail: Record<string, unknown> }
export interface GraphEdge { from: string; to: string; kind: string }
export interface GraphView { source: string; nodes: GraphNode[]; edges: GraphEdge[] }

export interface Kpi { key: string; label: string; value: number | null; unit: string; target: string; met: boolean | null; note: string }
export interface ScenarioResult { id: string; kind: string; tail: string; description: string; detected: boolean; detected_by: string[] }
export interface MetricsResponse {
  metrics: {
    recall: number; hypothesis_precision: number; alert_precision: number; scenarios: ScenarioResult[];
    hypotheses_total: number; hypotheses_matched: number; alerts_total: number; alerts_matched: number;
    feedback_count: number; feedback_precision: number | null; kpis: Kpi[];
  };
  recompute_ms: number;
  dataset: { aircraft: number; flights: number; events: number; work_orders: number; samples: number; seed: number };
  models: { name: string; version: string | null; kind: string; card?: Record<string, unknown> | null }[];
  drift?: Drift;
}

export interface Overview {
  clock: Clock; live: boolean;
  fleet: { total: number; nominal: number; review: number; attention: number; insufficient_data: number };
  alerts: { total: number; warning: number; caution: number; advisory: number; by_system: Record<string, number> };
  cases: { open: number; escalated: number };
  preflight: { needing_review: number; upcoming: { flight_id: string; tail: string; number: string; origin: string; dest: string; std: string; overall: Overall; counts: Record<string, number>; acknowledged_by: string | null }[] };
  top_alerts: Alert[];
  recompute_ms: number;
  metrics: { recall: number; precision: number };
}

export interface AuditEntry { seq: number; at: string; actor: string; action: string; subject: string; detail: unknown; prev_hash: string; hash: string }

// ---- Guardian (aips-emergency) ----
export type IncidentState = 'monitoring' | 'notifying' | 'acknowledged' | 'escalated' | 'cancelled' | 'resolved';
export interface Indicator { class: string; name: string; source: string; value: string; weight: number; at: string; contradicts?: boolean }
export interface CrewMember { id: string; name: string; position: string; designated: boolean; notified: boolean; acknowledged_at: string | null; available: boolean | null }
export interface Delivery { channel: string; recipient: string; sent_at: string; due_at: string; status: 'pending' | 'delivered' | 'failed' | 'not_transmitted'; delivered_at: string | null }
export interface Step { stage: number; name: string; status: 'pending' | 'done' | 'skipped' | 'not_available'; at: string | null; detail: string }
export interface Incident {
  id: string; training: boolean; tail: string; type_code: string; flight: string; scenario: string; created_by: string; created_at: string;
  state: IncidentState; confidence: number; independent_classes: number; qualified: boolean; indicators: Indicator[];
  crew: CrewMember[]; deliveries: Delivery[]; steps: Step[]; escalation_level: number; ack_deadline: string | null;
  feed_lost: boolean; last_data_at: string; failed_channel: string | null; closed_by: string | null; closed_reason: string | null;
  journal: { entries: AuditEntry[] };
  destination?: string; disagreement?: string | null;
  procedure?: { title: string; steps: string[]; source: string; training_example: boolean } | null;
  atc_message?: { text: string; destination: string; destination_metar: string | null; alternates: { icao: string; name: string; metar: string }[]; status: string; composed_at: string } | null;
}

export interface Weather {
  airport: string; observed: string; wind_dir: number; wind_kt: number; gust_kt: number | null; visibility_m: number;
  ceiling_ft: number | null; convective: boolean; metar: string;
}
export interface Notam { id: string; airport: string; text: string; from: string; to: string }
export interface FlightEvent {
  id: string; flight_id: string; tail: string; number: string; origin: string; dest: string; kind: string; severity: Severity;
  at: string; observed: string; threshold: string; source: string; data_age_seconds: number; weather: Weather | null;
  weather_note: string | null; correlated_events: string[]; correlation_note: string | null; evidence_grade: Grade;
  model_version: string; limitation: string;
}
export interface TrackPoint { t_min: number; alt_ft: number; speed_kt: number; vs_fpm: number }
export interface FlightView {
  flight: { id: string; tail: string; number: string; origin: string; dest: string; dep: string; arr: string; cycle: number };
  summary: Record<string, unknown> | null;
  track: TrackPoint[] | null;
  weather: { departure: Weather; arrival: Weather };
  notams: { departure: Notam[]; arrival: Notam[] };
  flight_events: FlightEvent[];
  data_quality: { source: string; samples: number; max_gap_s: number; received_at: string; delay_minutes: number; complete: boolean } | null;
  post_flight: {
    new_defects: { event: FaultEvent; prior_occurrences_on_tail: number; assessment: string }[];
    corrective_actions: WorkOrder[]; flight_events: number; status: string; generated_at: string;
  };
}
export interface RelRow {
  key: string | number; corrective_actions: number; repeat_within_30d: number; effectiveness: number; open: number;
  closed_without_signoff: number; median_days_to_close: number | null; mean_days_to_close: number;
}
export interface Reliability { definition: string; limitation: string; by_ata: RelRow[]; by_team: RelRow[] }
export interface FleetPatterns {
  k_anonymity: number; suppressed_groups: number; note: string;
  patterns: { type_code: string; kind: string; systems: string; aircraft: number; patterns: number; engineering_confirmed: number }[];
}
export interface Drift { model_version: string; rows: number; method: string; features: { feature: string; psi: number; status: string }[] }
export interface ModelRegistry {
  active: string | null; active_path: string | null; drift: Drift;
  registry: { version: string; active: boolean; evaluation: Record<string, unknown> | null; window_cycles: number | null; approval: unknown }[];
}
export interface Correction {
  id: string; version: number; record_type: string; record_id: string; field: string; old_value: string; new_value: string;
  reason: string; by: string; at: string;
}
export interface RecordHistory { record_type: string; record_id: string; original: Record<string, unknown>; corrections: Correction[]; current_version: number }
export interface ErasureRequest {
  id: string; tail: string; reason: string; requested_by: string; requested_at: string; approved_by: string | null;
  executed_at: string | null; rows_erased: number | null;
}
