# hombotd

Read-only ARM camera and telemetry server for the LG HomBot VR6340LV. It
replaces the blocking camera path in `lg.srv` while keeping `rpmain` as the
hardware abstraction layer. It never opens the Micom UART.

## Current release

- Version: 0.1.3
- Device path: `/usr/data/frankenhomo/releases/0.1.3/hombotd`
- Listen port: 6260
- SHA-256: `3893b112136b6a351ced2c1b3164cf0c76ad656b75925fe229d12ec506399a0a`
- Static ELF32 ARM, ARMv5TE baseline, musl, 440,104 bytes
- Release 0.1.2 runtime VSZ observed on the HomBot: 532 KiB

## Endpoints

- `GET /` – standalone FPV page
- `GET /healthz` – read-only JSON health response
- `GET /api/v1/status` – live SmartControl status as JSON
- `GET /frame.yuv` – one 320×240 YUV422P frame
- `GET /stream.yuv?fps=15` – continuous color YUV422P
- `GET /stream.y8?fps=20` – continuous 8-bit grayscale/luma

Only one logical stream is current. Opening a newer stream invalidates the
older generation, so changing modes does not leave a stale camera lease.
There are no command, upload, shell or actuator endpoints. The SmartControl
adapter performs only LG's `CONNECT/REQUEST` handshake and periodic
`SESSION/ALIVE`; it deliberately does not request Smart Diagnosis.

## Build on Windows

```powershell
rustup target add armv5te-unknown-linux-musleabi
$sysroot = rustc --print sysroot
$env:CARGO_TARGET_ARMV5TE_UNKNOWN_LINUX_MUSLEABI_LINKER =
  Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-gnu\bin\rust-lld.exe'
$env:RUSTFLAGS = '-C linker-flavor=ld.lld'
cargo build --release --target armv5te-unknown-linux-musleabi
```

The output is
`target/armv5te-unknown-linux-musleabi/release/hombotd-prototype`.

## Measured performance

The camera device itself delivered 20 frames in 0.689 seconds through one
persistent descriptor, approximately 29 raw frames/s. Ten independent `dd`
processes delivered ten frames in 0.439 seconds; process startup is not the
main 2.5-FPS bottleneck by itself. The old `lg.srv` architecture additionally
blocks its central socket loop and creates a new HTTP connection for every
frame.

Over the current RT5370/hotspot path:

- color: approximately 9–12 FPS, 11–15 Mbit/s
- grayscale: approximately 14–20 FPS, 8–13 Mbit/s
- one later 20-FPS grayscale check: 17.35 FPS, all 20 frames unique

Measurements vary with WLAN airtime. Grayscale is intended for lowest-latency
teleoperation; color remains available when detail matters.

## Device startup

`/usr/etc/rc.local` contains the marked block
`FRANKENHOMO_SERVER_START`/`END` and launches:

```sh
HOMBOTD_PORT=6260 HOMBOTD_SMARTCONTROL_HOST=127.0.0.1 \
  /usr/data/frankenhomo/releases/0.1.3/hombotd \
  >/tmp/hombotd.log 2>&1 &
```

The stock boot sequence leaves `lo` without `127.0.0.1` and without the `UP`
flag, although LG's SmartControl design expects localhost. The managed startup
block brings up standard loopback and pins the internal adapter to
`HOMBOTD_SMARTCONTROL_HOST=127.0.0.1` before starting `hombotd`.

The original `lg.srv` startup block is absent. `lg.srv` itself was not deleted.
The pre-change startup file is:

`/usr/data/frankenhomo-backup/rc.local.pre-hombotd-20260823-105728`

The deployed rollback helper is:

`/usr/data/frankenhomo/rollback-to-lgsrv.sh`

## Constraints before production status

- SmartControl exposes coarse robot state, mode and battery level only.
- Raw sensor telemetry and authenticated control still need separate adapters.
- Add bounded connection accounting and a watchdog before exposing write APIs.
- Store logs in `/tmp`, not UBIFS.
- Re-run 24-hour RSS, socket and camera stability tests.
