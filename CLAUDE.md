# CLAUDE.md — Meerkat

Command reference. For the model, the invariants and the traps, read [AGENTS.md](AGENTS.md)
first. To add a brand, read [docs/drivers.md](docs/drivers.md).

## Commands

```bash
npm install
npm run app            # tauri dev: Vite on :5175 + cargo, opens the window
npm run app:demo       # the same against the simulator (MEERKAT_DEMO=1)
npm run app:build      # release bundle for this platform (src-tauri/target/release/bundle)
npm run dev            # the UI alone in a browser, on the mock backend (src/lib/mock.ts)
npm test               # vitest: src/lib/net.ts
npm run typecheck      # tsc -b
cd src-tauri && cargo test --workspace     # drivers, engine, simulator e2e, Tauri command layer
cd src-tauri && cargo clippy --workspace --all-targets
cd src-tauri && cargo run -p meerkat-cli -- scan --demo
cd src-tauri && cargo run -p meerkat-cli -- install 192.168.10.30 --user admin --pass admin --demo   # ONVIF firmware check
```

## Rules

- **Never write to a camera you were not asked to touch.** Discovery is read-only and safe on a
  real network. `set`, Apply and a firmware install are not. Test writes against the simulator
  (`--demo`).
- Every datagram a driver sends is addressed with `ctx.to()` / `op.to()`, and every port comes
  from `ctx.port()`. That is what keeps demo mode off the network (AGENTS.md §4).
- The UI fixtures in `src/lib/fixtures/` are generated, not hand-written:
  from `src-tauri`, `cargo run -q -p meerkat-cli -- scan --demo --json > ../src/lib/fixtures/cameras.json`,
  and likewise `ifaces --demo --json`, `drivers --json` and `catalog --json`.
- The desktop shell cannot be screen-driven here. Verify the command layer with the mock-runtime
  test in `src-tauri/src/lib.rs`, and the UI in a browser against the mock backend.

## Release

Released through the fleet's process (stoatworks-backend `release/RELEASE-CHECKLIST.md`).
Meerkat is in the website's `projects.json`, sync-about's TARGETS, sync-theme's table and
`attributions/names.json`.

- Bump the version in `package.json`, `src-tauri/tauri.conf.json` and every
  `src-tauri/**/Cargo.toml` together, then `cargo update -w` so `Cargo.lock` follows, and
  grep for the old version before tagging.
- Push a `v*` tag: `.github/workflows/desktop.yml` builds the universal macOS app and dmg, the
  Linux deb and rpm and the Windows installer, and publishes the release with tauri-action.
- tauri-action rewrites the release body, so write the notes with `gh release edit
  --notes-file` after every job has finished.
- The autosign agent signs and notarises the macOS assets. Then, from the backend:
  `check-notarised.py --repo meerkat`, `gen-downloads.py --repo meerkat` (README block and
  the site's `downloads.json`; set `version` in `projects.json` by hand) and
  `gen-catalog-data.py --repo meerkat`. Refresh the Homebrew tap once the site is live.
- `docs/USER-GUIDE.md` is the only copy of the guide anyone edits. The PDF and the site page
  are generated from it by the website's `scripts/build_guides.py meerkat`.
