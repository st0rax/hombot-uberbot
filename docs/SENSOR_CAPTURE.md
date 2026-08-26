# Sensor capture — bumper and cliff

Bitflüsterer's live-session protocol. Read-only HTTP against the running
device. No dumps in this repository. This file does **not** decode the
158-byte wire frame; mapping lives in `docs/SENSOR_INVENTORY.md` and stays
unknown until a capture plus firmware evidence agree.

Device: `hombotd` **0.1.10** (see `STATUS_LIVE.md`).
Endpoint: `GET /api/v1/sensors` (`raw_record_hex`).
Tabu: UART1, `Name.dat`, Jig, Smart Diagnosis, driving, docking for cliff,
flash writes, decoder edits.

Supply `HOMBOT_HOST` from your own environment (`docs/OPERATOR_TOOLS.md`).
Do not put the address in this repository.

## Gate (abort if any check fails)

1. `GET /api/v1/status` — `version` is `0.1.10`, robot is not driving.
2. `GET /api/v1/sensors`:
   - `available` is `true`
   - `raw_record_size` is `158`
   - `raw_record_hex` is 316 hex characters
   - `age_ms` is `< 2000`
3. If `state` is `disabled`, the endpoint is 404, or hex is null: **stop**.
   Enabling `HOMBOTD_RAWSENSOR` is a device change, not this session.

Poll about 2 Hz during a trial so `age_ms` stays under 2000. Save the full
JSON line, not a trimmed hex snippet.

## Labels

Firmware zone words (FL/FC/FR) are **not** confirmed. Name files by the
physical part you touch.

| File | Stimulus |
|---|---|
| `01_baseline.jsonl` | rest |
| `02_bumper_L.jsonl` | left bumper only |
| `03_rest.jsonl` | return to rest |
| `04_bumper_C.jsonl` | center bumper only |
| `05_rest.jsonl` | rest |
| `06_bumper_R.jsonl` | right bumper only |
| `07_rest.jsonl` | rest |
| `08_cliff_L.jsonl` | left downward window only |
| `09_rest.jsonl` | rest |
| `10_cliff_C.jsonl` | center downward window only |
| `11_rest.jsonl` | rest |
| `12_cliff_R.jsonl` | right downward window only |
| `13_baseline_end.jsonl` | rest again |

Between every stimulus, record 8–10 rest frames.

## Baseline

Robot still. Wheels on a solid surface, or the chassis fully held. Bumpers
free. Cliff windows seeing floor or table. Not rolling.

If it is on the dock: bumper trials may run on-dock (note `robot_state`).
Cliff trials are **off-dock**, robot **held**. A docked cliff window may see
the station. Picking the robot up will likely move battery bytes 4–9; that
is contamination for those bytes, expected, ignore them for bumper/cliff.

Record about 15 frames for `01_baseline.jsonl`.

## Bumper

Press one bumper segment only. Hold 3–4 s, at least 8 frames. No driving.

## Cliff (held, never driven)

100% of the weight in hands or on a table. Never over an edge without grip.
Slide **one** cliff window past a table edge, or cover **one** window with a
matte card. No drop, no drive, no dock, no diagnosis.

Optional later, not required: one wheel slightly unloaded while supporting
the chassis (`wheel_drop`) — only if that does not start motion.

## Diff (after the session)

Per stimulus file versus the nearest rest/baseline:

1. Take the modal `raw_record_hex` (most common 316-character line).
2. Bytewise XOR. List index, before, after.
3. **Control, not signal:** bytes 4–9 (`legacy_level`, voltage, aux,
   charger) are the known battery/charger fields in
   `hombotd/src/rawsensor.rs`. They are **not** the bumper/cliff signal. If
   only those bytes move, the trial is bad (undock or charge). If they move
   *and* others move, still score the others.
4. **Honest expectation:** some of bytes 0–3 or 10–157 may flip. Which
   ones: **unknown**. Do not name them.
5. One stimulus should not also flip the other type (bumper trial without
   cliff-looking bits, and the reverse). If both flip, the physical action
   mixed them (lifting during bumper, etc.) — discard the trial.

Do not edit `rawsensor.rs` from a XOR list. A wire name goes into the
inventory only when capture and `DasPublishSensorRawData` (or an equivalent
firmware receipt) agree.

## Do not

- Drive.
- Dock for cliff.
- Run Smart Diagnosis (it can move the robot).
- Open UART1, Jig, or `Name.dat`.
- Check dumps into the repository.
- Invent wire field names or land a decoder from this protocol.
