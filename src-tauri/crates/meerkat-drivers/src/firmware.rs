//! Where each family of cameras gets its firmware, and how its maker says to
//! put it on, with the place each fact was read.
//!
//! Meerkat supplies no firmware and uploads none. A row links the maker's
//! download page, repeats the maker's steps and warnings, and names the
//! camera's own firmware page where the maker shows its address.
//!
//! The shape and the matching rule are `meerkat_core::catalog`, shared with
//! [`crate::logins`]; the same source rules apply. `upload` records a
//! published network upload, for information only.

use meerkat_core::catalog::{FirmwareEntry, Source, SourceKind};

const fn src(title: &'static str, url: &'static str, kind: SourceKind) -> Source {
    Source { title, url, kind }
}

const V: SourceKind = SourceKind::Vendor;

// ── Panasonic ────────────────────────────────────────────────────────────────

const PANASONIC: &[&str] = &["panasonic"];
const PANASONIC_NOTE: Option<&str> = Some("Panasonic shows an agreement page before the download.");
const PANASONIC_HOW: &[&str] = &[
    "Extract the download. Leave the extracted folder and its files as they are, in a folder whose path has no spaces or double-byte characters.",
    "In the camera's web page: SETUP > Maintenance > Product info. Under Firmware File, Select the UPDATE.HDC file, then Execute and OK.",
    "Or use Panasonic's Easy IP Setup Tool Plus (Windows 10 and 11).",
];
const PANASONIC_WARNINGS: &[&str] = &[
    "It takes about 10 minutes. Don't power the camera down or unplug its LAN cable, and do nothing else with it until it finishes.",
    "The camera switches itself off and on part-way through. The update is still running: keep the web page open.",
    "If the power or the network was lost and it says Update failed, reconnect and run it again.",
];
const PANASONIC_SOURCES: &[Source] = &[
    src(
        "AW-UE150 Update Procedure (Sep 2022)",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/ue150/ue150_update_e.pdf",
        V,
    ),
    src(
        "AW-HE40 update procedure",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/he40/he40_update_newtek_e.pdf",
        V,
    ),
];

const fn panasonic(id: &'static str, models: &'static [&'static str], family: &'static str, downloads: &'static str) -> FirmwareEntry {
    FirmwareEntry {
        id,
        brand: "Panasonic",
        vendors: PANASONIC,
        models,
        family,
        downloads,
        downloads_note: PANASONIC_NOTE,
        how: PANASONIC_HOW,
        web_path: None,
        warnings: PANASONIC_WARNINGS,
        upload: None,
        sources: PANASONIC_SOURCES,
    }
}

// ── Sony ─────────────────────────────────────────────────────────────────────

const SONY: &[&str] = &["sony"];
const SONY_NOTE: Option<&str> = Some("Sony asks you to accept a licence before the download.");
const SONY_X_HOW: &[&str] = &[
    "In the camera's web page, Version up: under Firmware upgrade, Choose File, then follow the on-screen instructions.",
    "Sony's release note for the version comes with the download.",
];
const SONY_X_SOURCES: &[Source] = &[src(
    "SRG-X120/X400 Operating Instructions 5-011-250-12(1): Version up (Sony's document, hosted by a reseller)",
    "https://www.adorama.com/col/productManuals/SOSRGX120W.pdf",
    V,
)];
const SONY_OTHER_HOW: &[&str] = &["Follow the steps Sony publishes with the download."];
const SONY_VERSIONUP: &str = "Sony's CGI command list documents an upload to /update/versionup.cgi (application/octet-stream, as FirmwareFile), without its method or what it answers.";

const fn sony(
    id: &'static str,
    models: &'static [&'static str],
    family: &'static str,
    downloads: &'static str,
    how: &'static [&'static str],
    sources: &'static [Source],
) -> FirmwareEntry {
    FirmwareEntry {
        id,
        brand: "Sony",
        vendors: SONY,
        models,
        family,
        downloads,
        downloads_note: SONY_NOTE,
        how,
        web_path: None,
        warnings: &[],
        upload: None,
        sources,
    }
}

