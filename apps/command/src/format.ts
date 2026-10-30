// Formatting helpers. All aviation times are shown in UTC.
import type { Grade, ItemStatus, Overall, Severity, SystemKey } from './types';

export function utc(ts: string | null | undefined, withDate = true): string {
  if (!ts) return '—';
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return '—';
  const hm = `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}`;
  if (!withDate) return `${hm}Z`;
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  return `${String(d.getUTCDate()).padStart(2, '0')} ${months[d.getUTCMonth()]} ${hm}Z`;
}

export function age(seconds: number): string {
  if (seconds < 90) return `${Math.round(seconds)} s`;
  if (seconds < 5400) return `${Math.round(seconds / 60)} min`;
  if (seconds < 172800) return `${(seconds / 3600).toFixed(1)} h`;
  return `${Math.round(seconds / 86400)} d`;
}

export function relative(ts: string, nowIso: string): string {
  const s = (Date.parse(nowIso) - Date.parse(ts)) / 1000;
  if (Number.isNaN(s)) return '—';
  return s >= 0 ? `${age(s)} ago` : `in ${age(-s)}`;
}

export const severityLabel: Record<Severity, string> = { info: 'Info', advisory: 'Advisory', caution: 'Caution', warning: 'Warning' };
export const gradeLabel: Record<Grade, string> = {
  rejected: 'Rejected', suspected: 'Suspected', corroborated: 'Corroborated', engineering_confirmed: 'Engineering-confirmed',
};
export const statusLabel: Record<ItemStatus, string> = {
  complete: 'Complete', requires_approval: 'Requires authorised approval', missing: 'Missing information',
  inconsistent: 'Inconsistent information', outstanding: 'Outstanding issue', insufficient_data: 'Insufficient data',
};
export const overallLabel: Record<Overall, string> = {
  insufficient_data: 'Insufficient data', review_required: 'Review required', no_findings_in_available_data: 'No findings in available data',
};
export const systemLabel: Record<SystemKey, string> = {
  engine: 'Engines', electrical: 'Electrical', hydraulic: 'Hydraulics', fuel: 'Fuel', landing_gear: 'Landing gear',
  flight_controls: 'Flight controls', cabin_pressure: 'Cabin pressure', avionics: 'Avionics', other: 'Other',
};
export const caseKindLabel: Record<string, string> = {
  recurring_defect: 'Recurring defect', repeat_repair: 'Repeat repair', cross_system: 'Cross-system link',
  degradation: 'Degradation', sensor_reliability: 'Sensor reliability',
};

export function num(v: number | null | undefined, digits = 0): string {
  if (v === null || v === undefined || Number.isNaN(v)) return '—';
  return v.toLocaleString('en-GB', { maximumFractionDigits: digits, minimumFractionDigits: digits });
}
