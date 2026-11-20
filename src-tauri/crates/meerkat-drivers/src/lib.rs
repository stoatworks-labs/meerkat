//! Meerkat's vendor drivers. One module per protocol; [`all`] is the registry
//! the app and the CLI load. To add a vendor, write a module that implements
//! [`meerkat_core::Driver`] and add it to [`all`] — see `docs/drivers.md`.

pub mod birddog;
pub mod ndi;
pub mod onvif;
pub mod panasonic;
pub mod sony;
pub mod udp;
pub mod util;
pub mod vhd;

use std::sync::Arc;

use meerkat_core::Driver;

/// Every driver, vendor protocols first.
pub fn all() -> Vec<Arc<dyn Driver>> {
    vec![
        Arc::new(panasonic::Panasonic),
        Arc::new(sony::Sony),
        Arc::new(birddog::BirdDog),
        Arc::new(vhd::Vhd),
        Arc::new(onvif::Onvif),
        Arc::new(ndi::Ndi),
    ]
}
