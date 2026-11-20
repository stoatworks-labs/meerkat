//! Meerkat's engine.
//!
//! The pieces, in the order to read them:
//!
//! - [`model`] — what a camera is once found: identity, address, network
//!   settings, what its driver can do with it, and where it was seen.
//! - [`driver`] — the one trait a vendor implements. Everything vendor-specific
//!   lives behind it, in `meerkat-drivers`; nothing in this crate knows a brand.
//! - [`net`] — local interfaces, address ranges, subnet maths, UDP sockets
//!   that send out of a chosen interface, and settings validation.
//! - [`http`] — a small HTTP client with Basic and Digest auth, which is what
//!   camera CGIs and REST APIs want.
//! - [`registry`] — merges the same camera found by several drivers (Easy IP
//!   and ONVIF and NDI, say) into one row.
//! - [`engine`] — runs a scan: every driver's broadcast discovery on every
//!   chosen interface, plus a unicast sweep of routed ranges, at once.
//! - [`alias`] — temporary addresses, so HTTP reaches a camera heard on the
//!   wrong subnet.
//! - [`catalog`] — what is known about a camera besides its protocol: the
//!   login it left the factory with, and where its maker publishes firmware.
//!   The shape and the matching rule; the rows are in `meerkat-drivers`.

pub mod alias;
pub mod catalog;
pub mod driver;
pub mod engine;
pub mod http;
pub mod model;
pub mod net;
pub mod registry;

pub use alias::Alias;
pub use catalog::{Catalog, CatalogMatch, FirmwareEntry, Login, LoginEntry, Matched, Source, SourceKind};
pub use driver::{Driver, DriverError, DriverInfo, OpCtx, Recovery, ScanCtx, WriteOutcome};
pub use engine::{Engine, Event, Sandbox, ScanRequest, ScanSummary};
pub use model::{Caps, Camera, Credentials, Mac, Method, NetConfig, Reach, Sighting};
pub use net::LocalIface;
