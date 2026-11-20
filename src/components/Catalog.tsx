import { useEffect, useMemo, useRef, useState } from 'react';

import { useStore } from '../store';
import type { Camera, Catalog, Credentials, FirmwareEntry, Login, LoginEntry, Source } from '../types';

// Factory logins and firmware pages (catalog.rs, rows in meerkat-drivers'
// logins.rs and firmware.rs): a section of each in the detail panel for the
// selected camera, and the whole of both in the reference dialog.
//
// Nothing here talks to a camera. "Use this login" only fills the login
// fields: Meerkat sends a login when the user presses Apply or Read, never
// on its own, because some cameras lock an address out after wrong passwords.

function loginText(l: Login): string {
  if (l.kind === 'create') return l.username ? `none: you set ${l.username}'s password at the first login` : 'none: you create the account at the first login';
  const pw = l.password === '' ? '(blank)' : l.password;
  return l.username === '' ? `password ${pw}, no user name` : `${l.username} / ${pw}`;
}

const KIND: Record<Source['kind'], string | null> = { vendor: null, vendorExcerpt: 'excerpt only', thirdParty: 'not the maker' };

function Sources({ sources }: { sources: Source[] }) {
  const doubtful = sources.some((s) => s.kind !== 'vendor');
  return (
    <details className="sources">
      <summary>
        {sources.length === 1 ? 'Source' : `Sources (${sources.length})`}
        {doubtful && <span className="badge warn">not all from the maker's own text</span>}
      </summary>
      <ul className="plain">
        {sources.map((s) => (
          <li key={s.url + s.title}>
            <a href={s.url} target="_blank" rel="noreferrer">
              {s.title}
            </a>
            {KIND[s.kind] && <span className="badge warn">{KIND[s.kind]}</span>}
          </li>
        ))}
      </ul>
    </details>
  );
}

function LoginCard({ row, onUse }: { row: LoginEntry; onUse?: (c: Credentials) => void }) {
  const [used, setUsed] = useState(false);
  const l = row.login;
  return (
    <div className="cat-card">
      {row.firmware && <div className="cat-cond">{row.firmware}</div>}
      <div className="cat-login">
        <span className="mono">{loginText(l)}</span>
        {onUse && l.kind === 'factory' && (
          <button
            type="button"
            className="link"
            onClick={() => {
              onUse({ username: l.username, password: l.password });
              setUsed(true);
            }}
          >
            {used ? 'Filled in' : 'Use this login'}
          </button>
        )}
      </div>
      {row.changeAtFirstLogin && <p className="muted small">The camera makes you replace it at the first login.</p>}
      {row.lockout && <p className="note warn small">Wrong passwords: {row.lockout}</p>}
      {row.notes.length > 0 && (
        <ul className="cat-notes">
          {row.notes.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
      <Sources sources={row.sources} />
    </div>
  );
}

function FirmwareCard({ row, cam }: { row: FirmwareEntry; cam?: Camera }) {
  const { openWeb } = useStore();
  return (
    <div className="cat-card">
      <div className="row wrap">
        <a className="sw-btn" href={row.downloads} target="_blank" rel="noreferrer">
          {row.brand}'s download page
        </a>
        {cam && row.webPath && (
          <button type="button" disabled={!cam.webUrl} onClick={() => void openWeb(cam.id, row.webPath!)} title={`Opens ${row.webPath} on the camera`}>
            Open its firmware page
          </button>
        )}
      </div>
      {row.downloadsNote && <p className="muted small">{row.downloadsNote}</p>}
      <ol className="cat-steps">
        {row.how.map((h) => (
          <li key={h}>{h}</li>
        ))}
      </ol>
      {row.warnings.length > 0 && (
        <ul className="note warn cat-warnings">
          {row.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      {row.upload && (
        <details className="sources">
          <summary>Published upload (Meerkat does not use it)</summary>
          <p className="small">{row.upload}</p>
        </details>
      )}
      <Sources sources={row.sources} />
    </div>
  );
}

function rows<T extends { id: string }>(table: T[], ids: string[]): T[] {
  return ids.map((id) => table.find((r) => r.id === id)).filter((r): r is T => !!r);
}

/** Why the brand's rows are offered rather than one: "…. Sony's logins:" */
function hintIntro(cam: Camera, brand: string, what: string): string {
  return cam.model ? `Meerkat has no row for the ${cam.model}. ${brand}'s ${what} for other models:` : `The camera did not say its model. ${brand}'s ${what}:`;
}

function brandOf(cam: Camera, catalog: Catalog): string {
  const id = cam.catalog.logins.ids[0] ?? cam.catalog.firmware.ids[0];
  return [...catalog.logins, ...catalog.firmware].find((r) => r.id === id)?.brand ?? cam.vendor;
}

/** The detail panel's Factory login section. `onUse` fills the login fields, when there are any. */
export function CameraLogins({ cam, onUse }: { cam: Camera; onUse?: (c: Credentials) => void }) {
  const { catalog, openReference } = useStore();
  const m = cam.catalog.logins;
  const found = rows(catalog.logins, m.ids);
  const brand = brandOf(cam, catalog);
  const summary = !found.length
    ? 'not on record'
    : !m.exact
      ? 'depends on the model'
      : found.length > 1
        ? 'depends on its firmware'
        : found[0].login.kind === 'create'
          ? 'none, set at first login'
          : loginText(found[0].login);
  return (
    <details className="cat-section">
      <summary>
        <span>Factory login</span>
        <span className={found.length && m.exact ? 'mono' : 'muted'}>{summary}</span>
      </summary>
      <p className="muted small">Meerkat never tries a login by itself: some cameras block an address after a few wrong passwords.</p>
      {!found.length ? (
        <p className="small">Meerkat has no factory login on record for {cam.vendor} cameras.</p>
      ) : m.exact ? (
        found.map((r) => <LoginCard key={r.id} row={r} onUse={onUse} />)
      ) : (
        <>
          <p className="small">{hintIntro(cam, brand, 'logins')}</p>
          <ul className="cat-hints">
            {found.map((r) => (
              <li key={r.id}>
                <span>{r.family}</span>
                <span className="mono">{loginText(r.login)}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      <button type="button" className="link" onClick={() => openReference({ tab: 'logins', brand: found.length ? brand : null })}>
        {found.length ? `Every ${brand} login, with sources…` : 'Every factory login Meerkat knows…'}
      </button>
    </details>
  );
}

/** The detail panel's Firmware section. `creds`: the login typed in the detail panel, to start the install dialog with. */
export function CameraFirmware({ cam, creds }: { cam: Camera; creds: Credentials | null }) {
  const { catalog, openReference, openInstall } = useStore();
  const m = cam.catalog.firmware;
  const found = rows(catalog.firmware, m.ids);
  const brand = brandOf(cam, catalog);
  const summary = !found.length ? 'no download page on record' : m.exact ? `${brand}'s page` : 'pick the model';
  return (
    <details className="cat-section">
      <summary>
        <span>Firmware update</span>
        <span className="muted">{summary}</span>
      </summary>
      <p className="muted small">
        {cam.firmware ? `This camera reports ${cam.firmware}. ` : ''}
        {cam.firmwareVia
          ? "Meerkat does not supply firmware: get it from the maker. This camera answered ONVIF, so Meerkat can send it a file you downloaded, if it says it takes one."
          : "Meerkat does not supply firmware: get it from the maker, and follow the maker's steps."}
      </p>
      {cam.firmwareVia && (
        <button
          type="button"
          disabled={cam.reach.kind === 'mismatch'}
          title={cam.reach.kind === 'mismatch' ? 'A firmware file goes over HTTP, which cannot reach a camera on another subnet' : undefined}
          onClick={() => openInstall({ id: cam.id, creds })}
        >
          Install a firmware file…
        </button>
      )}
      {!found.length ? (
        <p className="small">Meerkat has no firmware page on record for {cam.vendor} cameras.</p>
      ) : m.exact ? (
        found.map((r) => <FirmwareCard key={r.id} row={r} cam={cam} />)
      ) : (
        <>
          <p className="small">{hintIntro(cam, brand, 'download pages')}</p>
          <ul className="cat-hints">
            {found.map((r) => (
              <li key={r.id}>
                <a href={r.downloads} target="_blank" rel="noreferrer">
                  {r.family}
                </a>
              </li>
            ))}
          </ul>
        </>
      )}
      {found.length > 0 && (
        <button type="button" className="link" onClick={() => openReference({ tab: 'firmware', brand })}>
          Every {brand} page, with the steps…
        </button>
      )}
    </details>
  );
}

/** Both tables, by brand, with a search. */
export function ReferenceDialog() {
  const { catalog, reference, openReference } = useStore();
  const ref = useRef<HTMLDialogElement>(null);
  const [query, setQuery] = useState('');

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (reference && !el.open) {
      setQuery('');
      el.showModal();
    } else if (!reference && el.open) el.close();
  }, [reference]);

  const brands = useMemo(
    () => [...new Set([...catalog.logins, ...catalog.firmware].map((r) => r.brand))].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' })),
    [catalog],
  );
  const tab = reference?.tab ?? 'logins';
  const brand = reference?.brand ?? null;
  const q = query.trim().toLowerCase().replace(/[^a-z0-9]/g, '');
  const match = (r: { brand: string; family: string }) =>
    (!brand || r.brand === brand) && (!q || `${r.brand}${r.family}`.toLowerCase().replace(/[^a-z0-9]/g, '').includes(q));
  const table: (LoginEntry | FirmwareEntry)[] = tab === 'logins' ? catalog.logins.filter(match) : catalog.firmware.filter(match);
  const byBrand = brands.map((b) => [b, table.filter((r) => r.brand === b)] as const).filter(([, list]) => list.length > 0);

  return (
    <dialog ref={ref} className="reference" onClose={() => openReference(null)} aria-label="Factory logins and firmware">
      {reference && (
        <>
          <header className="row">
            <h2>{tab === 'logins' ? 'Factory logins' : 'Firmware downloads'}</h2>
            <span className="spacer" />
            <button type="button" className="sw-quiet close" aria-label="Close" onClick={() => openReference(null)}>
              ×
            </button>
          </header>
          <div className="row wrap ref-controls">
            <div className="sw-seg">
              <label>
                <input type="radio" name="ref-tab" checked={tab === 'logins'} onChange={() => openReference({ tab: 'logins', brand })} />
                <span>Logins</span>
              </label>
              <label>
                <input type="radio" name="ref-tab" checked={tab === 'firmware'} onChange={() => openReference({ tab: 'firmware', brand })} />
                <span>Firmware</span>
              </label>
            </div>
            <select value={brand ?? ''} onChange={(e) => openReference({ tab, brand: e.target.value || null })} aria-label="Brand">
              <option value="">Every brand</option>
              {brands.map((b) => (
                <option key={b} value={b}>
                  {b}
                </option>
              ))}
            </select>
            <input type="search" placeholder="Model…" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Model" />
          </div>
          <p className="muted small">
            {tab === 'logins'
              ? "From each maker's own documents, with the place each was read. Meerkat never tries these on a camera."
              : "Links to each maker's own page. Meerkat does not supply or download firmware; it can send a file you downloaded to a camera that takes one over ONVIF."}
          </p>
          <div className="ref-list">
            {byBrand.length === 0 && <p className="muted">Nothing matches.</p>}
            {byBrand.map(([b, list]) => (
              <section key={b}>
                <h3>{b}</h3>
                {list.map((r) => (
                  <div key={r.id} className="ref-row">
                    <div className="ref-family">{r.family}</div>
                    {tab === 'logins' ? <LoginCard row={r as LoginEntry} /> : <FirmwareCard row={r as FirmwareEntry} />}
                  </div>
                ))}
              </section>
            ))}
          </div>
        </>
      )}
    </dialog>
  );
}
