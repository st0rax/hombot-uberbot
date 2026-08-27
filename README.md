# HomBot UBERBOT

> **Verbindliche Arbeitsgrundlage:** Vor jeder Arbeit ist [`AGENTS.md`](AGENTS.md) vollständig zu lesen und strikt zu befolgen. Projektspezifische Regeln gelten ergänzend; bei Konflikten gilt die strengere Schutzregel.

UBERBOT is the independent integration project that combines two separately
versioned lines of work:

- [**webagent-rs**](https://github.com/st0rax/webagent-rs) provides the
  provider-agnostic agent/Brain runtime on a capable companion host.
- The **HomBot modernization** in this repository provides the physical body:
  the audited `hombotd` sidecar, device research, deployment and recovery
  procedures.

Neither project is absorbed into the other. `webagent-rs` keeps its own
repository, releases and product scope; the HomBot sidecar keeps its own device
status, safety rules and release line. This repository owns the integration
contracts and HomBot-specific adapters that turn those projects into the new
Uberbot system. See [`docs/PROJECT_BOUNDARIES.md`](docs/PROJECT_BOUNDARIES.md)
for the ownership rules and
[`docs/UBERBOT_ROADMAP.md`](docs/UBERBOT_ROADMAP.md) for the integration plan.

The physical-device project remains a reversible modernization of the LG
HomBot VR6340LV. It keeps the original real-time motion and safety stack in
place while replacing the obsolete unauthenticated web/camera layer with a
small, auditable Rust daemon.

**New to this repository? Start with [`START_HERE.md`](START_HERE.md)**, then
[`STATUS_LIVE.md`](STATUS_LIVE.md) for what is actually verified on the
physical device right now, then [`AGENTS.md`](AGENTS.md) before running
anything against it.

The current daemon, `hombotd`, provides a low-latency camera UI, live audio
streaming, a read-only SmartControl status adapter, and a decoded (but not yet
live-confirmed) subscriber for LG's own factory voice services. Actuator
control is intentionally not exposed yet.

> **Integration status:** the repository boundary and target architecture are
> documented, but no WebAgent/Uberbot core is deployed on the robot and no
> agent-to-body control path exists yet. The verified live system remains the
> standalone `hombotd` sidecar described in `STATUS_LIVE.md`.

## Current status

Release **0.1.10** remains the active service on the research device. The
repository's `main` branch prepares **0.1.11**, which adds a read-only
Voice-Telemetry panel
for the existing `/api/v1/voice` data: subscriber state, last decoded sound
bearing, last event, event counter and confirmation state. It does not enable
the subscriber, alter boot settings or expose actuator control. See
[`STATUS_LIVE.md`](STATUS_LIVE.md) for the up-to-date, verified-vs-decoded
breakdown; it changes faster than this file. In short:

- Runs as a static ARMv5TE/musl binary on the ARMv6 HomBot platform.
- Streams 320x240 YUV422P color or Y8 grayscale from `/dev/camclone`, and now
  a continuous 16 kHz mono WAV audio stream from whichever USB sound card in
  the robot's hub is free (`/stream.wav`, `/api/v1/audio`).
- A newer camera or audio stream invalidates an older one of the same kind to
  avoid stale leases.
- Reads robot status through the device-local LG SmartControl service, and
  reports live network interface state including whether an Android phone is
  attached via USB tethering (`/api/v1/system`).
- Exposes health, status, system, audio, voice and network endpoints plus a
  standalone browser FPV/telemetry page with a spoken boot greeting.
- Keeps the original `lg.srv` binary on the device and provides a documented
  rollback path.
- The first write-capable endpoint, `POST /api/v1/audio/play`, plays an
  uploaded clip through a free sound card and requires a local
  `X-Hombot-Token` (generated on first use, mode 0600) -- see `AGENTS.md`.
- USB tethering (RNDIS/CDC) and USB audio kernel modules are built
  reproducibly against a reconstructed kernel, ABI-verified offline, and
  `insmod`-confirmed live on the device -- see
  [`docs/USB_TETHERING.md`](docs/USB_TETHERING.md).

This is experimental robotics software. It is not production-ready and it must
not be used to bypass battery, cliff, wheel-drop, thermal or motion safety.

## Repository layout

```text
START_HERE.md            read this first
STATUS_LIVE.md           what is verified on the device right now
AGENTS.md                working rules, one per real incident
hombotd/                 Rust daemon and embedded web UI
tools/operator/          Windows-side scripts that drive the robot remotely
deploy/                  guarded install and rollback scripts
docs/ARCHITECTURE.md     component boundaries and data flows
docs/PROJECT_BOUNDARIES.md repository ownership and integration rules
docs/MOTION_READINESS.md evidence gate before physical movement
docs/PROTOCOL.md         reconstructed SmartControl framing
docs/REVERSE_ENGINEERING.md
docs/HARDWARE.md         board, UART and expansion findings
docs/ROADMAP.md          HomBot device and recovery roadmap
docs/UBERBOT_ROADMAP.md  cross-project Uberbot integration roadmap
docs/USB_TETHERING.md    Android RNDIS/CDC driver and relay plan
docs/VOICE_STACK.md      LG's dormant factory voice/keyword/SSL services
docs/VOICE_PROTOCOL.md   their message formats, decoded from disassembly
docs/OPERATOR_TOOLS.md   what the tools/operator/ scripts do and why
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

This runs the HomBot body service only. The future Uberbot integration runtime
is a separate component described in `docs/UBERBOT_ROADMAP.md`; it must not be
implied by a successful `hombotd` development-host run.

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
