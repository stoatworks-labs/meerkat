The first tagged build of Meerkat: find the PTZ cameras on a network, including the ones on the wrong subnet, give them the right address, and open their web pages.

**A beta.** Every driver is written from the vendor's published documentation and tested end to end against the simulator built into the app. No real camera has had its address changed by Meerkat yet, so try a change on an idle camera on the bench before you rely on it.

## What it does

- **Finds cameras with each brand's own discovery:** Panasonic Easy IP, Sony IP Setup, the VHD multicast for PTZOptics, NDI over mDNS (BirdDog and others) and ONVIF WS-Discovery. Every scan goes out of every network interface at once.
- **Across subnets:** layer-2 discovery hears cameras on the wrong subnet, and a routed range is swept address by address.
- **Changes the address, mask, gateway, DNS and name:** by MAC for Panasonic and Sony, even on the wrong subnet; over the API for BirdDog X-series and MAX (DHCP included) and for ONVIF. Every change is checked before it is sent, and the camera is found again at its new address. Several cameras can be given consecutive addresses in one go.
- **Opens each camera's web UI** on the port its brand uses.
- **Reach it from here:** a temporary second address on your interface, in a wrong-subnet camera's subnet, so its web page opens and HTTP drivers can change it. It asks for an administrator password, and is removed when Meerkat quits.
- **Lost a BirdDog?** A hand-off to BirdDog's Neuralyzer for a P-series camera at an address nothing reaches, then a watch that scans for it, its factory fallback address included.
- **Demo cameras:** eight simulated cameras answering every protocol on loopback, with nothing sent to the network.
- A command-line twin, `meerkat-cli`, built from source.

## Not tested yet

- Changing the address of a real Panasonic, Sony, BirdDog or ONVIF camera.
- The temporary-address commands on a real interface, on macOS, Windows or Linux.
- The Windows and Linux builds against a camera.

The Windows installer is not code-signed, so SmartScreen will warn: choose **More info → Run anyway**.

The [user guide](https://github.com/stoatworks-labs/meerkat/blob/main/docs/USER-GUIDE.md) covers every control.
