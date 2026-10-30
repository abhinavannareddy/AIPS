import type { ReactNode } from 'react';
import { download } from '../api';
import { useAction } from '../hooks';
import { gradeLabel, severityLabel, statusLabel, overallLabel } from '../format';
import type { Grade, ItemStatus, Overall, Severity } from '../types';

export function Sev({ s }: { s: Severity }) {
  return <span className={`sev sev-${s}`}><i aria-hidden />{severityLabel[s]}</span>;
}

/** Evidence grade: hollow ring = machine proposal, filled = corroborated, check = human-confirmed. */
export function GradeChip({ g, title }: { g: Grade; title?: string }) {
  const glyph = g === 'engineering_confirmed' ? '✓' : g === 'corroborated' ? '●' : g === 'rejected' ? '✕' : '○';
  return <span className={`grade grade-${g}`} title={title ?? 'Evidence grade'}><b aria-hidden>{glyph}</b>{gradeLabel[g]}</span>;
}

export function StatusPill({ s }: { s: ItemStatus }) {
  return <span className={`status status-${s}`}>{statusLabel[s]}</span>;
}

export function OverallPill({ o }: { o: Overall | null }) {
  if (!o) return <span className="status status-missing">No report</span>;
  return <span className={`overall overall-${o}`}>{overallLabel[o]}</span>;
}

export function Panel({ title, aside, children, className = '' }: { title?: ReactNode; aside?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`panel ${className}`}>
      {(title || aside) && <header className="panel-head">{title && <h2>{title}</h2>}{aside && <div className="panel-aside">{aside}</div>}</header>}
      {children}
    </section>
  );
}

export function Loading({ label = 'Loading' }: { label?: string }) {
  return <div className="state state-loading" role="status"><span className="spinner" aria-hidden />{label}…</div>;
}

export function ErrorBox({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="state state-error" role="alert">
      <strong>Could not load this view.</strong> {message}
      {onRetry && <button className="btn btn-quiet" onClick={onRetry}>Try again</button>}
    </div>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="state state-empty">{children}</div>;
}

export function Spark({ values, lo, hi, width = 120, height = 28, warn = false }: { values: number[]; lo?: number; hi?: number; width?: number; height?: number; warn?: boolean }) {
  if (values.length < 2) return <svg width={width} height={height} aria-hidden />;
  const min = Math.min(...values, lo ?? Infinity);
  const max = Math.max(...values, hi ?? -Infinity);
  const span = max - min || 1;
  const pts = values.map((v, i) => `${(i / (values.length - 1)) * width},${height - 2 - ((v - min) / span) * (height - 4)}`).join(' ');
  return (
    <svg width={width} height={height} className={`spark ${warn ? 'spark-warn' : ''}`} aria-hidden>
      <polyline points={pts} fill="none" />
    </svg>
  );
}

export function Kv({ k, v }: { k: ReactNode; v: ReactNode }) {
  return <div className="kv"><dt>{k}</dt><dd>{v}</dd></div>;
}

export function Notice({ tone = 'info', children }: { tone?: 'info' | 'caution' | 'safety'; children: ReactNode }) {
  return <div className={`notice notice-${tone}`}>{children}</div>;
}

/** CSV download button that shows a failure instead of swallowing it. */
export function DownloadButton({ path, filename }: { path: string; filename: string }) {
  const act = useAction();
  return (
    <span className="download">
      <button className="btn btn-quiet btn-small" onClick={() => { void act.run(() => download(path, filename)); }} disabled={act.pending}>Download CSV</button>
      {act.error && <span className="download-error" role="alert">{act.error}</span>}
    </span>
  );
}