// ── BirdDog ──────────────────────────────────────────────────────────────────

const BIRDDOG: &[&str] = &["birddog"];
const BIRDUI_HOW: &[&str] = &[
    "In BirdUI: SYSTEM > System Update > CHOOSE FILE…, then UPDATE.",
    "Follow the Firmware Upgrade Instructions in the download folder.",
];
const BIRDDOG_P_HOW: &[&str] = &["Follow the steps on BirdDog's page and in the download."];
const BIRDUI_SOURCES: &[Source] = &[src("BirdDog BirdUI User Guide: System Update", "https://birddog.tv/wp-content/uploads/2024/04/BirdDog_BirdUI_User-Guide.pdf", V)];

const fn birddog(
    id: &'static str,
    models: &'static [&'static str],
    family: &'static str,
    downloads: &'static str,
    how: &'static [&'static str],
    sources: &'static [Source],
) -> FirmwareEntry {
    FirmwareEntry {
        id,
        brand: "BirdDog",
        vendors: BIRDDOG,
        models,
        family,
        downloads,
        downloads_note: None,
        how,
        web_path: None,
        warnings: &[],
        upload: None,
        sources,
    }
}

// ── Lumens ───────────────────────────────────────────────────────────────────

const LUMENS_HOW: &[&str] = &[
    "Log in to the camera's web page: Maintenance > Choose files… > Submit.",
    "When it is done, Lumens asks for a factory reset: Maintenance > System Service > Reset.",
];
const LUMENS_WARNINGS: &[&str] = &[
    "It takes about 2 minutes. Don't stop it: Lumens warns that can leave the unit not working.",
    "If it says the update failed, run it again.",
];
const LUMENS_SOURCES: &[Source] = &[src(
    "FW006: VC-A61P/VC-A71P FW Update Manual 1.2",
    "https://www.mylumens.com/Download/FW006%20-%20VC-A61P_VC-A71P%20FW%20Update%20Manual_1_2.pdf",
    V,
)];

