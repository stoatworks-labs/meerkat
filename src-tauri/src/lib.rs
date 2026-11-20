//! The desktop app: the engine behind a handful of Tauri commands.
//!
//! Everything a camera needs (sockets bound to an interface, broadcasts,
//! unicast sweeps) happens here in Rust; the webview only draws. A scan
//! streams `meerkat://event` events as cameras land, so the table fills in
//! while it runs.
//!
//! Demo mode swaps the engine for one sandboxed onto the built-in simulator
//! (`meerkat-sim`): eight pretend cameras on loopback, every protocol answered,
//! nothing sent to the network. Start with `MEERKAT_DEMO=1` or `--demo`, or
//! flip it from the sidebar.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat_core::{Alias, Camera, Credentials, DriverInfo, Engine, Event, LocalIface, NetConfig, ScanRequest, ScanSummary, WriteOutcome};
use serde::Serialize;
use tauri::{AppHandle, Builder, Emitter, Manager, RunEvent, Runtime, State};
use tauri_plugin_opener::OpenerExt;

const EVENT: &str = "meerkat://event";

struct AppState {
    engine: Mutex<Arc<Engine>>,
    /// The simulator behind the engine in demo mode; dropped (and stopped)
    /// when demo mode is switched off.
    sim: Mutex<Option<meerkat_sim::Sim>>,
}

impl AppState {
    fn engine(&self) -> Arc<Engine> {
        self.engine.lock().unwrap().clone()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    demo: bool,
    platform: &'static str,
}

fn info(state: &AppState) -> AppInfo {
    AppInfo { version: env!("CARGO_PKG_VERSION"), demo: state.engine().is_sandboxed(), platform: std::env::consts::OS }
}

#[tauri::command]
fn app_info(state: State<AppState>) -> AppInfo {
    info(&state)
}

/// Switch between the real network and the simulator. Clears the table:
/// the two never mix.
#[tauri::command]
async fn set_demo(state: State<'_, AppState>, on: bool) -> Result<AppInfo, String> {
    if state.engine().is_running() {
        return Err("stop the scan first".into());
    }
    if on == state.engine().is_sandboxed() {
        return Ok(info(&state));
    }
    // The real engine is the only record of the addresses it added.
    if on && !state.engine().aliases().is_empty() {
        return Err("remove the temporary addresses first".into());
    }
    if on {
        let sim = meerkat_sim::Sim::start(meerkat_sim::demo_cameras()).await.map_err(|e| format!("could not start the simulator: {e}"))?;
        *state.engine.lock().unwrap() = Arc::new(Engine::sandboxed(meerkat_drivers::all(), sim.sandbox()));
        *state.sim.lock().unwrap() = Some(sim);
    } else {
        *state.engine.lock().unwrap() = Arc::new(Engine::new(meerkat_drivers::all()));
        *state.sim.lock().unwrap() = None;
    }
    Ok(info(&state))
}

#[tauri::command]
fn interfaces(state: State<AppState>) -> Result<Vec<LocalIface>, String> {
    state.engine().interfaces()
}

#[tauri::command]
fn drivers(state: State<AppState>) -> Vec<DriverInfo> {
    state.engine().drivers()
}

#[tauri::command]
fn cameras(state: State<AppState>) -> Vec<Camera> {
    state.engine().cameras()
}

/// Run a scan to the end. Cameras and log lines arrive as events meanwhile;
/// the summary is the return value (and the last event).
#[tauri::command]
async fn scan<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, req: ScanRequest) -> Result<ScanSummary, String> {
    let engine = state.engine();
    engine
        .scan(req, move |e: Event| {
            let _ = app.emit(EVENT, &e);
        })
        .await
}

#[tauri::command]
fn stop_scan(state: State<AppState>) {
    state.engine().cancel();
}

#[tauri::command]
fn forget(state: State<AppState>, id: String) {
    state.engine().forget(&id);
}

#[tauri::command]
fn clear(state: State<AppState>) -> Result<(), String> {
    let e = state.engine();
    if e.is_running() {
        return Err("stop the scan first".into());
    }
    e.clear();
    Ok(())
}

#[tauri::command]
async fn read_network(state: State<'_, AppState>, id: String, creds: Option<Credentials>) -> Result<NetConfig, String> {
    state.engine().read_network(&id, creds).await
}

#[tauri::command]
async fn write_network(state: State<'_, AppState>, id: String, cfg: NetConfig, creds: Option<Credentials>) -> Result<WriteOutcome, String> {
    state.engine().write_network(&id, cfg, creds).await
}

