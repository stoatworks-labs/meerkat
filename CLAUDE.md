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
```

## Rules

- **Never write to a camera you were not asked to touch.** Discovery is read-only and safe on a
  real network. `set` and Apply are not. Test writes against the simulator (`--demo`).
- Every datagram a driver sends is addressed with `ctx.to()` / `op.to()`, and every port comes
  from `ctx.port()`. That is what keeps demo mode off the network (AGENTS.md §4).
- The UI fixtures in `src/lib/fixtures/` are generated, not hand-written:
  from `src-tauri`, `cargo run -q -p meerkat-cli -- scan --demo --json > ../src/lib/fixtures/cameras.json`,
  and likewise `ifaces --demo --json` and `drivers --json`.
- The desktop shell cannot be screen-driven here. Verify the command layer with the mock-runtime
  test in `src-tauri/src/lib.rs`, and the UI in a browser against the mock backend.

## Release

No release, CI or website entry yet. Bump the version in `package.json`,
`src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` together.
