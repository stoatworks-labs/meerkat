import { Fragment, useState } from 'react';

import { inSubnet, parseIp, parsePrefix, prefixToMask, problems, suggest } from '../lib/net';
import { useStore } from '../store';
import type { Camera, Credentials, NetConfig } from '../types';
import { ReachIt } from './ReachIt';
import { label, reachNote, ReachBadge, sightings, Spinner, title } from './ui';

export function Detail() {
  const { cameras, selected } = useStore();
  const c = selected ? cameras[selected] : null;
  if (!c) return null;
  // Keyed by camera, so switching rows starts a fresh form.
  return <DetailFor key={c.id} cam={c} />;
}

const FIELD_NAMES: Record<string, string> = {
  api: 'API',
  power: 'Power',
  onvifXaddr: 'ONVIF service',
  ndiName: 'NDI name',
  ndiPort: 'NDI port',
  mdnsHost: 'mDNS host',
  easyIpMode: 'Easy IP mode',
  networkVersion: 'Network firmware',
  viscaModel: 'VISCA model code',
  gatewayField: 'Gateway',
};

function DetailFor({ cam }: { cam: Camera }) {
  const { drivers, ifaces, cameras, jobs, select, apply, readNetwork, openWeb, forget, openRecovery } = useStore();
  const driver = drivers.find((d) => d.id === cam.driver);
  // The vendor's own tool for a camera that cannot be changed from here.
  const rescue = drivers.find((d) => d.recovery && d.vendor === cam.vendor);
  const job = jobs[cam.id];
  const busy = job?.state === 'sending' || job?.state === 'waiting';
  const net = cam.net;

  const [dhcp, setDhcp] = useState(!!net?.dhcp);
  const [ip, setIp] = useState(net && !net.dhcp ? net.ip : cam.ip);
  const [mask, setMask] = useState(String(net?.prefix ?? 24));
  const [gateway, setGateway] = useState(net?.gateway ?? '');
  const [dns, setDns] = useState((net?.dns ?? []).join(', '));
  const [hostname, setHostname] = useState(net?.hostname ?? (cam.driver === 'sony' ? (cam.name ?? '') : ''));
  const [user, setUser] = useState('');
  const [pass, setPass] = useState('');
  const [confirming, setConfirming] = useState(false);

  const prefix = parsePrefix(mask);
  const dnsList = dns
    .split(/[\s,;]+/)
    .map((d) => d.trim())
    .filter(Boolean);
  const errs = dhcp ? [] : [...problems(ip, prefix, gateway), ...dnsList.filter((d) => parseIp(d) === null).map((d) => `DNS ${d} is not an IPv4 address`)];
  const creds: Credentials | null = user ? { username: user, password: pass } : null;
  const needsCreds = cam.caps.needsAuth;
  const optionalCreds = !needsCreds && cam.driver === 'panasonic';
  const canWrite = cam.caps.writeNetwork && !(cam.reach.kind === 'mismatch' && !cam.caps.writeByMac);

  const cfg = (): NetConfig => ({
    dhcp,
    ip: dhcp ? '0.0.0.0' : ip.trim(),
    prefix: prefix ?? 24,
    gateway: dhcp || !gateway.trim() ? null : gateway.trim(),
    dns: dhcp ? [] : dnsList,
    hostname: cam.caps.hostname && hostname.trim() ? hostname.trim() : null,
  });

  // A wrong-subnet camera heard on an interface: offer that interface's subnet.
  const heardOn = cam.reach.kind === 'mismatch' ? ifaces.find((i) => i.name === (cam.reach as { iface: string }).iface) : undefined;
  const moveHere = () => {
    if (!heardOn) return;
    const taken = new Set([...Object.values(cameras).map((c) => c.ip), ...ifaces.map((i) => i.ip)]);
    const host = parseIp(cam.ip);
    const want = host === null ? null : host & 255;
    const s = suggest(heardOn.ip, heardOn.prefix, want, taken);
    if (s) {
      setDhcp(false);
      setIp(s);
      setMask(String(heardOn.prefix));
      if (gateway && !inSubnet(gateway, s, heardOn.prefix)) setGateway('');
    }
  };

  const from = net ? (net.dhcp ? 'DHCP' : `${net.ip}/${net.prefix}${net.gateway ? ` via ${net.gateway}` : ''}`) : cam.ip;
  const to = dhcp ? 'DHCP' : `${ip}/${prefix ?? '?'}${gateway.trim() ? ` via ${gateway.trim()}` : ''}`;
  const note = reachNote(cam);
  const extra = Object.entries(cam.fields).filter(([k]) => k !== 'onvifXaddr' || cam.driver === 'onvif');

  return (
    <aside className="detail" aria-label="Camera details">
      <header className="detail-head">
        <div>
          <h2>{label(cam)}</h2>
          <div className="muted">{title(cam)}</div>
        </div>
        <button type="button" className="sw-quiet close" aria-label="Close" onClick={() => select(null)}>
          ×
        </button>
      </header>

      <div className="actions">
        <button type="button" className="sw-primary" disabled={!cam.webUrl} onClick={() => void openWeb(cam.id)}>
          Open web UI
        </button>
        <button type="button" onClick={() => void forget(cam.id)} title="Drop it from the list until the next scan finds it">
          Forget
        </button>
      </div>

      {note && <p className={`note ${cam.reach.kind === 'mismatch' ? 'warn' : ''}`}>{note}</p>}
      {cam.reach.kind === 'mismatch' && <ReachIt cam={cam} />}

      <dl className="facts">
        <dt>Address</dt>
        <dd className="mono">
          {cam.ip}
          {net && `/${net.prefix}`} <ReachBadge reach={cam.reach} />
        </dd>
        {net?.gateway && (
          <>
            <dt>Gateway</dt>
            <dd className="mono">{net.gateway}</dd>
          </>
        )}
        {net && net.dns.length > 0 && (
          <>
            <dt>DNS</dt>
            <dd className="mono">{net.dns.join(', ')}</dd>
          </>
        )}
        <dt>MAC</dt>
        <dd className="mono">{cam.mac ?? '—'}</dd>
        {cam.firmware && (
          <>
            <dt>Firmware</dt>
            <dd>{cam.firmware}</dd>
          </>
        )}
        {cam.serial && (
          <>
            <dt>Serial</dt>
            <dd className="mono">{cam.serial}</dd>
          </>
        )}
        <dt>Found by</dt>
        <dd>
          <ul className="plain">
            {sightings(cam, drivers).map((s) => (
              <li key={s}>{s}</li>
            ))}
          </ul>
        </dd>
        {driver?.defaultLogin && (
          <>
            <dt>Factory login</dt>
            <dd className="mono">
              {driver.defaultLogin.username} / {driver.defaultLogin.password}
            </dd>
          </>
        )}
        {extra.map(([k, v]) => (
          <Fragment key={k}>
            <dt>{FIELD_NAMES[k] ?? k}</dt>
            <dd className={k.endsWith('Xaddr') ? 'mono small' : undefined}>{v}</dd>
          </Fragment>
        ))}
      </dl>

      <section className="netform">
        <h3>Network settings</h3>
        {!cam.caps.writeNetwork ? (
          <>
            <p className="muted">
              {cam.driver === 'ndi'
                ? cam.reach.kind === 'mismatch'
                  ? 'Heard over NDI only. Once this computer can reach it, its own driver may be able to change it; otherwise set its address in its web UI.'
                  : 'Heard over NDI only, and no driver here can change it. Set its address in its web UI.'
                : `The ${driver?.vendor ?? cam.driver} driver cannot change this camera's settings${cam.fields.api ? ` (${cam.fields.api})` : ''} — use its web UI.`}
            </p>
            {rescue?.recovery && (
              <button type="button" className="link" onClick={() => openRecovery(rescue.id)}>
                Lost at an address nothing reaches? Reset its network with {rescue.recovery.tool}…
              </button>
            )}
          </>
        ) : !canWrite ? (
          <p className="muted">
            This driver works over HTTP, which cannot reach a camera on another subnet: add a temporary address above, or put this computer on {cam.ip}'s
            subnet.
          </p>
        ) : (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              if (errs.length === 0 && !busy) setConfirming(true);
            }}
          >
            {cam.caps.dhcp && (
              <div className="sw-seg mode">
                <label>
                  <input type="radio" name="mode" checked={!dhcp} onChange={() => setDhcp(false)} />
                  <span>Static</span>
                </label>
                <label>
                  <input type="radio" name="mode" checked={dhcp} onChange={() => setDhcp(true)} />
                  <span>DHCP</span>
                </label>
              </div>
            )}
            {!dhcp && (
              <div className="grid">
                <label>
                  Address
                  <input className="mono" value={ip} onChange={(e) => setIp(e.target.value)} spellCheck={false} />
                </label>
                <label>
                  Mask
                  <input className="mono" value={mask} onChange={(e) => setMask(e.target.value)} spellCheck={false} title={prefix !== null ? prefixToMask(prefix) : undefined} />
                </label>
                <label>
                  Gateway
                  <input className="mono" value={gateway} onChange={(e) => setGateway(e.target.value)} placeholder="none" spellCheck={false} />
                </label>
                {cam.caps.dns && (
                  <label>
                    DNS
                    <input className="mono" value={dns} onChange={(e) => setDns(e.target.value)} placeholder="none" spellCheck={false} />
                  </label>
                )}
              </div>
            )}
            {cam.caps.hostname && (
              <label className="wide">
                {cam.driver === 'sony' ? 'Name (8 letters, digits or spaces)' : 'Host name'}
                <input value={hostname} onChange={(e) => setHostname(e.target.value)} spellCheck={false} />
              </label>
            )}
            {heardOn && !dhcp && (
              <button type="button" className="link" onClick={moveHere}>
                Pick a free address on {heardOn.name} ({heardOn.ip}/{heardOn.prefix})
              </button>
            )}

            {(needsCreds || optionalCreds) && (
              <fieldset className="creds">
                <legend>{needsCreds ? 'Login' : 'Login (only if the camera has authentication on)'}</legend>
                <div className="grid">
                  <label>
                    User
                    <input value={user} onChange={(e) => setUser(e.target.value)} autoComplete="off" spellCheck={false} />
                  </label>
                  <label>
                    Password
                    <input type="password" value={pass} onChange={(e) => setPass(e.target.value)} autoComplete="off" />
                  </label>
                </div>
                {driver?.defaultLogin && !user && (
                  <button
                    type="button"
                    className="link"
                    onClick={() => {
                      setUser(driver.defaultLogin!.username);
                      setPass(driver.defaultLogin!.password);
                    }}
                  >
                    Use the factory login
                  </button>
                )}
                {needsCreds && cam.caps.readNetwork && (
                  <button
                    type="button"
                    className="link"
                    disabled={!creds || busy}
                    onClick={async () => {
                      const got = await readNetwork(cam.id, creds);
                      if (got) {
                        setDhcp(got.dhcp);
                        setIp(got.ip);
                        setMask(String(got.prefix));
                        setGateway(got.gateway ?? '');
                        setDns(got.dns.join(', '));
                        if (got.hostname) setHostname(got.hostname);
                      }
                    }}
                  >
                    Read its current settings
                  </button>
                )}
              </fieldset>
            )}

            {errs.length > 0 && (
              <ul className="errs">
                {errs.map((e) => (
                  <li key={e}>{e}</li>
                ))}
              </ul>
            )}

            {!confirming ? (
              <button type="submit" className="sw-primary" disabled={errs.length > 0 || busy || (needsCreds && !creds)}>
                Apply…
              </button>
            ) : (
              <div className="confirm" role="alertdialog" aria-label="Confirm the change">
                <p>
                  Change <b>{label(cam)}</b> from <span className="mono">{from}</span> to <span className="mono">{to}</span>?
                  {cam.caps.writeByMac ? ' It is sent by MAC address, so only this camera takes it.' : ''} The camera restarts its network to apply it.
                </p>
                <div className="row">
                  <button type="button" onClick={() => setConfirming(false)}>
                    Cancel
                  </button>
                  <button
                    type="button"
                    className="sw-primary"
                    onClick={async () => {
                      setConfirming(false);
                      await apply(cam.id, cfg(), creds);
                    }}
                  >
                    Change it
                  </button>
                </div>
              </div>
            )}
          </form>
        )}
        {job && (
          <p className={`job ${job.state}`} role="status">
            {busy && <Spinner />} {job.text}
          </p>
        )}
      </section>
    </aside>
  );
}
