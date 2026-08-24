# Sensor inventory — work in progress

Requested: a complete listing of the robot's sensors, built by reading the
firmware disassembly rather than guessing. This is not complete yet. Every
line below is either a measured offset (with the disassembly address it came
from) or explicitly marked as not yet found, per `AGENTS.md`'s evidence rule.

## Two different layouts, do not conflate them

There are two distinct data shapes in play and only one is currently decoded:

1. **The wire frame** — the 158-byte `RawSensor` payload (DAS service 110,
   topic 105, message ID `0x0304`) that travels over the LGRP broker
   connection. `hombotd/src/rawsensor.rs` decodes 4 of these 158 bytes today
   (`legacy_level`, `voltage_raw_centivolts`, `battery_aux_raw`,
   `charger_state_raw`).
2. **The in-memory `SensorData_t` struct** — what `rpmain`'s own C++ code
   works with internally, built from `CDataAccessServiceMessage::DasGetSensorData()`.
   This is a *different* layout (its own field offsets), consumed by
   `CMapBuilder`, `CBasicMove`, `CWallFollowing`, `CBehaviorDocking`, etc.

**The bridge between the two — the function that parses the raw 158-byte wire
frame into a `SensorData_t` (or vice versa, the one that serializes
`SensorData_t` into the 158-byte wire frame for `DasPublishSensorRawData`) has
not been located yet.** No direct `bl` call sites to
`CDataAccessServiceMessage::DasPublishSensorData(SensorData_t*)` (`0x1a958`)
or `DasPublishSensorRawData(SensorRawData_t*)` (`0x1a9b4`) turned up in
`rpmain_full.disasm.txt` — they are very likely reached through a function
pointer / vtable dispatch (message-handler tables are used throughout this
codebase, e.g. `AServiceMessage::SendMessage`), which plain `grep` for `bl
<addr>` cannot find. This needs either a proper vtable walk or reading how
`CDataAccessService`'s own message-dispatch table is built.

## Confirmed: `SensorData_t` in-memory field offsets

Source: `CMotionService::PrintSensorData(AServiceMessage*)` at `0x30994` in
`rpmain.axf`, a debug-print function gated by a `#if 0`-style toggle (it only
fires every 50th call and only when a debug flag at a fixed address is
nonzero). It calls `CDataAccessServiceMessage::DasGetSensorData()`, then reads
fixed offsets off the returned pointer (`r4`) directly into an
`AService::Print(char const*, ...)` varargs call. This is exactly the pattern
that decoded the voice-service message formats earlier in this project — a
formatted debug print naming its own fields.

Confirmed 32-bit fields (`ldr`, so 4-byte-aligned words):

| Offset | Access | Likely meaning (from print grouping) |
|---|---|---|
| `0x0c` | `ldr r1, [r4, #0xc]`  | pose X (first `%4d` in the pose group) |
| `0x10` | `ldr r2, [r4, #0x10]` | pose Y (second `%4d`) |
| `0x14` | `ldr r3, [r4, #0x14]` | pose theta (`%5d`, wider field matches an angle in millidegrees) |

Confirmed 16-bit signed fields (`ldrsh`, halfword offsets, all within one
contiguous run):

| Offset | Access |
|---|---|
| `0x20` | `ldrsh r3, [r4, #32]` |
| `0x22` | `ldrsh r2, [r4, #34]` |
| `0x24` | `ldrsh r0, [r4, #36]` |
| `0x26` | `ldrsh r1, [r4, #38]` |
| `0x28` | `ldrsh r0, [r4, #40]` |
| `0x2a` | `ldrsh r1, [r4, #42]` |
| `0x2c` | `ldrsh r2, [r4, #44]` |
| `0x2e` | `ldrsh r1, [r4, #46]` |
| `0x30` | `ldrsh r2, [r4, #48]` |

The format string these feed (`0x270c34`, verified byte-for-byte in
`rpmain_full.disasm.txt`):

```
(%4d,%4d,%5d) P(%3d,%3d) S(%3d,%3d, %d, %d) C(%d,%d,%d) D(0x%x, 0x%x)
```

