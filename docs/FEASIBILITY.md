# Feasibility

Skeptiker's 2026-08-26 study, written into the tree. Sources: `docs/ROADMAP.md`,
`docs/HARDWARE.md`, `docs/ARCHITECTURE.md`, `AGENTS.md`, `STATUS_LIVE.md`,
`docs/SENSOR_INVENTORY.md`. Claims below are either cited from those files or
marked as not demonstrated. No extra SBC is nominated. This is not a pitch.

## Verdict

**B (companion host as the brain, HomBot as real-time chassis) is the target
architecture.** It is only honest on top of a finished Stage-2 sidecar.
**A (sidecar Stage 2) is the next stage, not the product.** Skipping it turns
B into a camera dashboard.
**C (full firmware rewrite on the NXP2120) is rejected as the product.** Keep
Stage 4 (UART0 plus RAM boot) as recovery, not as the new system.

The companion architecture is now tracked explicitly in
`docs/UBERBOT_ROADMAP.md`. Its WebAgent/Brain side remains authoritative in the
independent `webagent-rs` project; this repository owns the HomBot body and the
integration contracts, not a fork of WebAgent. See
`docs/PROJECT_BOUNDARIES.md`.

## What is actually running

From `STATUS_LIVE.md` (latest device measurement 2026-08-27; older receipts
remain dated 2026-08-23):

- Deployed service: `hombotd 0.1.10`, reconfirmed 2026-08-27.
- The RawSensor subscriber was connected and a 15-frame rest baseline was
  captured on 2026-08-27; no bumper or cliff stimulus was applied.
- Camera, SmartControl keepalive, USB tether, USB audio, local control token,
  and `POST /api/v1/audio/play` gating have older live receipts from
  2026-08-23.
- Factory voice protocol is decoded; `/api/v1/voice` still reports
  `live_confirmed: false`. No live frame. Do not touch `Name.dat` (`AGENTS.md`).
- No motor/drive command path. `hombotd` sends keepalive only.
- No confirmed serial console.

Tree work after those receipts (Voice-Telemetry 0.1.11, C2 page) is not a device
receipt. `STATUS_LIVE.md` already says so.

## A — Sidecar / Stage 2 (next stage, not the product)

Roadmap Stage 2 is local authenticated control: semantic allowlist, exclusive
lease, heartbeat, stop on disconnect, and cliff / wheel-drop / bumper /
battery / transport interlocks (`docs/ROADMAP.md`, `docs/ARCHITECTURE.md`).
`hombotd` must not open the productive Micom UART in parallel with `rpmain`
(`docs/ARCHITECTURE.md`, `docs/HARDWARE.md` UART1 at 230400 baud).

**What already goes.** Observe-only sidecar is real: camera stream, read-only
SmartControl, token-gated audio play, USB tether/audio modules `insmod`'d
(`STATUS_LIVE.md`). UART1 staying with `rpmain` is the right boundary.

**What blocks.** There is still no motor path. Lease, heartbeat and interlocks
are unspecified in running code. On the broker wire, `hombotd` decodes 4 of
158 `RawSensor` bytes (`legacy_level`, `voltage_raw_centivolts`,
`battery_aux_raw`, `charger_state_raw`). The function that maps that frame to
in-memory `SensorData_t` has not been found (`docs/SENSOR_INVENTORY.md`).
Without cliff / bumper / wheel-drop on the wire, allowlist driving is guessing,
not control.

**Effort (coarse).** Weeks of protocol/sensor work before a first guarded
nudge, not a UI weekend. The missing bridge is a firmware-analysis problem,
not a dashboard problem.

**Risk.** Safety: motion without interlocks. Brick: still no confirmed UART0,
so a bad `rc.local` or module load remains hard to recover (`AGENTS.md`,
`STATUS_LIVE.md`).

**Hold / sharpen / drop.** Sharpen. Do not skip. Do not sell it as the product.

## B — Companion as brain (target architecture)

`docs/ARCHITECTURE.md`: heavy CV, SLAM, object recognition and long-term
logging belong on a current companion host. The robot is a real-time chassis,
sensor and actuator gateway. `docs/ROADMAP.md` Stage 3 says the same.

The attested compute path is the existing operator machine plus the measured
USB-tether link (`STATUS_LIVE.md`). The robot has one EHCI root port; WLAN
alone reports 450 mA of a ~500 mA budget (`AGENTS.md`, `docs/HARDWARE.md`).
A second unpowered USB device already falls off the bus. No additional board
is named here because none is documented in-tree.

**What already goes.** Network path to a host is real (WLAN and RNDIS tether).
Camera and status can feed a host. Architecture already draws the split.

**What blocks.** Without Stage-2 stop (lease, heartbeat, interlocks on the
robot, independent of the navigation process), the companion cannot be the
safety authority. Architecture is explicit: the stop path must not live only
in the external AI process. USB power forbids casually hanging a companion
SBC off the robot's only port.

**Effort (coarse).** Host software can start whenever Stage 2 exists. Until
then, companion work is perception against a read-only stream.

**Risk.** Safety: a host that "drives" through an API without robot-side
interlocks. Brick: low, if it stays off-device. Demo risk: high, if C2 or a
host UI is shown as live control.

**Hold / sharpen / drop.** Hold as the target. Not today's product. Not a
jump over A.

## C — Full firmware on the NXP2120 (not the product)

Roadmap Stages 4–5: UART0 identified and verified, U-Boot and NAND/OOB
recorded, RAM-only BusyBox/initramfs via YMODEM, no persistent flash writes
until that recovery exists. A mainline kernel is called out as a separate
board-support project (`docs/ROADMAP.md`).

**What is not demonstrated.** UART0 is not live. CN14 is a candidate header;
3.3 V logic is expected and unmeasured (`docs/HARDWARE.md`, `STATUS_LIVE.md`).
H.264 is declared in `nx_chip_p2120.h` and has no driver. Micom safety stays
on UART1; replacing `rpmain` means owning that path.

**What already goes, as recovery only.** U-Boot YMODEM and a short boot delay
are documented. The USB `root_update.sh` hook has saved this device once
(`AGENTS.md`). Stage 4 (UART0 + RAM boot + NAND/OOB restore) remains the
sensible recovery track.

**Effort (coarse).** Product-grade replacement firmware is a BSP: months to
years, not a release train on 0.1.x. Recovery Stage 4 is a finite hardware
bring-up once UART0 is actually found.

**Risk.** Brick: writing flash before UART0 and NAND/OOB restore is exactly
the failure mode the roadmap forbids. Safety: a homegrown motion stack that
bypasses Micom interlocks.

**Hold / sharpen / drop.** Drop as the product. Keep Stage 4 UART0 + RAM boot
as recovery. Do not start persistent firmware changes.

## What a demo may say

Say: the robot runs `hombotd` 0.1.10 as a sidecar; companion-as-brain is the
architecture; Stage 2 (allowlist + interlocks) is the next build; full
replacement firmware on this NXP is not the plan.

Do not say: we have drive, we have a new board, we have a console, we have
live voice bearing, or we have a new OS on the SoC.
