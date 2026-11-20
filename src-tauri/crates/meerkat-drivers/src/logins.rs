//! The login each family of cameras leaves the factory with, from its
//! maker's manuals, with the place each one was read.
//!
//! The shape and the matching rule are `meerkat_core::catalog`. A row is
//! matched by the camera's vendor string (lower-case pieces in `vendors`) and
//! its model (patterns in `models`, see [`meerkat_core::catalog::Applies`]).
//!
//! Rules for a row:
//!
//! - Every fact has a source. Prefer the maker's own document
//!   ([`SourceKind::Vendor`]). A fact seen only in a search excerpt of the
//!   maker's document is [`SourceKind::VendorExcerpt`]; one from anywhere else
//!   is [`SourceKind::ThirdParty`], and the UI says so.
//! - When firmware decides, there are two rows with the same models and a
//!   `firmware` line each: one per state, never a guess at which applies.
//! - Newer cameras mostly have no factory login: that is
//!   [`Login::Create`], not a blank.
//! - Say what the maker says about lockouts. Meerkat never tries a login on
//!   its own, and the UI shows this line wherever it offers to fill one in.

use meerkat_core::catalog::{Login, LoginEntry, Source, SourceKind};

const fn src(title: &'static str, url: &'static str, kind: SourceKind) -> Source {
    Source { title, url, kind }
}

const V: SourceKind = SourceKind::Vendor;
const X: SourceKind = SourceKind::VendorExcerpt;
const T: SourceKind = SourceKind::ThirdParty;

const fn factory(username: &'static str, password: &'static str) -> Login {
    Login::Factory { username, password }
}

const PANASONIC: &[&str] = &["panasonic"];
const SONY: &[&str] = &["sony"];
const BIRDDOG: &[&str] = &["birddog"];
const PTZOPTICS: &[&str] = &["ptzoptics"];
const CANON: &[&str] = &["canon"];
const VADDIO: &[&str] = &["vaddio", "legrand"];
const DATAVIDEO: &[&str] = &["datavideo"];

const PTZOPTICS_SOURCES: &[Source] = &[
    src("PTZOptics docs: I can't log in", "https://docs.ptzoptics.com/articles/troubleshooting/cant-log-in", V),
    src("PTZOptics: known vulnerabilities and fixes (CVE-2025-35451, CVE-2025-35452)", "https://ptzoptics.com/known-vulnerabilities-and-fixes", V),
];

const PTZOPTICS_G3: &[&str] = &["PT*-G3"];
const PTZOPTICS_G3_FAMILY: &str = "G3 cameras: Move 4K, Move SE, Link 4K, Studio Pro";