Reading the groups against the register wiring: the first parenthesis is the
pose triple above (`0x0c`/`0x10`/`0x14`). `P(...)` and `S(...)` consume the
nine `ldrsh` halfwords at `0x20`-`0x30` between them — most likely PSD
(position-sensitive-detector / cliff IR) and bump/wall-sensor short values,
given every consumer of `SensorData_t*` found in `symbols_all.txt` includes
`CMotionCliff`, `GeneratePSDObstaclePoint`, `GenerateUSSObstaclePoint`, and
`GenerateBumpObstaclePoint`. The exact per-field mapping (which of the 9
shorts is left-PSD vs right-PSD vs front-bump, etc.) is **not** confirmed —
the call only shows register wiring, not which named field lines up with
which `%3d`/`%d` slot, because `AService::Print` is a plain varargs call and
the disassembly window captured so far cuts off before all arguments needed
for `C(...)` and `D(0x%x, 0x%x)` are wired (those come from stack slots beyond
`sp`+the two visible `str`s). The `C(...)` and `D(...)` fields are not yet
traced to offsets at all.

## Known consumers of `SensorData_t*` (offsets not yet extracted from these)

From `work/firmware_re/symbols_all.txt`, everything below takes a
`SensorData_t*` or `SensorData_t const*` and is worth mining the same way
`PrintSensorData` was mined — a print/log function is the fastest way in,
computational ones (bump/cliff detectors) are slower but more precise:

- `CMapBuilder::GenerateBumpObstaclePoint`, `GenerateCliffObstaclePoint`,
  `GenerateUSSObstaclePoint`, `GeneratePSDObstaclePoint`
- `CMotionCliff` (whole class)
- `CStateHoming::HandleEmergencyWheelDrop`, `HandleEmergencyDustBin`
- `CEventService::HandleDASBumper`, `HandleDASWheelDrop`
- `CBehaviorDocking::UpdateDockingIRValue(SensorData_t*)`
- `CBasicMove`, `CLineFollowing`, `CMotionService`, `CWallFollowing`
- `CBehaviorStop::HandlePublishDASSensorData`,
  `CBehaviorGeneric::HandlePublishSensorData`,
  `CBehaviorComeHere::HandlePublishDASSensorData`,
  `CBehaviorRecovery::HandlePublishDASSensorData`,
  `CBehaviorSearching::HandlePublishDASSensorData`
- `CBehaviorDiagnosis::UpdateRobotSensorData`
- `CHomingPointer::HandlePublishSensorData`
- `CBehavior::HandlerPublishSensorData(AServiceMessage*, XYThetaInt32_t*)`

`CJigServiceMessage::JigSendFakeRawSensorData(SensorRawData_t*)` at `0x1a0bc`
confirmed the wire-side facts already in `rawsensor.rs`: it sends a
158-byte (`mov r2, #158`) buffer via `AServiceMessage::SendMessage(772, 0, ...)`
— `772` decimal is `0x0304`, matching `RAW_SENSOR_MESSAGE_ID`. It does not
itself construct the buffer's contents, so it did not add new field offsets.

## Attempted: `CMapBuilder::GenerateCliffObstaclePoint(SensorData_t*, MapPoint_t*, int)`

Disassembled at `0x8880c` as the next candidate (safety-relevant, expected to
read a small distinctive set of fields with visible threshold comparisons).
Result: the signature is misleading for register-reading purposes. The real
calling convention here puts the implicit `this` (a `CMapBuilder*`) in `r0`,
so the first ~130 disassembled instructions read `CMapBuilder`'s own member
config -- per-cliff-sensor enable bitmasks at `this+0xb8`/`this+0xbc` (tested
bit-by-bit per sensor index) and VFP double-precision threshold pairs at
`this+0xa0`/`this+0xa4`/`this+0xa8`/`this+0xac` -- not `SensorData_t` fields.
The actual `SensorData_t*` argument (real `r1` after the `this` shift) is
**never dereferenced anywhere in this function.** Read through to its end
(`0x88d1c`): the whole body is `CMapBuilder` building a fixed geometric fan of
candidate obstacle points around the robot from its own per-sensor-index
threshold config (`this+0xa0` through `this+0xc0`, all VFP double compares
against fixed offsets like `+100`, `+200`, `-100`, `-200`, `-300`) -- it never
reads an actual cliff *reading*. Whatever decides *whether* to call this
function for a given sensor index must be the caller, using the real
`SensorData_t` cliff bits -- this function only shapes geometry once told a
cliff fired. Dead end for finding the raw field offsets; worth remembering
only so a future pass does not re-disassemble it expecting sensor reads.

## Confirmed: `SensorData_t` field meanings (via `CMapBuilder::SensorDataHandler`)

