Factory logins, firmware pages, and firmware install over ONVIF.

**Still a beta.** Every driver is written from the vendor's published documentation and tested end to end against the simulator built into the app. No real camera has had its address changed by Meerkat yet, and none has been sent a firmware file.

## New

- **Factory logins.** The detail panel says what the selected camera logs in with out of the box, from its maker's own documents: the login, or none when the account is made at first use, whether the camera makes you change it, and what happens after wrong passwords. It covers Panasonic, Sony, BirdDog, PTZOptics, Canon, Lumens, AVer, Marshall, JVC, Vaddio, Datavideo, Avonic, Bolin, Zowietek and Telycam. **Meerkat never tries a login by itself**: "Use this login" only fills the fields.
- **Firmware pages.** A link to the maker's download page for the model, with the maker's update steps and warnings. Every fact links to its source. The sidebar's **Factory logins…** and **Firmware downloads…** list every brand.
- **Firmware install over ONVIF.** A camera that answers ONVIF and says it takes a firmware file over HTTP gets **Install a firmware file…**. Download the file from the maker first. Meerkat asks the camera with its administrator login, shows the file's SHA-256 beside the maker's warnings, and sends it only after you confirm it is that maker's firmware for that model. It sends it the way the ONVIF Core specification publishes (StartFirmwareUpgrade, §8.3.9), always to the camera's own address, waits for the camera to come back and reads its version. Only a changed version counts as success. Meerkat does not supply or download firmware, and brands whose upload is not ONVIF keep the maker's steps.
- **`meerkat-cli install <camera> [<file>]`**: asks the camera without a file, is a dry run without `--yes`.

## Not tested yet

- Sending firmware to a real camera.
- Changing the address of a real Panasonic, Sony, BirdDog or ONVIF camera.
- The temporary-address commands on a real interface, on macOS, Windows or Linux.
- The Windows and Linux builds against a camera.

The Windows installer is not code-signed, so SmartScreen will warn: choose **More info → Run anyway**.

The [user guide](https://github.com/stoatworks-labs/meerkat/blob/main/docs/USER-GUIDE.md) covers every control.

**Full changelog**: https://github.com/stoatworks-labs/meerkat/compare/v0.1.0...v0.2.0