pub static LOGINS: &[LoginEntry] = &[
    // ── Panasonic ────────────────────────────────────────────────────────────
    LoginEntry {
        id: "panasonic-he40-he130",
        brand: "Panasonic",
        vendors: PANASONIC,
        models: &["AW-HE40", "AW-HE130"],
        family: "AW-HE40, AW-HE130",
        firmware: None,
        login: factory("admin", "12345"),
        change_at_first_login: false,
        lockout: None,
        notes: &[
            "User authentication is off from the factory on the AW-HE40, so it may not ask for a login at all.",
            "An AW-RP50 controller has to be given the same login as the camera; one on firmware 3.30.01 or older cannot connect once authentication is on.",
            "The AW-HE130 reminds you to change the password while the factory one is in use (from a third-party copy of its manual).",
        ],
        sources: &[
            src("Panasonic FAQ: User Authentication Function", "https://eww.pass.panasonic.co.jp/pro-av/support/content/faq/EN/remcam/30_.pdf", V),
            src(
                "AW-HE130 firmware update procedure, step 6",
                "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/he130/he130_update_e_newtek.pdf",
                V,
            ),
        ],
    },
    LoginEntry {
        id: "panasonic-ue70-un70",
        brand: "Panasonic",
        vendors: PANASONIC,
        models: &["AW-UE70", "AW-UN70"],
        family: "AW-UE70, AW-UN70",
        firmware: None,
        login: factory("admin", "12345"),
        change_at_first_login: false,
        lockout: None,
        notes: &[],
        sources: &[
            src(
                "AW-UE70 firmware update procedure (Sep 2022)",
                "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/ue70/ue70_update_newtek_e.pdf",
                X,
            ),
            src(
                "AW-UN70 firmware update procedure (Sep 2022)",
                "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/un70/un70_update_e.pdf",
                X,
            ),
        ],
    },
    LoginEntry {
        id: "panasonic-initial-account",
        brand: "Panasonic",
        vendors: PANASONIC,
        models: &["AW-UE4", "AW-UE20", "AW-HE20", "AW-UE40", "AW-UE50", "AW-UE80", "AW-UE100", "AW-UE150", "AW-UE160"],
        family: "AW-UE4, UE20/HE20, UE40, UE50, UE80, UE100, UE150, UE160",
        firmware: None,
        login: Login::Create { username: None },
        change_at_first_login: false,
        lockout: Some("Not documented. A wrong login is written to the camera's event log (W1302, with the user and the address)."),
        notes: &[
            "The first visit to its web page opens an initial account screen, and its network functions do not work until the account exists.",
            "A Panasonic RP-series controller can find the camera but not control it until then (AW-UE160).",
            "Panasonic's Easy IP Setup Tool Plus can create the account too.",
            "The AW-UE160 asks for 8 or more characters of 3 kinds out of upper case, lower case, digits and symbols, but lets you go on after a warning.",
            "A forgotten account: the AW-UE160's service switch Initialization 1 clears its users and hosts.",
            "Panasonic's update procedures show admin / 12345 only as an example of what to type. For the UE20, UE40, UE50 and UE80 only excerpts of them were checked.",
        ],
        sources: &[
            src("AW-UE160 Operating Instructions, p. 52: Setting the initial account", "https://pro-av.panasonic.net/manual/pdf/AW-UE160P.PJ_simple(DVQX2340YA)_E.pdf", V),
            src("AW-UE4 Operating Instructions, p. 20", "https://pro-av.panasonic.net/manual/pdf/AW-UE4KG_WG_Operation(DVQP2109ZA)_E.pdf", V),
            src(
                "AW-UE150 firmware update procedure, p. 1",
                "https://eww.pass.panasonic.co.jp/pro-av/support/content/download/EN/ep2main/soft/ue150/ue150_update_e.pdf",
                V,
            ),
        ],
    },
    // ── Sony ─────────────────────────────────────────────────────────────────
    LoginEntry {
        id: "sony-x400-x120",
        brand: "Sony",
        vendors: SONY,
        models: &["BRC-X400", "BRC-X401", "SRG-X400", "SRG-X402", "SRG-201M2", "SRG-X120", "SRG-HD1M2"],
        family: "BRC-X400/X401, SRG-X400/X402/201M2, SRG-X120/HD1M2",
        firmware: None,
        login: factory("admin", "Admin_1234"),
        change_at_first_login: true,
        lockout: Some(
            "Brute-force protection covers the web page and RTSP: after a set number of wrong logins it blocks the address, until a timer runs out or for good, as set. A reboot clears the list. The factory count is not stated.",
        ),
        notes: &[
            "The new user name must be 5 to 16 characters and the password 8 to 64.",
            "From firmware 3.00 the factory password cannot be kept as the new one (from a third-party copy of the release note).",
        ],
        sources: &[src(
            "SRG-X120/X400 Operating Instructions 5-011-250-12(1), pp. 10, 58, 87 (Sony's document, hosted by a reseller)",
            "https://www.adorama.com/col/productManuals/SOSRGX120W.pdf",
            V,
        )],
    },
    LoginEntry {
        id: "sony-a40-a12",
        brand: "Sony",
        vendors: SONY,
        models: &["SRG-A40", "SRG-A12"],
        family: "SRG-A40, SRG-A12",
        firmware: None,
        login: factory("admin", ""),
        change_at_first_login: true,
        lockout: None,
        notes: &["The first login makes you set the administrator's password, of 8 to 64 characters."],
        sources: &[src(
            "SRG-A40/A12 VISCA/CGI Command List v4.0",
            "https://pro.sony/support/res/manuals/5053/f48354480e724f0aef3d45d2e1eeab63/50531551M.pdf",
            X,
        )],
    },
    LoginEntry {
        id: "sony-fr7-am7",
        brand: "Sony",
        vendors: SONY,
        models: &["ILME-FR7", "FR7", "BRC-AM7"],
        family: "ILME-FR7, FR7K, BRC-AM7",
        firmware: None,
        login: factory("admin", ""),
        change_at_first_login: true,
        lockout: Some("Brute-force protection is on from the factory: 8 wrong logins block the address for 60 seconds (adjustable from 30 to 86,400)."),
        notes: &[
            "The blank login is temporary: the first browser login makes you create the administrator's name and a password of 8 to 64 characters with upper case, lower case and a digit.",
            "The BRC-AM7 asks again if its backup battery runs completely flat.",
        ],
        sources: &[
            src("ILME-FR7 Help Guide: Initial setup of the unit", "https://helpguide.sony.net/ilc/2240/v1/en/contents/TP1000673409.html", V),
            src("ILME-FR7 Help Guide: Brute Force Attack Protection", "https://helpguide.sony.net/ilc/2240/v1/en/contents/TP1000669775.html", V),
            src("BRC-AM7 Help Guide: Brute Force Attack Protection", "https://helpguide.sony.net/rc/brc-am7/v1/en/contents/wm_brute_protection.html", V),
        ],
    },
    // ── BirdDog ──────────────────────────────────────────────────────────────
    LoginEntry {
        id: "birddog-p",
        brand: "BirdDog",
        vendors: BIRDDOG,
        models: &["P100", "P200", "P400", "P4K"],
        family: "P100, P200, P400, P4K",
        firmware: None,
        login: factory("", "birddog"),
        change_at_first_login: false,
        lockout: None,
        notes: &["There is no user name: the web page asks for the password only.", "NDI Studio Monitor's settings dialog takes the same password."],
        sources: &[
            src("BirdDog P100 User Guide, p. 25: Password Management", "https://birddog.tv/wp-content/uploads/2022/01/BirdDog_P100_User-Guide_231026.pdf", V),
            src("BirdDog BirdUI User Guide: Default Password", "https://birddog.tv/wp-content/uploads/2024/04/BirdDog_BirdUI_User-Guide.pdf", V),
        ],
    },
    LoginEntry {
        id: "birddog-x",
        brand: "BirdDog",
        vendors: BIRDDOG,
        models: &["X1", "X4", "MAKI Ultra"],
        family: "X1, X1 Ultra, X4 Ultra, MAKI Ultra",
        firmware: None,
        login: factory("", "birddog"),
        change_at_first_login: true,
        lockout: None,
        notes: &[
            "The X4 Ultra makes you set your own password at the first login, and again after a reset. For the X1 and MAKI Ultra only an excerpt says the same.",
            "The IR remote's *, #, MANUAL sequence restores the factory address (192.168.100.100) and password.",
        ],
        sources: &[
            src("BirdDog X4 Ultra manual: setup, and Forgotten IP address or login password", "https://birddog.tv/wp-content/uploads/2024/11/BirdDog-X4-Ultra-Manual.pdf", V),
            src("BirdDog X1 / X1 Ultra manual", "https://birddog.tv/wp-content/uploads/2024/04/BirdDog-X1-X1-Ultra_manual_02-05-2025.pdf", V),
            src("BirdDog knowledge base: factory reset, MAKI Ultra", "https://desk.birddog.tv/portal/en/kb/articles/factory-reset-maki-ultra", X),
        ],
    },
    // ── PTZOptics ────────────────────────────────────────────────────────────
    LoginEntry {
        id: "ptzoptics-g2",
        brand: "PTZOptics",
        vendors: PTZOPTICS,
        models: &["PT*-G2"],
        family: "G2 cameras: 12X, 20X and 30X SDI, NDI and USB, ZCAM",
        firmware: None,
        login: factory("admin", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &[
            "PTZOptics lists every G2 under CVE-2025-35452, a web password shared across cameras, until that password is changed.",
            "Older G2 firmware also has SSH and Telnet logins with fixed passwords (CVE-2025-35451), fixed in 12X 6.3.34, 20X 6.3.20, 30X 6.3.30 and USB 6.2.81 and 6.2.73.",
        ],
        sources: PTZOPTICS_SOURCES,
    },
    LoginEntry {
        id: "ptzoptics-g3-before-fix",
        brand: "PTZOptics",
        vendors: PTZOPTICS,
        models: PTZOPTICS_G3,
        family: PTZOPTICS_G3_FAMILY,
        firmware: Some("Before the early-2025 security update"),
        login: factory("admin", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &[],
        sources: PTZOPTICS_SOURCES,
    },
    LoginEntry {
        id: "ptzoptics-g3-after-fix",
        brand: "PTZOptics",
        vendors: PTZOPTICS,
        models: PTZOPTICS_G3,
        family: PTZOPTICS_G3_FAMILY,
        firmware: Some(
            "From the early-2025 security update: 4K G3 12X 0.0.58, 20X 0.0.85, 30X 2.0.64; Link 4K 0.0.63, 0.0.89, 2.0.71; Move SE 9.1.43, 9.1.32, 9.1.33; Studio Pro 9.0.41",
        ),
        login: Login::Create { username: Some("admin") },
        change_at_first_login: false,
        lockout: None,
        notes: &[
            "The first login, and the first after a password reset, makes you set admin's password; it cannot be admin.",
            "There is a guest account too; its password cannot be guest.",
            "The IR remote's network reset brings the setup screen back and switches the camera to DHCP.",
        ],
        sources: PTZOPTICS_SOURCES,
    },
    // ── Canon ────────────────────────────────────────────────────────────────
    LoginEntry {
        id: "canon-crn100-500",
        brand: "Canon",
        vendors: CANON,
        models: &["CR-N100", "CR-N300", "CR-N500"],
        family: "CR-N100, CR-N300, CR-N500",
        firmware: None,
        login: Login::Create { username: None },
        change_at_first_login: false,
        lockout: Some("Not documented. Only one administrator can be logged in at a time."),
        notes: &[
            "Canon's Camera Search Tool or the browser opens Default Settings (1/2), where you name the administrator and set its password.",
            "Controllers log in as an Authorized User; guests need no login. RTSP has its own authentication setting, and NDI, RTMP and SRT are not covered by the user accounts.",
            "A forgotten password needs a factory reset, which also clears the network settings.",
        ],
        sources: &[
            src("CR-N500/CR-N300 Setup Guide (firmware 1.7.0), pp. 15–16 and 76–77", "https://gdlp01.c-wss.com/gds/4/0300048394/03/crn500-300-sg-en.pdf", V),
            src("CR-N100 Setup Guide", "https://gdlp01.c-wss.com/gds/7/0300047877/04/crn100-sg-en.pdf", V),
        ],
    },
    LoginEntry {
        id: "canon-crn700-crx",
        brand: "Canon",
        vendors: CANON,
        models: &["CR-N700", "CR-X300", "CR-X500"],
        family: "CR-N700, CR-X300, CR-X500",
        firmware: None,
        login: Login::Create { username: None },
        change_at_first_login: false,
        lockout: None,
        notes: &["The same Initial Camera Settings step, per an excerpt of the CR-X300 manual; the CR-N700 and CR-X500 manuals were not read in full."],
        sources: &[
            src("CR-X300 User Manual, p. 26: Initial Camera Settings", "https://gdlp01.c-wss.com/gds/3/0300043553/06/crx300-um-en.pdf", X),
            src("CR-N700 Setup Guide", "https://gdlp01.c-wss.com/gds/1/0300045551/03/crn7sg-e.pdf", X),
        ],
    },
    // ── Lumens ───────────────────────────────────────────────────────────────
    LoginEntry {
        id: "lumens-vc",
        brand: "Lumens",
        vendors: &["lumens"],
        models: &["VC-A50P", "VC-A61P", "VC-A71P", "VC-TR1"],
        family: "VC-A50P, VC-A61P, VC-A71P/A71PN, VC-TR1",
        firmware: None,
        login: factory("admin", "9999"),
        change_at_first_login: false,
        lockout: None,
        notes: &[
            "Lumens asks you to change it at the first login.",
            "RTSP uses the web account. RTSP authentication is off from the factory, except on the VC-TR1.",
            "The VC-A50P firmware manual says new firmware changed the default password, then lists admin / 9999 again.",
        ],
        sources: &[
            src("VC-A71P/A71PN User Manual (2024-02-05), p. 31", "https://www.mylumens.com/Download/VC-A71P_A71PN-User_Manual-English%2020240205.pdf", V),
            src("VC-A61P User Manual", "https://www.mylumens.com/Download/VC-A61P_User_Manual-English_2020-0113.pdf", V),
            src("VC-A50P Firmware Update Manual, 3.2 and 3.5", "https://www.mylumens.com/Download/FW017%20-%20VC-A50P%20FW%20Update%20Manual.pdf", V),
        ],
    },
    // ── AVer ─────────────────────────────────────────────────────────────────
    LoginEntry {
        id: "aver-ptc500",
        brand: "AVer",
        vendors: &["aver"],
        models: &["PTC500"],
        family: "PTC500, PTC500S",
        firmware: None,
        login: factory("", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &["The web page asks for a password only.", "The remote's hot key sets it back to admin."],
        sources: &[src(
            "PTC500S User Manual v1.0.2, pp. 14 and 41",
            "https://communication.aver.com/DownloadFile.aspx?n=5056%7CB527A283-825A-40E6-B43D-641739795372&t=ServiceDownload",
            V,
        )],
    },
    LoginEntry {
        id: "aver-tr-ptz",
        brand: "AVer",
        vendors: &["aver"],
        models: &["TR31*", "TR33*", "PTZ310", "PTZ330"],
        family: "TR310, TR311, TR313, TR331, TR333 (and their V2), PTZ310, PTZ330",
        firmware: None,
        login: factory("admin", "admin"),
        change_at_first_login: true,
        lockout: None,
        notes: &[
            "The first web login makes you change both the user name and the password.",
            "AVer's PTZ Management software starts as admin / admin too, and is changed the same way.",
        ],
        sources: &[
            src(
                "TR Series / TR311(N) User Manual, pp. 31–32",
                "https://www.averusa.com/pro-av/downloads/user-manual/TR%20Series+TR311(N)_User%20Manual_En_v1-2021.04.23.pdf",
                V,
            ),
            src(
                "AVer: PTZ, TR and TR3xxV2 camera integration with Echo360, step 2",
                "https://www.averusa.com/pro-av/downloads/how-to/PTZ-TR%20and%20New%20TR3xx%20and%20TR3xxV2%20Camera%20Integration%20with%20Echo360.pdf",
                V,
            ),
        ],
    },
    // ── Marshall ─────────────────────────────────────────────────────────────
    LoginEntry {
        id: "marshall-cv",
        brand: "Marshall",
        vendors: &["marshall"],
        models: &["CV620", "CV630", "CV730"],
        family: "CV620, CV630, CV730 (IP, NDI and ND3 versions)",
        firmware: None,
        login: factory("admin", "9999"),
        change_at_first_login: false,
        lockout: None,
        notes: &["Marshall advises changing it.", "Read in the CV730-NDI manual; the CV620 and CV630 manuals were checked as excerpts."],
        sources: &[src("CV730-NDI manual, p. 17", "https://marshall-usa.com/pdf/CV730-NDI-manual.pdf", V)],
    },
    // ── JVC ──────────────────────────────────────────────────────────────────
    LoginEntry {
        id: "jvc-pz100",
        brand: "JVC",
        vendors: &["jvc"],
        models: &["KY-PZ100"],
        family: "KY-PZ100",
        firmware: None,
        login: factory("jvc", "0000"),
        change_at_first_login: false,
        lockout: None,
        notes: &["JVC asks you to change it at the first login."],
        sources: &[src("KY-PZ100 instructions (B5A-1715-01): Live View login", "https://manuals.jvckenwood.com/download/files/B5A-1715-01.pdf", V)],
    },
    LoginEntry {
        id: "jvc-pz-n",
        brand: "JVC",
        vendors: &["jvc"],
        models: &["KY-PZ200", "KY-PZ400", "KY-PZ510"],
        family: "KY-PZ200N, KY-PZ400N, KY-PZ510N",
        firmware: None,
        login: factory("jvc", "0000"),
        change_at_first_login: true,
        lockout: None,
        notes: &["A reset returns the camera to 192.168.0.10 and jvc / 0000."],
        sources: &[
            src("KY-PZ200N/PZ400N instructions (B5A-4102-10), p. 25", "https://manuals.jvckenwood.com/download/files/B5A-4102-10.pdf", V),
            src("KY-PZ510N instructions (B5A-4258-10): troubleshooting", "https://manuals.jvckenwood.com/download/files/B5A-4258-10.pdf", V),
        ],
    },
    // ── Vaddio ───────────────────────────────────────────────────────────────
    LoginEntry {
        id: "vaddio-before-2019",
        brand: "Vaddio",
        vendors: VADDIO,
        models: &["RoboSHOT"],
        family: "RoboSHOT cameras",
        firmware: Some("Before RoboSHOT CAT5 3.2.0 and RoboSHOT UHD 2.1.0 (December 2019)"),
        login: factory("admin", "password"),
        change_at_first_login: false,
        lockout: None,
        notes: &["There is a second account, user, with the same password.", "Telnet uses the admin account."],
        sources: &[src(
            "Vaddio Configuration and Administration Guide (999-9960-100), p. 9",
            "https://res.cloudinary.com/iwh/image/upload/q_auto,g_center/assets/1/26/vaddio_999-9960-100_Administration_Guide.pdf",
            V,
        )],
    },
    LoginEntry {
        id: "vaddio-from-2019",
        brand: "Vaddio",
        vendors: VADDIO,
        models: &["RoboSHOT", "PrimeSHOT"],
        family: "RoboSHOT cameras, PrimeSHOT 20 HDMI",
        firmware: Some("RoboSHOT 12E/30E CAT5 3.2.0 and later, RoboSHOT 20/40 UHD 2.1.0 and later, PrimeSHOT 20 HDMI 1.2.0 and later"),
        login: Login::Create { username: Some("admin") },
        change_at_first_login: false,
        lockout: None,
        notes: &[
            "The Initial Set-Up page asks for admin's password, and asks again after a factory reset.",
            "Until then HTTP is off: the camera answers on HTTPS only, with a self-signed certificate. Telnet is off and refused until the password is set.",
            "The IR remote works without a password.",
        ],
        sources: &[
            src(
                "RoboSHOT CAT5 3.2.0 release notes, p. 1",
                "https://res.cloudinary.com/avd/image/upload/v132245212/Resources/Vaddio/Cameras/Firmware/RoboSHOT_CAT5_Release_Notes_and_Update_Instructions_3.2.0.pdf",
                V,
            ),
            src(
                "RoboSHOT UHD 2.3.0 release notes, p. 6: Initial Set-Up Procedure",
                "https://res.cloudinary.com/iwh/image/upload/q_auto,g_center/assets/1/26/RoboSHOT_UHD_Firmware_Release_Notes_and_Update_Instructions_2.3.0.pdf",
                V,
            ),
        ],
    },
    // ── Datavideo ────────────────────────────────────────────────────────────
    LoginEntry {
        id: "datavideo-ptc140",
        brand: "Datavideo",
        vendors: DATAVIDEO,
        models: &["PTC-140"],
        family: "PTC-140, PTC-140T, PTC-140NDI",
        firmware: None,
        login: factory("admin", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &[
            "There are User 1 and User 2 accounts as well.",
            "A button combination restores the login and the address; the factory address is 192.168.5.163.",
        ],
        sources: &[src("PTC-140 instruction manual: web login", "http://resource.datavideo.com/Instruction_Manuals/PTC-140_E7_A5_EN.pdf", X)],
    },
    LoginEntry {
        id: "datavideo-ptc285-305",
        brand: "Datavideo",
        vendors: DATAVIDEO,
        models: &["PTC-285", "PTC-305"],
        family: "PTC-285(T), PTC-305(T)",
        firmware: None,
        login: factory("admin", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &[],
        sources: &[src("PTC-305T FAQ (a manual mirror)", "https://www.notice-facile.com/en/manual/1043099/datavideo+ptc-305t", T)],
    },
    // ── Others ───────────────────────────────────────────────────────────────
    LoginEntry {
        id: "avonic",
        brand: "Avonic",
        vendors: &["avonic"],
        models: &[],
        family: "Every Avonic PTZ: CM40, CM70, CM90",
        firmware: None,
        login: factory("admin", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &["There are user1 and user2 accounts as well (from an excerpt)."],
        sources: &[src(
            "Avonic knowledge base: Default IP address and access",
            "https://avonic.freshdesk.com/support/solutions/articles/80000554306-how-to-set-the-ip-address-of-a-camera",
            V,
        )],
    },
    LoginEntry {
        id: "bolin",
        brand: "Bolin",
        vendors: &["bolin"],
        models: &[],
        family: "Bolin PTZ cameras: EXU248F, the dual-output SDI/IP range, VCC-7",
        firmware: None,
        login: factory("admin", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &[
            "The EXU248F (A2) and VCC-7 make you change it at the first login.",
            "ONVIF uses the same login, on port 2000.",
            "A forgotten password: Bolin support emails a reset code, which gives a temporary password for 24 hours.",
            "One FEX zoom-camera guide gives admin or 123456 instead (from an excerpt).",
        ],
        sources: &[
            src("Bolin EXU248F (A2) User Guide: login and ONVIF", "https://knowledge.bolintechnology.com/exu248fa2userguide", V),
            src(
                "Bolin Outdoor Dual Output IP Camera Network User Guide: login",
                "https://bolintechnology.com/wp-content/uploads/2023/03/PART-TWO-Outdoor-Dual-Output-IP-camera-Network-User-Guide-02202021.pdf",
                V,
            ),
        ],
    },
    LoginEntry {
        id: "zowietek",
        brand: "Zowietek",
        vendors: &["zowietek", "zowie"],
        models: &[],
        family: "ZowiePTZ cameras",
        firmware: None,
        login: factory("admin", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &[
            "The built-in hotspot's name and password are on the rear label.",
            "The remote's * + # + Manual restores the login and the address; the fallback address is 192.168.5.168.",
        ],
        sources: &[src("Zowietek PTZ camera quick start: network note", "https://zowietek.com/wp-content/uploads/2024/08/zowietek-PTZ-Camera-quick-start2.pdf", V)],
    },
    LoginEntry {
        id: "telycam-dp300",
        brand: "Telycam",
        vendors: &["telycam"],
        models: &["DP300*"],
        family: "Drive+ SE DP300-20H",
        firmware: None,
        login: factory("admin", "admin"),
        change_at_first_login: false,
        lockout: None,
        notes: &[],
        sources: &[src("Drive+ SE DP300-20H FAQ (a manual mirror)", "https://www.notice-facile.com/en/manual/913514/telycam+drive-se-dp300-20h", T)],
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
        for r in LOGINS {
            assert!(ids.insert(r.id), "{} twice", r.id);
            assert!(r.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", r.id);
            assert!(!r.brand.is_empty() && !r.family.is_empty(), "{}", r.id);
            assert!(!r.vendors.is_empty() && r.vendors.iter().all(|v| !v.is_empty() && *v == v.to_ascii_lowercase()), "{}", r.id);
            assert!(r.models.iter().all(|m| !squash(m).replace(['*', '$'], "").is_empty()), "{}", r.id);
            assert!(!r.sources.is_empty(), "{} has no source", r.id);
            for s in r.sources {
                assert!(s.url.starts_with("https://") || s.url.starts_with("http://"), "{}: {}", r.id, s.url);
                assert!(!s.title.is_empty() && !s.url.contains(' '), "{}: {}", r.id, s.url);
            }
            if let Login::Factory { username, password } = r.login {
                assert!(!username.is_empty() || !password.is_empty(), "{}: a factory login with neither part", r.id);
            }
            if matches!(r.login, Login::Create { .. }) {
                assert!(!r.change_at_first_login, "{}: there is no factory login to change", r.id);
            }
        }
    }

    fn ids(vendor: &str, model: Option<&str>) -> (Vec<String>, bool) {
        let m = pick(LOGINS, vendor, model);
        (m.ids, m.exact)
    }

    fn one(vendor: &str, model: &str) -> String {
        let (ids, exact) = ids(vendor, Some(model));
        assert!(exact && ids.len() == 1, "{vendor} {model}: {ids:?} exact={exact}");
        ids[0].clone()
    }

    #[test]
    fn the_cameras_the_drivers_report_find_their_rows() {
        assert_eq!(one("Panasonic", "AW-UE150"), "panasonic-initial-account");
        assert_eq!(one("Panasonic", "AW-UE100"), "panasonic-initial-account");
        assert_eq!(one("Panasonic", "AW-HE130W"), "panasonic-he40-he130");
        assert_eq!(one("Panasonic", "AW-UE70K"), "panasonic-ue70-un70");
        assert_eq!(one("Sony", "SRG-X120"), "sony-x400-x120");
        assert_eq!(one("Sony", "ILME-FR7K"), "sony-fr7-am7");
        assert_eq!(one("Sony", "BRC-AM7"), "sony-fr7-am7");
        assert_eq!(one("BirdDog", "X1 Ultra"), "birddog-x");
        assert_eq!(one("BirdDog", "BirdDog P400"), "birddog-p");
        assert_eq!(one("AVer", "PTZ330UV"), "aver-tr-ptz");
        assert_eq!(one("AVer", "TR311N"), "aver-tr-ptz");
        assert_eq!(one("Lumens", "VC-A71PN"), "lumens-vc");
        assert_eq!(one("JVC", "KY-PZ400N"), "jvc-pz-n");
        assert_eq!(one("Avonic", "CM70-IP"), "avonic");
        assert_eq!(one("PTZOptics / VHD", "PT20X-SDI-GY-G2"), "ptzoptics-g2");
    }

    #[test]
    fn where_firmware_decides_both_states_are_offered() {
        assert_eq!(ids("PTZOptics / VHD", Some("PT20X-SDI-G3")), (vec!["ptzoptics-g3-before-fix".into(), "ptzoptics-g3-after-fix".into()], true));
        assert_eq!(ids("Vaddio", Some("RoboSHOT 30E HDBT")).0, ["vaddio-before-2019", "vaddio-from-2019"]);
        assert_eq!(ids("Vaddio", Some("PrimeSHOT 20 HDMI")).0, ["vaddio-from-2019"]);
    }

    #[test]
    fn an_unknown_model_is_a_hint_from_its_brand() {
        // Sony's IP Setup broadcast does not carry the model.
        let (got, exact) = ids("Sony", None);
        assert!(!exact && got.len() == 3, "{got:?}");
        let (got, exact) = ids("Panasonic", Some("AW-HE42"));
        assert!(!exact && got.contains(&"panasonic-initial-account".to_string()), "{got:?}");
        assert_eq!(ids("ONVIF", Some("IPC-HD")), (vec![], false));
    }
}
