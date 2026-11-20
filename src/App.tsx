import { useEffect, useState } from 'react';

import { BulkDialog } from './components/BulkDialog';
import { CameraTable, useVisibleCameras } from './components/CameraTable';
import { Detail } from './components/Detail';
import { LogPanel } from './components/LogPanel';
import { Sidebar } from './components/Sidebar';
import { Spinner } from './components/ui';
import { inTauri } from './lib/ipc';
import { useStore } from './store';

function Mark() {
  return (
    <span className="sw-mark" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round">
        <circle cx="12" cy="11" r="5.2" fill="currentColor" stroke="none" />
        <path d="M6.5 12v5.5h11V12" />
        <path d="M5 21h14" />
        <path d="M16.2 3.6a7 7 0 0 1 3 3.4" />
      </svg>
    </span>
  );
}

export function App() {
  const { init, ready, info, scan, stop, scanning, summary, error, dismissError, cameras, checked, filter, setFilter, toggleLog, showLog, logs, clearTable, selected } =
    useStore();
  const [bulk, setBulk] = useState(false);
  const visible = useVisibleCameras();
  const total = Object.keys(cameras).length;
  const warnings = logs.filter((l) => l.level === 'warn' || l.level === 'error').length;

  useEffect(() => {
    void init();
  }, [init]);

  return (
    <div className={`app ${selected ? 'with-detail' : ''}`}>
      <header className="sw-bar">
        <span className="sw-brand">
          <Mark />
          Meerkat
        </span>
        <span className="sw-sub">PTZ camera discovery and addressing</span>
        {info?.demo && (
          <span className="demo-pill" title="Scanning the built-in simulator, not the network">
            {inTauri ? 'Demo' : 'Browser preview'}
          </span>
        )}
        <span className="spacer" />
        {scanning ? (
          <button type="button" onClick={() => void stop()}>
            <Spinner /> Stop
          </button>
        ) : (
          <button type="button" className="sw-primary" onClick={() => void scan()} disabled={!ready}>
            Scan
          </button>
        )}
        <button type="button" data-stoatworks-about>
          About
        </button>
      </header>

      {error && (
        <div className="banner" role="alert">
          <span>{error}</span>
          <button type="button" className="sw-quiet" onClick={dismissError} aria-label="Dismiss">
            ×
          </button>
        </div>
      )}

      <div className="body">
        <Sidebar />
        <main className="main">
          <div className="toolbar">
            <input type="search" placeholder="Filter by brand, model, name, address, MAC…" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Filter" />
            <button type="button" disabled={checked.length < 2} onClick={() => setBulk(true)} title="Give the ticked cameras consecutive addresses">
              Address {checked.length >= 2 ? checked.length : ''} in sequence…
            </button>
            <button type="button" disabled={scanning || total === 0} onClick={() => void clearTable()}>
              Clear
            </button>
          </div>
          <CameraTable />
          <footer className="status">
            <span>
              {total === 0
                ? 'No cameras'
                : visible.length === total
                  ? `${total} camera${total === 1 ? '' : 's'}`
                  : `${visible.length} of ${total} cameras`}
              {summary && !scanning && ` · last scan ${(summary.elapsedMs / 1000).toFixed(1)} s${summary.new ? `, ${summary.new} new` : ''}${summary.hostsSwept ? `, ${summary.hostsSwept} addresses swept` : ''}${summary.cancelled ? ' (stopped)' : ''}`}
            </span>
            <span className="spacer" />
            <button type="button" className="sw-quiet small" onClick={toggleLog} aria-expanded={showLog}>
              {showLog ? 'Hide log' : 'Log'}
              {warnings > 0 && <span className="count">{warnings}</span>}
            </button>
          </footer>
          <LogPanel />
        </main>
        <Detail />
      </div>
      <BulkDialog open={bulk} onClose={() => setBulk(false)} />
    </div>
  );
}
