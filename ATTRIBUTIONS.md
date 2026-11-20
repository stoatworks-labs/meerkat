# Attributions

Meerkat is built on other people's work. This file lists what that work is, who did
it, and what it is doing here.

It is generated — the master lists live in the `stoatworks-backend` repo and are
pushed out by `scripts/sync-attributions.py`. Edit it there, not here.

## Third-party code this project uses

Libraries, SDKs and frameworks the project is built on or bundles.

### Tauri

<https://tauri.app>  
Licence: MIT or Apache-2.0  
Copyright: The Tauri Programme within The Commons Conservancy

A Cargo and npm dependency — of the app itself under src-tauri/, or of the desktop launcher under launcher/src-tauri/.

Wraps a web front end in a native desktop app using the platform's own webview rather than a bundled browser, so the binary stays small.

### React

<https://react.dev>  
Licence: MIT  
Copyright: Meta Platforms, Inc. and affiliates

An npm dependency.

The UI layer for the browser tools and the Electron and Tauri front ends.

### The Rust crate ecosystem

<https://crates.io>  
Licence: predominantly MIT or Apache-2.0  
Copyright: the individual crate authors

Cargo dependencies, resolved and pinned in Cargo.lock.

Async runtimes, protocol codecs, serialisation and GUI toolkits. The exact set and versions for any build are in that repo's Cargo.lock, which is the authoritative list.

### The npm ecosystem

<https://www.npmjs.com>  
Licence: predominantly MIT  
Copyright: the individual package authors

npm dependencies, resolved and pinned in the lockfile.

Build tooling, test runners and the libraries the front ends are assembled from. The exact set and versions for any build are in that repo's lockfile, which is the authoritative list.

The full transitive dependency set for any build is pinned in this repo's lockfile,
which is the authoritative list. What is named above is the layers a reader would
want to know about, not every package that has ever been resolved.

## Work we checked ourselves against

No code was taken from these — but they were how we knew we had it right, and that is worth saying out loud.

### dPro-Software's PanasonicEasyIPsetupCore, AndyJae's smart-reset-browser, and Panasonic's AW-UE100 operating instructions

Panasonic has not published Easy IP Setup. The frame format in src-tauri/crates/meerkat-drivers/src/panasonic.rs was worked out from the packet captures these two independent reimplementations publish (an AW-HE130, and an AW-UE160 checked against Panasonic's own tool), with Panasonic's operating instructions and web-control supplements. No code was taken from either.

### tt-0927/IPC_Camera and Electronics/PTZControllerUI

The ValueHD (PTZOptics) upgrade-tool discovery in src-tauri/crates/meerkat-drivers/src/vhd.rs was read from the device side of the first and the client side of the second. No code was taken, and because neither of the two set dialects in that code is verified, Meerkat only discovers these cameras.

### Bitfocus Companion's birddog-ptz module

Read alongside BirdDog's own API documents for how the cameras answer. No module code is used.

### BirdDog Neuralyzer

Meerkat links to BirdDog's own reset tool and watches for the camera afterwards. It does not speak Neuralyzer's protocol, which BirdDog has not published, and nothing of Neuralyzer is included.

## Standards and published specifications

What the implementation is measured against.

- **ONVIF Core Specification, WS-Discovery (2005/04) and the OASIS Web Services Security UsernameToken Profile 1.0** — The ONVIF driver (src-tauri/crates/meerkat-drivers/src/onvif.rs) implements discovery (§7.3), authentication (§5.9) and the network settings of the device service (§8.2) from the published specifications.
- **Multicast DNS (RFC 6762) and DNS-Based Service Discovery (RFC 6763)** — How the NDI driver (src-tauri/crates/meerkat-drivers/src/ndi.rs) hears the `_ndi._tcp` announcements NDI cameras make. It only browses them: no NDI SDK is used or shipped. NDI® is a registered trademark of Vizrt NDI AB.
- **Sony Camera IP Setting Command, in the Sony VISCA Command List E-042-100-13 v2.10** — The Sony driver (src-tauri/crates/meerkat-drivers/src/sony.rs) implements the IP setup inquiry and set from the section Sony publishes in its VISCA command lists (BRC-X400, SRG-X400 and others; the BRC-AM7 and ILME-FR7 lists carry the same section).
- **BirdDog RESTful API 2.0 and the X-series API 2.1 documentation** — The BirdDog driver (src-tauri/crates/meerkat-drivers/src/birddog.rs) identifies cameras and reads and changes their network settings with the published API.

## Getting this wrong

If your work is here and the description is inaccurate, the licence is wrong, or you would rather not be listed — open an issue and it will be fixed.
