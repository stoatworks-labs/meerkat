import { useEffect, useRef } from 'react';

import { useStore } from '../store';

export function LogPanel() {
  const { logs, showLog } = useStore();
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (showLog) end.current?.scrollIntoView({ block: 'end' });
  }, [logs, showLog]);
  if (!showLog) return null;
  return (
    <section className="log" aria-label="Log">
      {logs.length === 0 && <p className="muted">Nothing logged yet.</p>}
      {logs.map((l, i) => (
        <div key={i} className={`line ${l.level}`}>
          <span className="time">{new Date(l.t).toLocaleTimeString()}</span>
          <span className="who">{l.driver}</span>
          <span>{l.message}</span>
        </div>
      ))}
      <div ref={end} />
    </section>
  );
}