`CMapBuilder::SensorDataHandler(SensorData_t*)` at `0x89bd0` is the real find
of this pass -- it is the top-level entry point that receives a live
`SensorData_t*` (not a message-wrapped copy) and dispatches into the
`Generate*ObstaclePoint` family. Because it reads fields directly off the
struct pointer with no wrapper offset, its field numbering is the ground
truth; it also lines up exactly with `PrintSensorData`'s numbering once you
subtract the 12-byte `AServiceMessage` header that call site added on top:

| `SensorData_t` offset | Width | Access | Confirmed meaning |
|---|---|---|---|
| `0x00` | 32-bit | `ldr r0,[r6]`   | odometry delta X -- diffed against `CMapBuilder`'s stored previous pose (`this+0x18`) every call |
| `0x04` | 32-bit | `ldr r0,[r6,#4]` | odometry delta Y (vs. `this+0x1c`) |
| `0x08` | 32-bit | `ldr r0,[r6,#8]` | odometry delta theta (vs. `this+0x20`) |
| `0x20` | u16 | `ldrh r3,[r6,#32]` | zone-status word 1 -- `tst r3,#1` gates `CMapBuilder`'s cliff-enable flag for zone 1 |
| `0x22` | u16 | `ldrh r3,[r6,#34]` | zone-status word 2 -- same, zone 2 |
| `0x24` | u16 | `ldrh r3,[r6,#36]` | zone-status word 3 -- same, zone 3 |

**Correction to a first read of these three fields:** `GenerateBumpObstaclePoint(MapPoint_t*, SensorData_t*)`
(`0x883ac`) reads the *same* three offsets (`0x20`/`0x22`/`0x24`) and ORs them
together, then masks with `& 2` (bit 1, not bit 0) to decide whether *any*
bump fired. Combined with `SensorDataHandler` masking bit 0 of each field
individually per zone for cliff, this means these three `u16` fields are not
one-sensor-per-field -- **each is a per-zone bitmask register** (bit 0 =
cliff triggered in that zone, bit 1 = bump triggered in that zone, meaning of
further bits not yet determined) covering three physical zones, most likely
front-left / front-center-or-right / a third zone given the robot's known
sensor placement. The remaining six `ldrsh` halfwords found earlier at
`0x26`-`0x30` were *not* touched by either function disassembled this pass --
still open, most likely PSD (cliff IR distance) or USS (ultrasonic) raw
values given `GeneratePSDObstaclePoint`/`GenerateUSSObstaclePoint` are the
next two dispatch targets in `SensorDataHandler` right after the bump/cliff
zone checks.

## Partial: `GeneratePSDObstaclePoint` touches offset `0x2e`

`CMapBuilder::GeneratePSDObstaclePoint(SensorData_t*, MapPoint_t*, int)`
(`0x89760`) reloads the `SensorData_t*` argument from its stack spill area
and reads `ldrsh r11, [r0, #46]` (offset `0x2e`) as essentially its first
action -- that value then gates the function's main loop (`cmp r11,#0` /
`ble`, skip entirely if non-positive). This is consistent with `0x2e` being a
PSD reading count or a signed distance/threshold, matching its position in
`PrintSensorData`'s `S(...)` print group. The function is heavily
VFP-floating-point from there on (geometry projection, not more raw field
reads in the portion examined) -- confirming the remaining offsets
(`0x26`/`0x28`/`0x2a`/`0x2c`/`0x30`) needs either more of this same function
or `GenerateUSSObstaclePoint`, not yet done this pass.

## Confirmed (second pass): USS and PSD channel offsets

`CMapBuilder::GenerateUSSObstaclePoint(SensorData_t*, MapPoint_t*, int)`
(`0x8944c`) read in full:

| `SensorData_t` offset | Width | Evidence | Meaning |
|---|---|---|---|
| `0x26` | s16 | `ldrsh r2,[r7,#38]` (entry) and re-load at `0x89530` | ultrasonic distance #1 -- compared against a range cap built from `this+0xd4 ? 225 : min(this+0x8c+40, 240)` |
| `0x28` | s16 | `ldrsh r11,[r7,#40]` at `0x894ac`, re-load at `0x89590`/`0x89648` | ultrasonic distance #2 -- same threshold family |

Debounce pattern: each USS sub-channel has a 2-of-N counter in a
`CMapBuilder` table at `0x2e4180` (fields `+0x0/+0x4/+0x8`); only the second
consecutive hit produces a `MapPoint_t` write (`str r1,[r6,#8]`,
`[r6,#0x14]`, `[r6,#0x20]`). Sensor index comes in as the `int` argument
(`r3`), results land at `MapPoint+0x8+r5*12`.

