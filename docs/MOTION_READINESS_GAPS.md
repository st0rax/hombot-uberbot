# Motion-readiness gap ledger

Snapshot: 2026-08-27, during the owner-reserved offline maintenance window.
This is a repository and receipt audit, not a live-device measurement. The
authoritative current state remains `STATUS_LIVE.md`.

## Current decision: LOCKED

The gate cannot release. The owner reports blocked bumpers, the 158-byte
RawSensor record is only partially decoded, and no motor path, motion lease,
heartbeat or independently tested stop path exists. A green host test suite
does not override those live and implementation gaps.

## What is already useful

- The deployed version, dedicated key-based SSH receipt and executable
  `rc.local` receipt are documented from 2026-08-27.
- RawSensor freshness already fails closed in the API after two seconds.
- Authentication tests cover missing, wrong and matching control tokens.
- The current C2 page has no motor console or motor endpoint.
- The Rust host suite passes 58 tests on 2026-08-27.
- `tools/operator/motion_readiness_gate.py` now evaluates all required receipts
  as one session and exact deployed version. It cannot access the robot.

These facts reduce uncertainty but do not constitute a complete Gate A-D run.

## Blocking gaps by gate

| Gate | Known state | Required next work |
| --- | --- | --- |
| A: baseline | Some dated receipts exist; maintenance is in progress and no serial console is confirmed. | After maintenance, recheck exact deployed version, SSH/rollback, executable `rc.local`, LG/Micom authority and absence of pending changes in one session. |
| B: functions | 58 host tests pass. Live receipts have mixed dates; bounded connections, watchdog and a full deployed-function inventory are not proven. | Build the inventory, close host-side limit/watchdog gaps, then run every implemented non-motion function on one deployed build. |
| C: sensors | RawSensor is live but only four of 158 bytes are decoded. No bumper/cliff stimulus capture exists; bumpers are owner-reported blocked. | Repair bumpers, capture independent press/release baselines, map all motion-safety fields, then test bumper, cliff, lift/wheel-drop, cover, dock/power, thermal, contradictory and stale states. |
| D: control safety | No motor path exists, which prevents accidental motion today. Lease, heartbeat and motion-stop interlocks are not implemented. | Implement and exhaustively host-test a fail-closed safety state machine before connecting it to any actuator; live-test all stop causes before a motion envelope can open. |
| E: first envelope | Not assessed during offline maintenance. | Define a clear area, minimum speed/duration, automatic bound and independent stop path after Gates A-D pass. |

## Safe implementation order

1. Keep actuator commands absent.
2. Implement a pure, deterministic safety state machine for lease, heartbeat,
   freshness and interlock decisions; test every failure transition offline.
3. Finish non-motion function inventory and host-side limits/watchdog tests.
4. After maintenance, capture and map safety-sensor stimuli without movement.
5. Wire read-only live sensor state into the safety state machine and prove it
   remains fail-closed with actuator output still absent.
6. Discover and document the smallest semantic stop/motion path without
   exposing raw frames or a public endpoint.
7. Add the actuator adapter behind authentication, the readiness gate and the
   already-tested state machine.
8. Run the complete fresh Gate A-E receipt set. Only a fully green evaluator
   result permits the first bounded motion.

This order allows productive development while the robot is offline and makes
the eventual movement path the last component attached, not the first.
