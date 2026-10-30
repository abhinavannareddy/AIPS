import { age, num, utc } from '../format';
import { go } from '../hooks';
import type { AircraftView } from '../types';
import { OverallPill } from './ui';

const STATUS_TEXT: Record<AircraftView['status'], string> = {
  nominal: 'Nominal', review: 'Review', attention: 'Attention', insufficient_data: 'Insufficient data',
};

/** One aircraft as an ATC-style flight progress strip: status band, registration, next leg, health. */
export function Strip({ a }: { a: AircraftView }) {
  const d = a.next_departure;
  return (
    <button className={`strip strip-${a.status}`} onClick={() => go(`aircraft/${a.tail}`)} aria-label={`${a.tail}, ${STATUS_TEXT[a.status]}`}>
      <span className="strip-band" aria-hidden />
      <span className="strip-tail">
        <strong>{a.tail}</strong>
        <small>{a.type_code} · {a.base}</small>
      </span>
      <span className="strip-leg">
        {d ? (<><strong>{d.number}</strong><small>{d.origin}–{d.dest} {utc(d.std, false)}</small></>) : <small>No departure planned</small>}
      </span>
      <span className="strip-health">
        <strong className="tabular">{a.status === 'insufficient_data' ? '—' : a.health_score}</strong>
        <small>{STATUS_TEXT[a.status]}</small>
      </span>
      <span className="strip-counts">
        <span title="Warnings" className={a.warnings ? 'c-warn' : 'c-zero'}>{a.warnings} W</span>
        <span title="Cautions" className={a.cautions ? 'c-caut' : 'c-zero'}>{a.cautions} C</span>
        <span title="Open investigations" className={a.open_cases ? 'c-case' : 'c-zero'}>{a.open_cases} inv</span>
      </span>
      <span className="strip-pf"><OverallPill o={a.preflight} /></span>
      <span className="strip-data">
        <small>Data age</small>
        <strong className={a.data_age_seconds > 43200 ? 'stale' : ''}>{age(a.data_age_seconds)}</strong>
        {a.rul_min_p10 !== null && a.rul_min_p10 < 800 && <small className="c-caut">RUL p10 {num(a.rul_min_p10)} cyc</small>}
      </span>
    </button>
  );
}
