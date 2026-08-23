# HomBot UBERBOT

UBERBOT is a reversible modernization project for the LG HomBot VR6340LV.
The project keeps the original real-time motion and safety stack in place while
replacing the obsolete unauthenticated web/camera layer with a small, auditable
Rust daemon.

The current daemon, `hombotd`, provides a low-latency camera UI and a read-only
SmartControl status adapter. Actuator control is intentionally not exposed yet.

## Current status

- Release 0.1.3 is deployed as the active service on the research device.
- Runs as a static ARMv5TE/musl binary on the ARMv6 HomBot platform.
- Streams 320x240 YUV422P color or Y8 grayscale from `/dev/camclone`.
- A newer stream invalidates an older stream to avoid stale camera leases.
- Reads robot status through the device-local LG SmartControl service. On the
  observed legacy boot setup, the managed startup block restores standard
  loopback before connecting.
- SmartControl is connected: the long-lived port-4000 channel remains established
  and the one-shot port-4002 admission channel is explicitly closed after enable.
- Current `CONNECT_INIT` status fields are null. The API preserves that unknown
  state instead of inventing robot, battery or mode values.
- Exposes health and status endpoints plus a standalone browser FPV page.
- Keeps the original `lg.srv` binary on the device and provides a documented
  rollback path.

Latest bounded camera checks delivered 20/20 unique frames in both modes: 10.29
FPS for color and 16.53 FPS for grayscale. Results describe the tested WLAN path,
not a guaranteed performance level.

This is experimental robotics software. It is not production-ready and it must
not be used to bypass battery, cliff, wheel-drop, thermal or motion safety.

## Repository layout

```text
hombotd/                 Rust daemon and embedded web UI
deploy/                  guarded install and rollback scripts
docs/ARCHITECTURE.md     component boundaries and data flows
docs/PROTOCOL.md         reconstructed SmartControl framing
docs/REVERSE_ENGINEERING.md
docs/HARDWARE.md         board, UART and expansion findings
docs/ROADMAP.md          staged path from sidecar to recovery OS
docs/USB_TETHERING.md    Android RNDIS/CDC driver and relay plan
.github/workflows/       reproducible ARM kernel-module build
SECURITY.md              threat model and disclosure guidance
CONTRIBUTING.md          developer workflow and evidence rules
```

Firmware images, NAND dumps, device captures, credentials, network identifiers,
vendor binaries and build output are deliberately not part of this repository.

## Build

The daemon is dependency-free Rust and targets the oldest compatible CPU
baseline used by the current deployment:

```powershell
rustup target add armv5te-unknown-linux-musleabi
$sysroot = rustc --print sysroot
$env:CARGO_TARGET_ARMV5TE_UNKNOWN_LINUX_MUSLEABI_LINKER =
  Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-gnu\bin\rust-lld.exe'
$env:RUSTFLAGS = '-C linker-flavor=ld.lld'
cargo build --manifest-path hombotd/Cargo.toml --release `
  --target armv5te-unknown-linux-musleabi
```

The binary is created at
`hombotd/target/armv5te-unknown-linux-musleabi/release/hombotd-prototype`.

## Run on a development host

The camera device path and listen address can be supplied by environment
variables. On the robot, keep logs in RAM-backed `/tmp` to reduce UBIFS wear.
Consult `hombotd/README.md` for the exact endpoints and current limitations.

## UBERPHONE USB relay

Android USB tethering is an active hardware/software track. The stock kernel
has `usbnet`, CDC Ethernet and RNDIS host support configured as modules, but LG
omitted the latter two binaries. A reproducible module workflow and the guarded
live-test plan are documented in
[docs/USB_TETHERING.md](docs/USB_TETHERING.md).

## Deployment policy

1. Build and verify the binary off-device.
2. Copy it into a versioned release directory.
3. Verify its digest on the device.
4. Keep the original startup file and `lg.srv` executable.
5. Stage and syntax-check startup changes before an atomic rename.
6. Verify health after activation and retain an out-of-band recovery route.

The scripts under `deploy/` model that process but contain no hostnames,
addresses or credentials. Review their version paths before use.

## Project principles

- Evidence before assumptions: claims cite code, symbols, logs or measurements.
- Read-only observation before commands.
- No flash writes before UART recovery, a RAM-only boot and complete NAND/OOB
  recovery have been demonstrated.
- Every persistent change must have a tested rollback.
- Motion requires a short exclusive lease, heartbeat and independent sensor
  interlocks; this layer is not implemented yet.

## Legal note

This repository contains original interoperability and research code, not LG
firmware or copied vendor binaries. Device owners are responsible for complying
with local law, warranty terms and safety requirements.
