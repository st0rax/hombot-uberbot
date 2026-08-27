# Motion readiness gate

This is the mandatory evidence gate before any command may cause physical
movement. The device owner authorizes the agent to release this lock
autonomously, but only when **every** mandatory row below is fresh and
positive. Failed, unknown, unavailable or stale is a failed gate. There is no
"probably safe" state.

Passing the gate removes the need for per-movement owner confirmation while
the evidence remains valid. It does not remove any technical safety control.
Any relevant change or fault closes the gate immediately.

## Evidence record

Record each live result in `STATUS_LIVE.md` with:

- timestamp and deployed version or revision;
- exact test or stimulus and observed result;
- the interface used to observe it;
- pass/fail, plus any measurement or artifact needed to reproduce the claim.

A source-code test is not a live-device receipt. An owner report is useful
context but does not substitute for a live sensor stimulus test.

## Gate A: known and recoverable baseline

- [ ] The deployed `hombotd` version and configuration are known.
- [ ] Key-based SSH and the rollback path work without changing boot files.
- [ ] `/usr/etc/rc.local` is still executable.
- [ ] No pending deployment, reboot or maintenance can invalidate the test run.
- [ ] Original LG/Micom safety authority remains active.

Any boot-time or firmware change still follows the stricter recovery rules in
`AGENTS.md`. Passing this gate is not permission to flash firmware.

## Gate B: non-motion function tests

- [ ] Every implemented non-motion function in the deployed build has a
      current positive test or is documented as intentionally disabled and
      irrelevant to motion safety.
- [ ] Health, system and SmartControl status endpoints return current data.
- [ ] Camera capture works in every supported mode used by the operator path.
- [ ] RawSensor reports connected, available and fresh data continuously.
- [ ] Authentication is tested both negatively and positively for every
      write-capable endpoint.
- [ ] Required audio capture/playback paths pass when they are part of the
      intended session; unavailable optional hardware is explicitly excluded.
- [ ] Resource limits, watchdog behavior and bounded connection handling pass.
- [ ] Logs show no unexplained service restart, transport fault or stale feed.

## Gate C: safety-sensor stimulus tests

The sensor inventory must identify the live field and active state for every
chassis or vendor-motion sensor that can signal contact, obstacle, cliff,
lift, wheel state, docking or a relevant power condition. Exercise each
sensor independently and observe transition, release and freshness. At
minimum:

- [ ] Every bumper transitions when pressed and returns when released.
- [ ] Every cliff sensor responds to a safe, controlled stimulus and recovers.
- [ ] Wheel-drop or lift detection responds and recovers, where fitted.
- [ ] Cover/chassis contacts used by the safety policy respond and recover.
- [ ] Charger/dock/power state used by motion policy is correctly identified.
- [ ] Simultaneous or contradictory safety inputs fail closed.
- [ ] A frozen or stale sensor stream is detected and causes stop/inhibit.

A physically blocked bumper, unmapped field, intermittent transition or
ambiguous polarity fails this gate.

## Gate D: control-path safety tests

- [ ] Commands use an authenticated semantic allowlist. Raw frames, arbitrary
      memory writes, shell commands and uploaded executables are impossible.
- [ ] Only one controller can hold an exclusive short-lived motion lease.
- [ ] Motion requires a frequent heartbeat; expiry produces an on-device stop.
- [ ] Client disconnect produces an on-device stop.
- [ ] SmartControl or other transport loss produces an on-device stop.
- [ ] A safety-sensor trip during a command produces an on-device stop.
- [ ] Stale sensor state inhibits a new command and stops an active one.
- [ ] The stop path does not depend solely on the external Uberbot/WebAgent
      process or its network connection.
- [ ] Stop latency is measured and below the documented bound for the first
      motion envelope.

These tests must use the same deployed binary, configuration and interfaces as
the proposed motion test. A mocked transport can support development but does
not pass the live gate.

## Gate E: controlled first-motion envelope

After Gates A-D pass, define and record the first envelope before executing
it:

- clear, stable test area with no stairs, people, animals or loose cables;
- lowest practical speed;
- shortest practical duration or distance;
- one simple direction with no autonomous exploration or docking;
- a known automatic stop bound plus an independent reachable stop path;
- live observation of safety sensors, lease and heartbeat throughout.

Execute only that smallest bounded action first. Confirm the expected stop and
post-motion sensor state before expanding speed, duration, direction or task
complexity. Each expansion is a new envelope and must remain inside the tested
safety bounds.

## Self-release and automatic re-lock

The agent may mark the gate **PASS** in `STATUS_LIVE.md` and begin the bounded
first-motion test without asking for a new owner confirmation only when all
mandatory items above have fresh positive evidence.

The gate automatically returns to **LOCKED** on any of the following:

- a failed, unknown, unavailable, contradictory or stale prerequisite;
- a new deployment, reboot, configuration change or relevant hardware change;
- bumper, cliff, wheel-drop, cover or transport behavior outside its verified
  mapping;
- failure or uncertainty in authentication, lease, heartbeat or stop behavior;
- inability to maintain the controlled test envelope.

When re-locked, no movement-capable command is sent until the affected checks
are repeated and pass. `STATUS_LIVE.md` is the authority for the current gate
state; roadmap intent alone never opens it.
