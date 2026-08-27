# Uberbot integration roadmap

## Purpose

This roadmap combines the independently maintained `webagent-rs` agent runtime
with the HomBot body implemented by `hombotd`. It complements the
device-specific [`ROADMAP.md`](ROADMAP.md); it does not replace it.

The architectural principle is separation of:

- **Soul** -- agent loop, Brain providers, memory and research supplied through
  `webagent-rs` interfaces;
- **Body** -- discovered hardware and semantic operations supplied first by
  the HomBot adapter;
- **Interface** -- the versioned, permissioned contracts through which the
  integration runtime observes and acts.

The HomBot is the first serious body target, not the definition of Uberbot.
The two source projects remain independently buildable and releasable. See
[`PROJECT_BOUNDARIES.md`](PROJECT_BOUNDARIES.md).

## Present truth

**Status: boundary documented; integration runtime not implemented.**

- The physical robot runs `hombotd 0.1.10`; `STATUS_LIVE.md` is authoritative.
- `webagent-rs` is not deployed on the HomBot.
- No Brain-to-Body contract is active.
- No motor command path exists.
- Host-side integration may begin read-only before HomBot device-roadmap Stage
  2, but motion cannot.

## Integration milestones

### I0 -- Independent-project contract

**Goal:** make ownership, evidence and versioning unambiguous.

- Keep separate repositories, releases and CI pipelines.
- Record authoritative responsibilities in `PROJECT_BOUNDARIES.md`.
- Keep device and integration roadmaps separate.
- Define evidence labels that prevent plans from appearing as live features.

**Exit criterion:** a contributor can identify which project owns every major
component and can reproduce the exact revisions used by an integration build.

### I1 -- Read-only contract spike on a companion host

**Goal:** prove the seam before building a large runtime.

- Define a minimal versioned Brain interface backed by the documented
  [`webagent-rs` API bridge](https://github.com/st0rax/webagent-rs/blob/master/docs/API_BRIDGE.md)
  or another explicitly released service boundary.
- Define a minimal read-only Body interface for health, system, camera, audio,
  SmartControl status and brokered sensor observations.
- Create captured fixtures from already documented payload shapes; fixtures are
  contract evidence, not live-device evidence.
- Preserve unknown, stale and unavailable states explicitly.
- Authenticate every non-loopback boundary; do not expose credentials through
  either API.

**Exit criterion:** a host-side probe can query a pinned `hombotd` contract and
present its capabilities without changing the robot.

### I2 -- Portable integration runtime

**Goal:** create an Uberbot runtime that is neither `webagent-rs` nor
`hombotd`.

Target modes:

```text
uberbot --headless
uberbot --web
uberbot --service
```

- Start on Linux x86_64 or another capable development host.
- Keep Brain, Body and storage providers pluggable.
- Use structured logs, evidence records and bounded resource use.
- Make the browser a client/UI, not the agent identity.
- Fail usefully when either source project is absent or incompatible.

**Exit criterion:** the runtime starts without HomBot-specific assumptions and
exposes its state locally while using pinned interface versions.

### I3 -- Capability graph and HomBot body adapter

**Goal:** turn existing HomBot observations into a self-describing body.

- Model capabilities, constraints, provenance, confidence, freshness,
  permissions and safety classes.
- Implement the HomBot adapter without moving device protocol code out of
  `hombotd`.
- Separate self model (available tools/resources) from environment model
  (camera, audio, peers and task observations).
- Treat a discovered actuator as non-authorized until an explicit permission
  and safety contract exists.

**Exit criterion:** the web UI shows a useful read-only capability inventory
whose values can be traced to `hombotd` responses or explicit unknown states.

### I4 -- WebAgent Brain provider

**Goal:** use `webagent-rs` as one replaceable reasoning provider.

- Connect through its documented authenticated service/API boundary instead of
  reading its internal run store or browser profiles.
- Bind every agent run to the exact WebAgent and Uberbot versions used.
- Store prompts, observations and decisions with an audit trail that excludes
  credentials and private browser-session material.
- Keep single- and multi-Brain behavior behind the same provider contract.

**Exit criterion:** a harmless read-only task can travel from Uberbot to a
pinned WebAgent provider, return a plan and be recorded without directly
changing the body.

### I5 -- Observe, model and plan loop

**Goal:** close the agent loop without granting actuation.

```text
Observe -> update self/environment model -> plan
        -> permission check -> explain proposed action -> record evidence
```

- Begin with read-only observations and simulated or rejected actions.
- Use semantic capability identifiers, not generic shell access.
- Detect stale observations, unavailable providers and version mismatches.
- Research unknown capabilities only through explicitly safe evidence sources
  and experiments.

**Exit criterion:** the integrated system can explain what it sees, what it
could do, why it cannot yet act and what evidence is missing.

### I6 -- Bounded semantic actions

**Dependency:** HomBot device-roadmap Stage 2 must be completed and live
verified for the specific action class.

- Start with an already bounded action such as authenticated audio playback.
- Require explicit user permission, an allowlist, request bounds and freshness
  checks.
- Motion additionally requires an exclusive lease, heartbeat, independent
  robot-side stop behavior and live-confirmed cliff, wheel-drop, bumper,
  battery/power and transport interlocks.
- Never give the Brain a raw LG-frame sender, unrestricted shell or direct
  Micom transport.

**Exit criterion:** each enabled semantic action has a dated device receipt,
negative authorization tests and a reliable stop or rollback path.

### I7 -- Memory consolidation and evidence maintenance

**Goal:** maintain knowledge without turning repetition into truth.

- Preserve provenance, timestamps, confidence and component versions.
- Detect contradictions and stale procedures.
- Deduplicate records without deleting their evidence trail.
- Convert missing or inconsistent knowledge into explicit research tasks.

**Exit criterion:** the runtime can identify that a previously known procedure
is now stale or contradictory without inventing a replacement.

### I8 -- Distributed and portable embodiment

**Goal:** validate that Uberbot is more than one robot integration.

- Treat remote capabilities as remote, versioned and permissioned.
- Keep heavy vision, storage and Brains on capable hosts when ARMv6 resources
  are insufficient.
- Test the same runtime against an unrelated safe host or simulated body.
- Consider direct ARM/ARMv6 execution only where the resource and recovery
  evidence supports it.

**Exit criterion:** the integration runtime can start on an unrelated supported
host, discover capabilities, distinguish known from unknown and operate within
explicit permissions without HomBot assumptions in its core.

## Dependency map

| Integration work | May start now | Requires HomBot device Stage 2 |
|---|---:|---:|
| Repository/interface design | Yes | No |
| Read-only fixtures and contract tests | Yes | No |
| Capability graph and read-only HomBot adapter | Yes | No |
| WebAgent Brain provider | Yes | No |
| Observe/model/plan with simulated actions | Yes | No |
| Authenticated audio playback integration | After a dedicated live recheck | No motion dependency |
| Any motion command | No | Yes, including live interlock evidence |

## Integration definition of done

Uberbot is successful when a fresh supported companion host can run the
integration runtime, connect to explicit compatible versions of WebAgent and a
body provider, construct an auditable capability model, reason about its own
limitations, and use only permissioned semantic operations. The HomBot must
retain its existing independent build, deployment, rollback and original
safety authority throughout.