/// After a write: look for the camera at its new address (keyed by MAC, so
/// it keeps its row). `None` means it did not answer within the time.
#[tauri::command]
async fn verify<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, id: String, outcome: WriteOutcome, timeout_secs: Option<u64>) -> Result<Option<Camera>, String> {
    let engine = state.engine();
    let wait = Duration::from_millis(outcome.settle_ms) + Duration::from_secs(timeout_secs.unwrap_or(45));
    engine
        .verify(&id, &outcome, wait, move |e: Event| {
            if matches!(e, Event::Camera { .. }) {
                let _ = app.emit(EVENT, &e);
            }
        })
        .await
}

/// Open a camera's own web interface in the system browser. Returns the URL
/// opened (in demo mode, the simulator's page on loopback).
#[tauri::command]
fn open_web_ui<R: Runtime>(app: AppHandle<R>, state: State<AppState>, id: String) -> Result<String, String> {
    let engine = state.engine();
    let cam = engine.camera(&id).ok_or("that camera is no longer in the list")?;
    let url = engine.browser_url(cam.web_url.as_deref().ok_or("this camera has no web UI address")?);
    app.opener().open_url(&url, None::<&str>).map_err(|e| format!("could not open a browser: {e}"))?;
    Ok(url)
}

/// The temporary addresses Meerkat has added (see `meerkat_core::alias`).
#[tauri::command]
fn aliases(state: State<AppState>) -> Vec<Alias> {
    state.engine().aliases()
}

/// The temporary address that would put this computer on a camera's subnet.
#[tauri::command]
fn plan_alias(state: State<AppState>, id: String) -> Result<Alias, String> {
    state.engine().plan_alias(&id)
}

/// Add one. The system asks for an admin password; the cameras come back with
/// their reach worked out again.
#[tauri::command]
async fn add_alias(state: State<'_, AppState>, alias: Alias) -> Result<Vec<Camera>, String> {
    state.engine().add_alias(alias).await
}

/// Remove the ones listed, or every one.
#[tauri::command]
async fn remove_aliases(state: State<'_, AppState>, only: Option<Vec<Alias>>) -> Result<Vec<Camera>, String> {
    state.engine().remove_aliases(only.as_deref()).await
}

/// A sweep of a /22 holds a socket per probe in flight per driver; macOS
/// starts every app at 256 descriptors.
#[cfg(unix)]
fn raise_fd_limit() {
    unsafe {
        let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) == 0 {
            let want = r.rlim_max.min(8192);
            if r.rlim_cur < want {
                r.rlim_cur = want;
                let _ = libc::setrlimit(libc::RLIMIT_NOFILE, &r);
            }
        }
    }
}

#[cfg(not(unix))]
fn raise_fd_limit() {}

pub fn run() {
    raise_fd_limit();
    let demo = std::env::var("MEERKAT_DEMO").is_ok_and(|v| !v.is_empty() && v != "0") || std::env::args().any(|a| a == "--demo");
    let (engine, sim) = if demo {
        match tauri::async_runtime::block_on(meerkat_sim::Sim::start(meerkat_sim::demo_cameras())) {
            Ok(sim) => (Engine::sandboxed(meerkat_drivers::all(), sim.sandbox()), Some(sim)),
            Err(e) => {
                eprintln!("meerkat: could not start the simulator ({e}); using the network");
                (Engine::new(meerkat_drivers::all()), None)
            }
        }
    } else {
        (Engine::new(meerkat_drivers::all()), None)
    };
    app(tauri::Builder::default(), AppState { engine: Mutex::new(Arc::new(engine)), sim: Mutex::new(sim) })
        .build(tauri::generate_context!())
        .expect("error while starting Meerkat")
        .run(|app, event| {
            // Temporary addresses go when Meerkat does (the system asks for
            // the password once more). A restart would drop them anyway.
            if let RunEvent::Exit = event {
                let engine = app.state::<AppState>().engine();
                if !engine.aliases().is_empty() {
                    if let Err(e) = tauri::async_runtime::block_on(engine.remove_aliases(None)) {
                        eprintln!("meerkat: could not remove the temporary addresses ({e}); a restart removes them");
                    }
                }
            }
        });
}

/// Everything but the runtime and the context, so the tests can drive the
/// same commands on Tauri's mock runtime.
fn app<R: Runtime>(builder: Builder<R>, state: AppState) -> Builder<R> {
    builder.plugin(tauri_plugin_opener::init()).manage(state).invoke_handler(tauri::generate_handler![
        app_info,
        set_demo,
        interfaces,
        drivers,
        cameras,
        scan,
        stop_scan,
        forget,
        clear,
        read_network,
        write_network,
        verify,
        open_web_ui,
        aliases,
        plan_alias,
        add_alias,
        remove_aliases
    ])
}

