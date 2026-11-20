import { useStore, usable } from '../store';

export function Sidebar() {
  const { ifaces, drivers, scope, setScope, info, setDemo, scanning } = useStore();
  const allIfaces = scope.ifaces === null;
  const chosenIfaces = scope.ifaces ?? ifaces.filter(usable).map((i) => i.name);
  const chosenDrivers = scope.drivers ?? drivers.filter((d) => d.enabledByDefault).map((d) => d.id);

  const toggleIface = (name: string) => {
    const next = chosenIfaces.includes(name) ? chosenIfaces.filter((n) => n !== name) : [...chosenIfaces, name];
    setScope({ ifaces: next });
  };
  const toggleDriver = (id: string) => {
    const next = chosenDrivers.includes(id) ? chosenDrivers.filter((n) => n !== id) : [...chosenDrivers, id];
    setScope({ drivers: next });
  };

  return (
    <aside className="sidebar">
      <section>
        <header className="section-head">
          <h2>Interfaces</h2>
          <label className="inline">
            <input type="checkbox" checked={allIfaces} onChange={(e) => setScope({ ifaces: e.target.checked ? null : chosenIfaces })} />
            all
          </label>
        </header>
        <p className="hint">Broadcast discovery goes out of each one, and finds cameras on the wrong subnet too.</p>
        <ul className="checks">
          {ifaces.map((i) => {
            const why = i.loopback ? 'loopback' : i.p2p ? 'VPN / tunnel' : i.linkLocal ? 'link-local' : '';
            const on = allIfaces ? usable(i) : chosenIfaces.includes(i.name);
            return (
              <li key={`${i.name}-${i.ip}`}>
                <label className={why ? 'dim' : ''} title={why ? `${i.name} is ${why}: not scanned unless you tick it` : undefined}>
                  <input type="checkbox" checked={on} disabled={allIfaces} onChange={() => toggleIface(i.name)} />
                  <span className="mono">{i.name}</span>
                  <span className="mono muted">
                    {i.ip}/{i.prefix}
                  </span>
                </label>
              </li>
            );
          })}
          {ifaces.length === 0 && <li className="muted">No IPv4 interfaces.</li>}
        </ul>
      </section>

      <section>
        <h2>Routed subnets</h2>
        <p className="hint">Swept address by address, for cameras behind a router. One per line.</p>
        <textarea
          className="mono"
          rows={3}
          spellCheck={false}
          placeholder={'10.20.0.0/24\n10.30.1.10-60'}
          value={scope.ranges}
          onChange={(e) => setScope({ ranges: e.target.value })}
        />
      </section>

      <section>
        <h2>Brands</h2>
        <ul className="checks">
          {drivers.map((d) => (
            <li key={d.id}>
              <label title={[...d.discovery, ...d.notes].join('\n\n')}>
                <input type="checkbox" checked={chosenDrivers.includes(d.id)} onChange={() => toggleDriver(d.id)} />
                <span>{d.id === 'onvif' ? 'ONVIF' : d.id === 'ndi' ? 'NDI' : d.vendor}</span>
                <span className="muted small">{d.protocol}</span>
              </label>
            </li>
          ))}
        </ul>
        {scope.drivers !== null && (
          <button type="button" className="link" onClick={() => setScope({ drivers: null })}>
            Back to the defaults
          </button>
        )}
      </section>

      <section>
        <h2>Listen for</h2>
        <div className="sw-seg">
          {[1.5, 2.5, 5, 10].map((s) => (
            <label key={s}>
              <input type="radio" name="listen" checked={scope.listenSecs === s} onChange={() => setScope({ listenSecs: s })} />
              <span>{s} s</span>
            </label>
          ))}
        </div>
      </section>

      <section className="demo">
        <label className="inline" title="Eight pretend cameras answering every protocol on loopback. Nothing is sent to the network.">
          <input type="checkbox" checked={!!info?.demo} disabled={scanning || info?.platform === 'browser'} onChange={(e) => void setDemo(e.target.checked)} />
          Demo cameras
        </label>
        <p className="hint">The built-in simulator instead of the network.</p>
      </section>
    </aside>
  );
}