`CMapBuilder::GeneratePSDObstaclePoint` (`0x89760`) completed:

| `SensorData_t` offset | Width | Evidence | Meaning |
|---|---|---|---|
| `0x2e` | s16 | `ldrsh r11,[r0,#46]` at `0x89780` | PSD reading channel 1 -- `>0` gates the projection loop, exactly as suspected |
| `0x30` | s16 | `ldrsh r11,[r0,#48]` at `0x89960` | PSD reading channel 2 -- same `>0` gate, second ray/zone |

Still unidentified in the `0x20..0x31` window: **`0x2a` and `0x2c`**.
A whole-binary search for `ldrsh [rX,#42]/[#44]` finds no other
`SensorData_t` consumer (hits at `0xeb7b4`/`0xecea0`/`0xee004` are GSM-DSP
routines, unrelated structs). Best remaining hypothesis: wall-IR pair,
consumed by `CWallFollowing` through helpers rather than direct halfword
loads.

## The struct is much larger than `0x32`

`CMotionService::MatchingProcess(SensorData_t const*, float, float, short)`
(`0x2fac8`) does `add r12, r0, #1280` and then reads halfword *pairs* at
`+0x500+36/+40` and `+0x500+38/+42` (`0x524/0x528`, `0x526/0x52a`),
comparing them against each other with VFP math and a match-flag byte at
`this+0x55c`; on match it calls
`CMotionService::MakeLocalMap(SensorData_t const*, float, float, short, short)`
(`0x2f9e4`). So the layout continues far past `0x30` with at least one
history/secondary section at `+0x500`. Any future field hunting should treat
the struct size as unknown-but-large, not 52 bytes.

A second extension sits right behind the known window:
`CMotionService::IsThereDoorFrame(short, SensorData_t const*, short)`
(`0x2fe14`) reads halfword pairs at `0x34/0x36`, `0x38/0x3a`, `0x3c/0x3e`
and feeds each pair into `MatchingProcess`;
`CMotionService::InitCheckDoorFrameVariables(SensorData_t const*)`
(`0x2feec`) writes defaults across `0x24..0x30` and `0x34..0x3c`. So offsets
`0x34..0x3f` hold three more s16 sensor pairs -- most plausibly the
door-frame / wall IR receivers used for docking-station recognition.
`0x2a`/`0x2c` remain the only unassigned halfwords in the first window.

Also recovered: the previously-missing second half of
`PrintSensorData`'s argument wiring (`0x30a24`-`0x30ac0`). The final three
stack slots are filled by `stm sp,{r1,r2,r3}` with `0x2e`, `0x30`, `0x26`
-- consistent with the meanings above landing in the tail of the format
string's numeric groups.

## Next concrete step

1. Disassemble `CMapBuilder::GeneratePSDObstaclePoint(SensorData_t*, MapPoint_t*, int)`
   (`0x89760`) and `GenerateUSSObstaclePoint(SensorData_t*, MapPoint_t*, int)`
   (`0x8944c`) -- called right after the bump/cliff zone checks in
   `SensorDataHandler`, so they are the best remaining candidates for the six
   still-unidentified `0x26`-`0x30` halfwords.
2. Separately locate the message-dispatch table for DAS service 110 to find
   the raw-158-byte-frame ↔ `SensorData_t` conversion function this document
   still cannot point to -- try walking the vtable/handler-table construction
   in `CDataAccessService`'s constructor rather than grepping for direct `bl`
   call sites, since none exist in the plain disassembly (dispatch is
   evidently indirect here, same as elsewhere in this codebase).
3. Identify `0x2a`/`0x2c`: disassemble
   `CWallFollowing::FollowLeftWall(SensorData_t const*)` (`0xb0340`) and
   `UpdateObstacleInformation` (`0xaf7c0`) looking for halfword access via
   computed offsets (the direct `ldrsh #42/#44` grep is exhausted).
4. Map the `+0x500` section: disassemble
   `CMotionService::MakeLocalMap(SensorData_t const*, ...)` (`0x2f9e4`) and
   `UpdateLocalMap1StepBefore(SensorData_t const*)` (`0x2fc18`), which also
   diffs pose words `[r7,#0xc/0x10/0x14]` against stored state.
5. Empirical cross-check (cheap, high yield): hombotd already receives live
   158-byte frames. Log them while physically triggering bumper/cliff/USS
   per zone and diff which bytes move -- this anchors the wire-frame layout
   even before the converter function is found statically.