pub static FIRMWARE: &[FirmwareEntry] = &[
    // ── Panasonic: one page per family ───────────────────────────────────────
    panasonic(
        "panasonic-ue150",
        &["AW-UE150", "AW-UE155", "AW-UN145"],
        "AW-UE150, AW-UE155, AW-UN145",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upgue150_e.htm",
    ),
    panasonic(
        "panasonic-he40",
        &["AW-HE35", "AW-HE38", "AW-HE40", "AW-HE48", "AW-HE58", "AW-HE65", "AW-HN40"],
        "AW-HE35, HE38, HE40, HE48, HE58, HE65, HN40",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upghe40_e.htm",
    ),
    panasonic(
        "panasonic-ue100",
        &["AW-UE100"],
        "AW-UE100",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upgue100_e.htm",
    ),
    panasonic(
        "panasonic-ue160",
        &["AW-UE160"],
        "AW-UE160",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upgue160_e.htm",
    ),
    panasonic("panasonic-ue4", &["AW-UE4"], "AW-UE4", "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upgue4_e.htm"),
    panasonic(
        "panasonic-ue20",
        &["AW-UE20", "AW-HE20"],
        "AW-UE20, AW-HE20",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upgue20_e.htm",
    ),
    panasonic(
        "panasonic-ue40",
        &["AW-UE30", "AW-UE40", "AW-UE43", "AW-UE50", "AW-UE53"],
        "AW-UE30, UE40, UE43, UE50, UE53",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upgue40_e.htm",
    ),
    panasonic(
        "panasonic-ue70",
        &["AW-UE63", "AW-UE65", "AW-UE70", "AW-UN70"],
        "AW-UE63, UE65, UE70, UN70",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upgue70_e.htm",
    ),
    panasonic("panasonic-ue80", &["AW-UE80"], "AW-UE80", "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upgue80_e.htm"),
    panasonic(
        "panasonic-he130",
        &["AW-HE130"],
        "AW-HE130",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upghe130_e.htm",
    ),
    panasonic(
        "panasonic-he145",
        &["AW-HE145"],
        "AW-HE145",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upghe145_e.htm",
    ),
    panasonic(
        "panasonic-he42",
        &["AW-HE42", "AW-HE68"],
        "AW-HE42, AW-HE68",
        "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/upghe42_e.htm",
    ),
    // ── Sony: one page per model ─────────────────────────────────────────────
    sony(
        "sony-srg-x400",
        &["SRG-X400"],
        "SRG-X400",
        "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-x400/downloads",
        SONY_X_HOW,
        SONY_X_SOURCES,
    ),
    sony(
        "sony-srg-x402",
        &["SRG-X402"],
        "SRG-X402",
        "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-x402/downloads",
        SONY_X_HOW,
        SONY_X_SOURCES,
    ),
    sony(
        "sony-srg-x120",
        &["SRG-X120"],
        "SRG-X120",
        "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-x120/downloads",
        SONY_X_HOW,
        SONY_X_SOURCES,
    ),
    sony(
        "sony-brc-x400",
        &["BRC-X400"],
        "BRC-X400",
        "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/brc-x400/downloads",
        SONY_X_HOW,
        SONY_X_SOURCES,
    ),
    sony(
        "sony-brc-x401",
        &["BRC-X401"],
        "BRC-X401",
        "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/brc-x401/downloads",
        SONY_X_HOW,
        SONY_X_SOURCES,
    ),
    sony(
        "sony-srg-a40",
        &["SRG-A40"],
        "SRG-A40",
        "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-a40/downloads",
        SONY_OTHER_HOW,
        &[src("Sony UK: SRG-A40 downloads", "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-a40/downloads", V)],
    ),
    sony(
        "sony-srg-a12",
        &["SRG-A12"],
        "SRG-A12",
        "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-a12/downloads",
        SONY_OTHER_HOW,
        &[src("Sony UK: SRG-A12 downloads", "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-a12/downloads", V)],
    ),
    sony(
        "sony-srg-300se",
        &["SRG-300SE"],
        "SRG-300SE",
        "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-300se/downloads",
        SONY_OTHER_HOW,
        &[src("Sony UK: SRG-300SE downloads", "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/srg-300se/downloads", V)],
    ),
    FirmwareEntry {
        upload: Some(SONY_VERSIONUP),
        ..sony(
            "sony-brc-am7",
            &["BRC-AM7"],
            "BRC-AM7",
            "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/brc-am7/downloads",
            SONY_OTHER_HOW,
            &[
                src("Sony UK: BRC-AM7 downloads", "https://www.sony.co.uk/electronics/support/ptz-and-remote-cameras-ptz-cameras/brc-am7/downloads", V),
                src("BRC-AM7 CGI command list (Japanese): versionup", "https://www.sony.jp/remote-camera/download/pdf/BRC-AM7/5066066011.pdf", V),
            ],
        )
    },
    FirmwareEntry {
        warnings: &[
            "It takes about 30 minutes. Take the lens off and release the pan/tilt lock first, and set SETUP switch 4 to OFF.",
            "Power it from the AC adapter or PoE++. Don't switch it off, unplug it or close the browser while it runs: the POWER lamp blinks yellow, and it restarts itself when done.",
            "The camera refuses an update it doesn't need.",
        ],
        upload: Some(
            "Sony's CGI command list documents uploads to /update/versionup.cgi (the camera, as FirmwareFile) and /update/lens_versionup.cgi (the lens, as LensFirmwareFile), without their method or what they answer.",
        ),
        ..sony(
            "sony-fr7",
            &["ILME-FR7", "FR7"],
            "ILME-FR7, FR7K",
            "https://www.sony.co.uk/electronics/support/interchangeable-lens-camcorders-ilme-series/ilme-fr7/downloads",
            &["In the Web App: Settings > Maintenance > Information > Camera > Version Up > Choose File (the .dat file)."],
            &[
                src("Sony UK: ILME-FR7 downloads and update steps", "https://www.sony.co.uk/electronics/support/interchangeable-lens-camcorders-ilme-series/ilme-fr7/downloads", V),
                src("ILME-FR7 CGI command list v4.00 (Japanese): versionup", "https://www.sony.jp/ls-camera/download/pdf/FR7/ILME-FR7_CGI_command_list_v4.00.pdf", V),
            ],
        )
    },
    // ── BirdDog: one page per model ──────────────────────────────────────────
    birddog("birddog-p100", &["P100"], "P100", "https://birddog.tv/p100-firmware-update/", BIRDDOG_P_HOW, &[src("BirdDog: P100 firmware update", "https://birddog.tv/p100-firmware-update/", V)]),
    birddog("birddog-p110", &["P110"], "P110", "https://birddog.tv/p110-firmware-update/", BIRDDOG_P_HOW, &[src("BirdDog: P110 firmware update", "https://birddog.tv/p110-firmware-update/", V)]),
    birddog("birddog-p120", &["P120"], "P120", "https://birddog.tv/p120-firmware-update/", BIRDDOG_P_HOW, &[src("BirdDog: P120 firmware update", "https://birddog.tv/p120-firmware-update/", V)]),
    birddog("birddog-p200", &["P200"], "P200", "https://birddog.tv/p200-firmware-update/", BIRDDOG_P_HOW, &[src("BirdDog: P200 firmware update", "https://birddog.tv/p200-firmware-update/", V)]),
    birddog("birddog-p240", &["P240"], "P240", "https://birddog.tv/p240-firmware-update/", BIRDDOG_P_HOW, &[src("BirdDog: P240 firmware update", "https://birddog.tv/p240-firmware-update/", V)]),
    birddog("birddog-p400", &["P400"], "P400", "https://birddog.tv/p400-firmware-update/", BIRDDOG_P_HOW, &[src("BirdDog: P400 firmware update", "https://birddog.tv/p400-firmware-update/", V)]),
    birddog("birddog-p4k", &["P4K"], "P4K", "https://birddog.tv/p4k-firmware-update/", BIRDDOG_P_HOW, &[src("BirdDog: P4K firmware update", "https://birddog.tv/p4k-firmware-update/", V)]),
    FirmwareEntry {
        warnings: &[
            "Do the steps on BirdDog's page in order: for 6.3.102 the update fails if step 1 is skipped.",
            "Once updated, the camera cannot be downgraded by hand (BirdDog's Firmware Rollback Protection).",
        ],
        sources: &[
            src("BirdDog: X1 firmware update", "https://birddog.tv/x1-firmware-update/", V),
            src("BirdDog BirdUI User Guide: System Update", "https://birddog.tv/wp-content/uploads/2024/04/BirdDog_BirdUI_User-Guide.pdf", V),
        ],
        ..birddog("birddog-x1", &["X1$"], "X1", "https://birddog.tv/x1-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES)
    },
    birddog("birddog-x1-ultra", &["X1 Ultra"], "X1 Ultra", "https://birddog.tv/x1ultra-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES),
    birddog("birddog-x4-ultra", &["X4 Ultra"], "X4 Ultra", "https://birddog.tv/x4u-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES),
    birddog("birddog-x4e-ultra", &["X4E"], "X4E Ultra", "https://birddog.tv/x4eultra-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES),
    birddog("birddog-x5-ultra", &["X5"], "X5 Ultra", "https://birddog.tv/x5ultra-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES),
    birddog("birddog-max", &["MAX"], "MAX", "https://birddog.tv/max-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES),
    birddog("birddog-maki-live", &["MAKI Live"], "MAKI Live", "https://birddog.tv/makilive-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES),
    birddog("birddog-maki-ultra-12x", &["MAKI Ultra 12"], "MAKI Ultra 12x", "https://birddog.tv/makiultra12x-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES),
    birddog("birddog-maki-ultra-20x", &["MAKI Ultra 20"], "MAKI Ultra 20x", "https://birddog.tv/makiultra20x-firmware-update/", BIRDUI_HOW, BIRDUI_SOURCES),
    // ── PTZOptics ────────────────────────────────────────────────────────────
    FirmwareEntry {
        id: "ptzoptics-g3",
        brand: "PTZOptics",
        vendors: &["ptzoptics"],
        models: &["PT*-G3"],
        family: "G3 cameras: Move 4K, Move SE, Link 4K, Studio Pro",
        downloads: "https://docs.ptzoptics.com/docs/cameras/",
        downloads_note: Some("Pick the camera, then Firmware: each model has its own page."),
        how: &[
            "In the camera's web page, System Settings: Check Firmware Version, then Apply, downloads and installs the latest.",
            "Or, from a file: open Advanced in System Settings, Select File (the .img), then Apply.",
        ],
        web_path: None,
        warnings: &[
            "The upload takes 1 to 3 minutes, then the camera says it is installing: give it another 3 to 6 minutes before reloading the page.",
        ],
        upload: None,
        sources: &[src("PTZOptics docs: Move 4K firmware instructions", "https://docs.ptzoptics.com/docs/cameras/move-4k/firmware/instructions/", V)],
    },
    FirmwareEntry {
        id: "ptzoptics-g2",
        brand: "PTZOptics",
        vendors: &["ptzoptics"],
        models: &["PT*-G2"],
        family: "G2 cameras: 12X, 20X and 30X SDI, NDI and USB, ZCAM",
        downloads: "https://docs.ptzoptics.com/docs/cameras/g2-legacy/firmware/",
        downloads_note: Some("SDI/NDI and USB cameras have separate pages."),
        how: &[
            "With PTZOptics' Upgrade Tool: v2.8 on Windows; the Mac tool, v2.7, cannot back up or restore, and on Big Sur cannot search, so type the address in.",
            "Some G2 cameras update over the network, some over USB, some both: the instructions list which.",
        ],
        web_path: None,
        warnings: &[
            "Use firmware for the right camera type: an SoC version starting with 6 is PoE, with 7 non-PoE. PTZOptics warns the wrong firmware may permanently damage the camera.",
            "There are two Upgrade Tool v2.8C builds, Old MAC (D4:E0:8E…) and New MAC (DC:ED:84…). The wrong one changes the camera's MAC address.",
            "A backup holds that camera's MAC address, serial number and focus data: never restore it to another camera.",
            "The camera may need a focus calibration afterwards. A file saved twice as \"… (1)\" must be renamed before it will load.",
        ],
        upload: None,
        sources: &[
            src("PTZOptics docs: SDI/NDI G2 firmware", "https://docs.ptzoptics.com/docs/cameras/g2-legacy/firmware/sdi-ndi-g2/", V),
            src("PTZOptics Firmware Upgrade Instructions, rev 1.4", "https://docs.ptzoptics.com/docs/cameras/g2-legacy/firmware/sdi-ndi-g2/instructions/", V),
        ],
    },
    // ── Canon ────────────────────────────────────────────────────────────────
    FirmwareEntry {
        id: "canon-cr",
        brand: "Canon",
        vendors: &["canon"],
        models: &["CR-N700", "CR-N500", "CR-N400", "CR-N350", "CR-N300", "CR-N100", "CR-X300"],
        family: "CR-N700, N500, N400, N350, N300, N100, CR-X300",
        downloads: "https://www.canon-europe.com/support/firmware/ptz/",
        downloads_note: None,
        how: &[
            "Back the settings up first: System > Maintenance > Backup/Restore, Execute, saves a DAT file.",
            "System > Maintenance > Firmware Update: choose the .bin from the extracted download, then Execute and OK.",
            "Afterwards, restore the backup on the same Backup/Restore page.",
        ],
        web_path: Some("/admin/system/firmup"),
        warnings: &["Don't switch the camera off during the update: Canon says it can cause a fault."],
        upload: None,
        sources: &[src(
            "Canon Japan: CR-N500 firmware, with the remote camera update procedure (Japanese)",
            "https://canon.jp/support/software/os/select/remotecamera/crn500-firm",
            V,
        )],
    },
    // ── Lumens ───────────────────────────────────────────────────────────────
    FirmwareEntry {
        id: "lumens-vc-a71p",
        brand: "Lumens",
        vendors: &["lumens"],
        models: &["VC-A71P"],
        family: "VC-A71P, VC-A71PN",
        downloads: "https://www.mylumens.com/en/Downloads?keyword2=VC-A71P&id=2",
        downloads_note: None,
        how: LUMENS_HOW,
        web_path: None,
        warnings: LUMENS_WARNINGS,
        upload: None,
        sources: LUMENS_SOURCES,
    },
    FirmwareEntry {
        id: "lumens-vc-a61p",
        brand: "Lumens",
        vendors: &["lumens"],
        models: &["VC-A61P"],
        family: "VC-A61P",
        downloads: "https://www.mylumens.com/en/Downloads",
        downloads_note: Some("Search for the model."),
        how: LUMENS_HOW,
        web_path: None,
        warnings: LUMENS_WARNINGS,
        upload: None,
        sources: LUMENS_SOURCES,
    },
    // ── AVer ─────────────────────────────────────────────────────────────────
    FirmwareEntry {
        id: "aver-ptz310-330",
        brand: "AVer",
        vendors: &["aver"],
        models: &["PTZ310", "PTZ330"],
        family: "PTZ310, PTZ330 (N and V2)",
        downloads: "https://www.aver.com/download-center",
        downloads_note: Some("Search for the model."),
        how: &["In the camera's web page: System > Upgrade firmware, Browse to the file, then Upgrade. Refresh the browser afterwards."],
        web_path: None,
        warnings: &["The network connection drops during the update, and the camera restarts itself when it is done."],
        upload: Some(
            "AVer's PTZ310V2 manual gives an HTTP upload: curl -X POST --user NAME:PASSWORD -F file1=@FILE http://CAMERA/system/ (multipart). What it answers is not documented.",
        ),
        sources: &[src(
            "PTZ310V2 Series User Manual (2026.5): Upgrade Firmware, and the CGI list, p. 55",
            "https://averusa.com/pro-av/downloads/user-manual/PTZ310V2+Series_UM_EN_2026.5.pdf",
            V,
        )],
    },
    // ── Marshall ─────────────────────────────────────────────────────────────
    FirmwareEntry {
        id: "marshall-cv605",
        brand: "Marshall",
        vendors: &["marshall"],
        models: &["CV605"],
        family: "CV605",
        downloads: "https://marshall-usa.com/discontinued/cameras/CV605-BK-WH.php",
        downloads_note: None,
        how: &[
            "In the camera's web page: Configurations > Update > Choose File, the ARM file (CV605_…_ARM_…mrg) first, then Upgrade. The camera restarts.",
            "Close the page, log in again, and do the same with the second file (D_CV605_…mrg).",
        ],
        web_path: None,
        warnings: &["Two files, in that order, with the restart between them."],
        upload: None,
        sources: &[src("Marshall: CV605 PTZ firmware update instructions", "https://marshall-usa.com/pdf/CV605-PTZ-firmware-update-instructions.pdf", V)],
    },
    // ── JVC ──────────────────────────────────────────────────────────────────
    FirmwareEntry {
        id: "jvc-pz510",
        brand: "JVC",
        vendors: &["jvc"],
        models: &["KY-PZ510"],
        family: "KY-PZ510, KY-PZ510N",
        downloads: "https://support.jvc.com/pro/firmwareupgrade/versionentry.jsp?modelId=86",
        downloads_note: Some("JVC asks for the camera's current firmware version first."),
        how: &["Follow the instructions JVC publishes with the download."],
        web_path: None,
        warnings: &[
            "From v3.0.12, preset files exported on older firmware can't restore preset positions, and files exported on v3.0.12 can't be used on cameras at that version or older.",
        ],
        upload: None,
        sources: &[src("JVC: KY-PZ510 Firmware Update (v3.0.12 or later), B5A-4617-01", "https://manuals.jvckenwood.com/download/files/B5A-4617-01.pdf", V)],
    },
    FirmwareEntry {
        id: "jvc-pz",
        brand: "JVC",
        vendors: &["jvc"],
        models: &["KY-PZ100", "KY-PZ200", "KY-PZ400"],
        family: "KY-PZ100, KY-PZ200(N), KY-PZ400(N)",
        downloads: "https://support.jvc.com/pro/firmwareupgrade/modellist.jsp",
        downloads_note: Some("Pick the model on JVC's list."),
        how: &["Follow the instructions JVC publishes with the download."],
        web_path: None,
        warnings: &[],
        upload: None,
        sources: &[src("JVC: KY-PZ400 and KY-PZ200 Firmware Update notice, B5A-4559-00", "https://manuals.jvckenwood.com/download/files/B5A-4559-00.pdf", V)],
    },
    // ── Vaddio ───────────────────────────────────────────────────────────────
    FirmwareEntry {
        id: "vaddio-roboshot",
        brand: "Vaddio",
        vendors: &["vaddio", "legrand"],
        models: &["RoboSHOT"],
        family: "RoboSHOT cameras",
        downloads: "https://www.legrandav.com/",
        downloads_note: Some("Find the camera's product page and open its Resources tab."),
        how: &[
            "In the camera's web page, System: Choose File (the firmware ends in .p7m), then Begin Firmware Update.",
            "Read the Confirm dialog, then Continue. The camera restarts when it is done.",
        ],
        web_path: None,
        warnings: &[
            "Keep the camera on power and on the network until it finishes: an interrupted update can leave it unusable.",
            "It can take up to 5 minutes.",
        ],
        upload: None,
        sources: &[
            src(
                "RoboSHOT UHD 2.3.0 release notes and update instructions",
                "https://res.cloudinary.com/iwh/image/upload/q_auto,g_center/assets/1/26/RoboSHOT_UHD_Firmware_Release_Notes_and_Update_Instructions_2.3.0.pdf",
                V,
            ),
            src(
                "RoboSHOT CAT5 3.2.0 release notes and update instructions",
                "https://res.cloudinary.com/avd/image/upload/v132245212/Resources/Vaddio/Cameras/Firmware/RoboSHOT_CAT5_Release_Notes_and_Update_Instructions_3.2.0.pdf",
                V,
            ),
        ],
    },
    // ── Datavideo ────────────────────────────────────────────────────────────
    FirmwareEntry {
        id: "datavideo-ptc140",
        brand: "Datavideo",
        vendors: &["datavideo"],
        models: &["PTC-140"],
        family: "PTC-140, PTC-140T, PTC-140NDI",
        downloads: "https://www.datavideo.com/product/PTC-140T",
        downloads_note: None,
        how: &["In the camera's web page: System Configure > Update, Select File, then Upgrade."],
        web_path: None,
        warnings: &["It takes a few minutes and keeps the settings. Don't interrupt it once it has started: that can leave the camera unresponsive."],
        upload: None,
        sources: &[src("PTC-140 instruction manual, ch. 9: Firmware Update", "http://resource.datavideo.com/Instruction_Manuals/PTC-140_E7_A5_EN.pdf", V)],
    },
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use meerkat_core::catalog::{pick, squash};

    use super::*;

    #[test]
    fn every_row_is_sourced_and_well_formed() {
        let mut ids = HashSet::new();
        for r in FIRMWARE {
            assert!(ids.insert(r.id), "{} twice", r.id);
            assert!(r.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", r.id);
            assert!(!r.brand.is_empty() && !r.family.is_empty(), "{}", r.id);
            assert!(!r.vendors.is_empty() && r.vendors.iter().all(|v| !v.is_empty() && *v == v.to_ascii_lowercase()), "{}", r.id);
            assert!(!r.models.is_empty(), "{}: a firmware row names its models", r.id);
            assert!(r.models.iter().all(|m| !squash(m).replace(['*', '$'], "").is_empty()), "{}", r.id);
            assert!(r.downloads.starts_with("https://") && !r.downloads.contains(' '), "{}: {}", r.id, r.downloads);
            assert!(!r.how.is_empty(), "{}: say how, even if it is the maker's own instructions", r.id);
            assert!(r.web_path.is_none_or(|p| p.starts_with('/')), "{}", r.id);
            assert!(!r.sources.is_empty(), "{} has no source", r.id);
            for s in r.sources {
                assert!(s.url.starts_with("https://") || s.url.starts_with("http://"), "{}: {}", r.id, s.url);
                assert!(!s.title.is_empty() && !s.url.contains(' '), "{}: {}", r.id, s.url);
            }
        }
    }

    fn one(vendor: &str, model: &str) -> &'static str {
        let m = pick(FIRMWARE, vendor, Some(model));
        assert!(m.exact && m.ids.len() == 1, "{vendor} {model}: {:?} exact={}", m.ids, m.exact);
        FIRMWARE.iter().find(|r| r.id == m.ids[0]).unwrap().id
    }

    #[test]
    fn each_camera_finds_its_own_page() {
        assert_eq!(one("Panasonic", "AW-UE150W"), "panasonic-ue150");
        assert_eq!(one("Panasonic", "AW-UE4KG"), "panasonic-ue4");
        assert_eq!(one("Panasonic", "AW-UE40W"), "panasonic-ue40");
        assert_eq!(one("Panasonic", "AW-HE42"), "panasonic-he42");
        assert_eq!(one("Panasonic", "AW-HE130K"), "panasonic-he130");
        assert_eq!(one("Sony", "SRG-X400"), "sony-srg-x400");
        assert_eq!(one("Sony", "ILME-FR7K"), "sony-fr7");
        assert_eq!(one("BirdDog", "X1"), "birddog-x1");
        assert_eq!(one("BirdDog", "X1 Ultra"), "birddog-x1-ultra");
        assert_eq!(one("BirdDog", "X4 Ultra"), "birddog-x4-ultra");
        assert_eq!(one("BirdDog", "X4E Ultra"), "birddog-x4e-ultra");
        assert_eq!(one("BirdDog", "BirdDog P400"), "birddog-p400");
        assert_eq!(one("BirdDog", "MAKI Ultra 20x"), "birddog-maki-ultra-20x");
        assert_eq!(one("PTZOptics / VHD", "PT20X-SDI-G3"), "ptzoptics-g3");
        assert_eq!(one("PTZOptics / VHD", "PT20X-NDI-GY-G2"), "ptzoptics-g2");
        assert_eq!(one("Canon", "CR-N300"), "canon-cr");
        assert_eq!(one("AVer", "PTZ310V2"), "aver-ptz310-330");
        assert_eq!(one("Vaddio", "RoboSHOT 30E HDBT"), "vaddio-roboshot");
    }

    #[test]
    fn only_canon_names_its_firmware_page() {
        let named: Vec<_> = FIRMWARE.iter().filter(|r| r.web_path.is_some()).map(|r| r.id).collect();
        assert_eq!(named, ["canon-cr"]);
    }
}
