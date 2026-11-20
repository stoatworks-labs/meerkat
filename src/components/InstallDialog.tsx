import { useEffect, useRef, useState } from 'react';

import { api, message } from '../lib/ipc';
import { useStore } from '../store';
import type { Camera, Credentials, FirmwareCheck, InstallReport, InstallStage } from '../types';
import { label, Spinner } from './ui';

// Install a firmware file the user downloaded from the maker, over a protocol
// that publishes how (ONVIF's StartFirmwareUpgrade): ask the camera first,
// then the file, then an explicit yes for this camera and this file. The
// engine sends it, waits out the time the camera gave, finds it again and
// reads its version: only that version says the install took.

/** The largest file the app takes (MAX_FIRMWARE in driver.rs). */
const MAX_BYTES = 1 << 30;

function size(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)} kB`;
  return `${n} bytes`;
}

function secs(n: number): string {
  return n < 90 ? `${n} s` : `${Math.round(n / 60)} min`;
}

function stageText(s: InstallStage): string {
  switch (s.kind) {
    case 'preparing':
      return 'Asking the camera to get ready';
    case 'pausing':
      return `Waiting ${secs(s.secs)}, as the camera asked`;
    case 'sending':
      return `Sending ${size(s.bytes)}`;
    case 'installing':
      return `The camera has it and is installing it. It expects to be away ${secs(s.secs)}`;
    case 'looking':
      return 'Looking for it on the network';
  }
}

async function sha256(data: Uint8Array): Promise<string | null> {
  try {
    const h = await crypto.subtle.digest('SHA-256', data as Uint8Array<ArrayBuffer>);
    return [...new Uint8Array(h)].map((b) => b.toString(16).padStart(2, '0')).join('');
  } catch {
    return null;
  }
}

/** The simulator's image format (firmware_file in meerkat-sim), for the demo. */
function pretendFirmware(model: string, current: string | null): { name: string; data: Uint8Array } {
  const version = current === '2.00' ? '2.10' : '2.00';
  return { name: `${model}-${version}-demo.bin`, data: new TextEncoder().encode(`MEERKAT-SIM-FIRMWARE\nmodel=${model}\nversion=${version}\n`) };
}

interface Chosen {
  name: string;
  data: Uint8Array;
  sha: string | null;
}

export function InstallDialog() {
  const { installFor, openInstall, cameras } = useStore();
  const ref = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false);
  const cam = installFor ? cameras[installFor.id] : undefined;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (installFor && !el.open) el.showModal();
    else if (!installFor && el.open) el.close();
  }, [installFor]);

  return (
    <dialog
      ref={ref}
      className="install"
      aria-label="Install firmware"
      // Escape must not hide an install that is still running.
      onCancel={(e) => busy && e.preventDefault()}
      onClose={() => openInstall(null)}
    >
      {installFor && cam && <InstallFor key={cam.id} cam={cam} initial={installFor.creds} busy={busy} setBusy={setBusy} />}
    </dialog>
  );
}

function InstallFor({ cam, initial, busy, setBusy }: { cam: Camera; initial: Credentials | null; busy: boolean; setBusy: (b: boolean) => void }) {
  const { openInstall, catalog, info, noteInstall } = useStore();
  const [user, setUser] = useState(initial?.username ?? '');
  const [pass, setPass] = useState(initial?.password ?? '');
  const [check, setCheck] = useState<FirmwareCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [file, setFile] = useState<Chosen | null>(null);
  const [sure, setSure] = useState(false);
  const [stages, setStages] = useState<InstallStage[]>([]);
  const [report, setReport] = useState<InstallReport | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [since, setSince] = useState(0);
  const [now, setNow] = useState(0);
  const creds: Credentials | null = user ? { username: user, password: pass } : null;

  useEffect(() => {
    if (!busy) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [busy]);

  // The maker's own warnings for this model, from the catalogue.
  const rows = cam.catalog.firmware.exact ? catalog.firmware.filter((r) => cam.catalog.firmware.ids.includes(r.id)) : [];
  const maker = check?.manufacturer ?? cam.vendor;
  const model = check?.model ?? cam.model ?? 'camera';

  const ask = async () => {
    setChecking(true);
    setErr(null);
    setCheck(null);
    try {
      const c = await (await api()).firmwareCheck(cam.id, creds);
      setCheck(c);
      if (c.version) noteInstall(cam.id, 'info', `runs firmware ${c.version}`, c.version);
    } catch (e) {
      setErr(message(e));
    } finally {
      setChecking(false);
    }
  };

  const choose = async (f: File | null | undefined) => {
    setFile(null);
    setSure(false);
    setErr(null);
    if (!f) return;
    if (f.size === 0) return setErr(`${f.name} is empty`);
    if (f.size > MAX_BYTES) return setErr(`${f.name} is ${size(f.size)}; Meerkat sends files up to ${size(MAX_BYTES)}`);
    const data = new Uint8Array(await f.arrayBuffer());
    setFile({ name: f.name, data, sha: await sha256(data) });
  };

  const pretend = async () => {
    const p = pretendFirmware(model, check?.version ?? null);
    setSure(false);
    setFile({ ...p, sha: await sha256(p.data) });
  };

  const install = async () => {
    if (!file || !check?.accepts) return;
    setBusy(true);
    setErr(null);
    setReport(null);
    setStages([]);
    setSince(Date.now());
    setNow(Date.now());
    const b = await api();
    const off = await b.onInstall((e) => {
      if (e.id === cam.id) setStages((s) => [...s, e.stage]);
    });
    try {
      const held = await b.firmwareStage(file.data);
      const r = await b.firmwareInstall(cam.id, held.token, file.name, creds);
      setReport(r);
      const took = r.back && r.after !== null && r.after !== r.before;
      noteInstall(cam.id, took ? 'info' : 'warn', `${file.name}: ${outcome(r)}`, r.after);
      if (r.after) setCheck((c) => c && { ...c, version: r.after });
      // Installed: the file is what the camera runs now, and is not offered again.
      if (took) setFile(null);
    } catch (e) {
      setErr(message(e));
      noteInstall(cam.id, 'error', `${file.name}: ${message(e)}`, null);
    } finally {
      off();
      setBusy(false);
      // Another send needs another yes.
      setSure(false);
    }
  };

  const started = stages.length > 0 || busy;
  const elapsed = busy ? Math.max(0, Math.round((now - since) / 1000)) : 0;

  return (
    <>
      <header className="row">
        <h2>Install firmware on {label(cam)}</h2>
        <span className="spacer" />
        <button type="button" className="sw-quiet close" aria-label="Close" disabled={busy} onClick={() => openInstall(null)}>
          ×
        </button>
      </header>
      <p className="muted small">
        Meerkat does not supply firmware: download it from {cam.vendor}'s own page first. Then Meerkat sends the file to {cam.ip} the way ONVIF
        publishes, once the camera says it takes one.
      </p>

      <section>
        <h3>1. Ask the camera</h3>
        <div className="row wrap">
          <label>
            User
            <input value={user} onChange={(e) => setUser(e.target.value)} autoComplete="off" spellCheck={false} disabled={busy} />
          </label>
          <label>
            Password
            <input type="password" value={pass} onChange={(e) => setPass(e.target.value)} autoComplete="off" disabled={busy} />
          </label>
          <button type="button" disabled={!creds || checking || busy} onClick={() => void ask()}>
            {checking && <Spinner />} Ask it
          </button>
        </div>
        <p className="muted small">An install needs an administrator's login. Asking reads only: what it runs, and whether it takes a file.</p>
        {check && (
          <dl className="facts">
            <dt>Camera</dt>
            <dd>{[check.manufacturer, check.model].filter(Boolean).join(' ') || '(it did not say)'}</dd>
            <dt>Runs</dt>
            <dd>{check.version ?? '(it did not say)'}</dd>
            <dt>Sent by</dt>
            <dd>{check.method}</dd>
          </dl>
        )}
        {check && !check.accepts && <p className="note warn">{check.note ?? 'It does not say it takes a firmware file this way.'}</p>}
      </section>

      {check?.accepts && (
        <section>
          <h3>2. The file</h3>
          <div className="row wrap">
            <input type="file" aria-label="Firmware file" disabled={busy} onChange={(e) => void choose(e.target.files?.[0])} />
            {info?.demo && (
              <button type="button" className="link" disabled={busy} onClick={() => void pretend()}>
                Use a pretend file for this simulated camera
              </button>
            )}
          </div>
          {file && (
            <dl className="facts">
              <dt>File</dt>
              <dd>
                {file.name}, {size(file.data.length)}
              </dd>
              <dt>SHA-256</dt>
              <dd className="mono small sha">{file.sha ?? '(this browser cannot work it out)'}</dd>
            </dl>
          )}
          {file && <p className="muted small">If {cam.vendor} publishes a checksum for the download, it should match this one.</p>}
          {rows.flatMap((r) => r.warnings).length > 0 && (
            <>
              <p className="small">{cam.vendor}'s own warnings for this model:</p>
              <ul className="note warn cat-warnings">
                {rows.flatMap((r) => r.warnings).map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            </>
          )}
        </section>
      )}

      {check?.accepts && file && (
        <section className="confirm" aria-label="Confirm the install">
          <ul className="cat-notes">
            <li>Meerkat cannot undo a firmware update.</li>
            <li>Keep the camera powered and connected until it is back. A camera that loses power while it installs can need its maker's repair.</li>
            <li>Under ONVIF it keeps its address and logins (Core specification §8.3.9).</li>
          </ul>
          <label className="check">
            <input type="checkbox" checked={sure} disabled={busy} onChange={(e) => setSure(e.target.checked)} />
            <span>
              {file.name} is {maker}'s firmware for the {model}, from {maker}'s own download page.
            </span>
          </label>
          <div className="row">
            <span className="spacer" />
            <button type="button" className="sw-primary" disabled={!sure || busy} onClick={() => void install()}>
              {busy && <Spinner />} Install on {label(cam)} ({cam.ip})
            </button>
          </div>
        </section>
      )}

      {started && (
        <ol className="stages" aria-label="Progress">
          {stages.map((s, i) => {
            const current = busy && i === stages.length - 1;
            return (
              <li key={`${s.kind}${i}`} className={current ? 'current' : 'done'}>
                <span className="tick" aria-hidden="true">
                  {current ? <Spinner /> : '✓'}
                </span>
                {stageText(s)}
                {current && <span className="muted"> · {secs(elapsed)}</span>}
              </li>
            );
          })}
        </ol>
      )}

      {report && <Outcome report={report} />}
      {err && (
        <p className="job failed" role="alert">
          {err}
        </p>
      )}
    </>
  );
}

function outcome(r: InstallReport): string {
  if (!r.back) return `not back after ${secs(r.waitedSecs)}: it may still be installing. Leave it powered on, and scan again later.`;
  if (r.after === null) return 'back, but it did not say its version: check its web UI.';
  if (r.after === r.before) return `back, but still running ${r.after}: the file did not take. Check its web UI.`;
  return `back, running ${r.after} (was ${r.before ?? 'unknown'}).`;
}

function Outcome({ report }: { report: InstallReport }) {
  const took = report.back && report.after !== null && report.after !== report.before;
  const text = outcome(report);
  return (
    <div className={`note ${took ? 'ok' : 'warn'}`} role="status">
      <p>{text.charAt(0).toUpperCase() + text.slice(1)}</p>
      <p className="muted small">{report.message}.</p>
    </div>
  );
}