/// The command layer the webview talks to, driven the way `src/lib/ipc.ts`
/// calls it (same command names, same camelCase argument names), against the
/// simulator. What this cannot cover is the webview itself.
#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::{Listener, WebviewWindow, WebviewWindowBuilder};

    use super::*;

    fn call(w: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Result<Value, Value> {
        get_ipc_response(
            w,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<Value>().unwrap())
    }

    #[test]
    fn the_ui_commands_scan_readdress_and_verify_in_demo_mode() {
        let sim = tauri::async_runtime::block_on(meerkat_sim::Sim::start(meerkat_sim::demo_cameras())).expect("simulator");
        let engine = Engine::sandboxed(meerkat_drivers::all(), sim.sandbox());
        let state = AppState { engine: Mutex::new(Arc::new(engine)), sim: Mutex::new(Some(sim)) };
        let app = app(mock_builder(), state).build(mock_context(noop_assets())).expect("app");
        let w = WebviewWindowBuilder::new(&app, "main", Default::default()).build().expect("window");

        let events = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = events.clone();
        app.listen(EVENT, move |e| seen.lock().unwrap().push(serde_json::from_str(e.payload()).unwrap()));

        let info = call(&w, "app_info", json!({})).unwrap();
        assert_eq!(info["demo"], true, "{info}");
        assert!(call(&w, "drivers", json!({})).unwrap().as_array().unwrap().len() >= 6);
        let ifaces = call(&w, "interfaces", json!({})).unwrap();
        assert_eq!(ifaces[0]["name"], "sim0", "{ifaces}");

        // The scan returns its summary; cameras stream as events meanwhile.
        let summary = call(&w, "scan", json!({ "req": { "listenMs": 700, "probeTimeoutMs": 400 } })).expect("scan");
        let cams = call(&w, "cameras", json!({})).unwrap();
        let cams = cams.as_array().unwrap();
        assert_eq!(cams.len(), 8, "{summary}\n{cams:#?}");
        let streamed = events.lock().unwrap().iter().filter(|e| e["type"] == "camera").count();
        assert!(streamed >= 8, "{streamed} camera events");

        // Move the wrong-subnet Panasonic onto sim0's subnet, as the Apply button does.
        let p = cams.iter().find(|c| c["mac"] == "00:80:45:1a:2b:01").expect("UE150");
        let id = p["id"].clone();
        let cfg = json!({ "dhcp": false, "ip": "192.168.10.21", "prefix": 24, "gateway": "192.168.10.1", "dns": [] });
        let outcome = call(&w, "write_network", json!({ "id": id, "cfg": cfg, "creds": null })).expect("write");
        assert_eq!(outcome["expectIp"], "192.168.10.21", "{outcome}");
        let back = call(&w, "verify", json!({ "id": id, "outcome": outcome, "timeoutSecs": 5 })).expect("verify");
        assert_eq!((&back["id"], &back["ip"]), (&id, &json!("192.168.10.21")), "{back}");
        assert_eq!(back["reach"]["kind"], "local", "{back}");

        // A refusal comes back as the error string the UI shows.
        let locked = cams.iter().find(|c| c["mac"] == "00:80:45:1a:2b:02").expect("UE100");
        let cfg = json!({ "dhcp": false, "ip": "192.168.10.22", "prefix": 24, "gateway": "192.168.10.1" });
        let e = call(&w, "write_network", json!({ "id": locked["id"], "cfg": cfg, "creds": null })).unwrap_err();
        assert!(e.as_str().is_some_and(|s| s.contains("20 minutes")), "{e}");

        // The BirdDog on its fallback address: plan, add and remove a
        // temporary address the way the detail panel does.
        let q = cams.iter().find(|c| c["ip"] == "192.168.100.100").expect("P400");
        assert_eq!(q["reach"]["kind"], "mismatch");
        let alias = call(&w, "plan_alias", json!({ "id": q["id"] })).expect("plan");
        assert_eq!((&alias["iface"], &alias["ip"], &alias["prefix"]), (&json!("sim0"), &json!("192.168.100.253"), &json!(24)), "{alias}");
        let now = call(&w, "add_alias", json!({ "alias": alias })).expect("add");
        let q2 = now.as_array().unwrap().iter().find(|c| c["id"] == q["id"]).unwrap();
        assert_eq!(q2["reach"]["kind"], "local", "{q2}");
        assert_eq!(call(&w, "aliases", json!({})).unwrap().as_array().unwrap().len(), 1);
        let e = call(&w, "set_demo", json!({ "on": true })).unwrap();
        assert_eq!(e["demo"], true, "already in demo mode: nothing to switch");
        let now = call(&w, "remove_aliases", json!({ "only": [alias] })).expect("remove");
        assert_eq!(now.as_array().unwrap().iter().find(|c| c["id"] == q["id"]).unwrap()["reach"]["kind"], "mismatch");
        assert_eq!(call(&w, "aliases", json!({})).unwrap(), json!([]));

        // The table can be cleared once the scan is over.
        call(&w, "clear", json!({})).unwrap();
        assert_eq!(call(&w, "cameras", json!({})).unwrap(), json!([]));
    }
}
