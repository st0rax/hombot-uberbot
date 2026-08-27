# Project boundaries

## Decision

Uberbot is a new integration system built from two independently useful
projects:

1. [`st0rax/webagent-rs`](https://github.com/st0rax/webagent-rs) supplies the
   general agent/Brain side.
2. The HomBot modernization in this repository supplies the first embodied
   target through `hombotd`, device research and recovery procedures.

The projects are combined at runtime and at their interfaces, not by merging
repositories. They keep separate histories, issue trackers, release numbers,
builds and definitions of done. This repository is also the initial home of
the new Uberbot integration track because it owns the HomBot adapter; that
does not make the HomBot sidecar dependent on WebAgent.

## Ownership map

| Concern | Authoritative project | Integration rule |
|---|---|---|
| Agent loop, Brain providers, Web sessions, agent memory and generic tool execution | `webagent-rs` | Consume a documented, versioned interface or released artifact. Do not fork these internals into `hombotd`. |
| Camera, audio, SmartControl, brokered sensors, ARM deployment and physical-device recovery | `hombot-uberbot` / `hombotd` | Remain usable and testable without any Brain connected. |
| Capability graph, Brain-to-Body contract, HomBot body adapter and cross-project end-to-end tests | Uberbot integration track in this repository | Depend on explicit versions of both sides and preserve their safety boundaries. |
| Motion and physical safety | HomBot device stack | The original `rpmain`/Micom safety path stays authoritative. An external Brain is never the only stop path. |
| Live device truth | `STATUS_LIVE.md` | A cross-project plan or passing host test is not a device receipt. |

## Repository rules

- Do not merge the `webagent-rs` Git history into this repository.
- Do not use an unpinned `main` or `master` checkout as a release dependency.
- Prefer versioned network/process contracts and released artifacts over copied
  source.
- If source reuse later becomes necessary, record its originating repository,
  commit and ownership/license decision before copying it.
- Do not duplicate authoritative WebAgent documentation here. Link to the
  upstream document and describe only the integration contract.
- A change in one repository must not silently publish, tag or deploy the
  other.
- Each project keeps its own CI. The integration track adds separate contract
  and end-to-end gates using pinned versions.

As checked through GitHub repository metadata on 2026-08-27, neither repository
advertised a detected license. This documentation change copies no source code
between them. Licensing or an explicit internal source-reuse decision must be
settled before shared implementation is moved across repositories.

## Runtime contract

The intended initial deployment is distributed:

```text
Companion host
  webagent-rs
      |
      | authenticated, versioned Brain interface
      v
  Uberbot integration runtime
      |
      | authenticated, versioned semantic Body API
      v
HomBot
  hombotd -> rpmain -> Micom/original safety logic
```

The interfaces must expose semantic operations and evidence, not unrestricted
shells or opaque hardware-send primitives. At minimum, every capability record
needs:

- stable identifier and interface version;
- originating project, component and observed device;
- supported operations and constraints;
- read/write and safety classification;
- evidence/provenance, confidence and last verification time;
- permissions and dependencies;
- explicit unavailable or unknown state.

## Versioning and compatibility

An integration release records all three versions:

```text
uberbot integration version
webagent-rs revision or release
hombotd revision or release
Body API version / Brain API version
```

Contract tests run against those exact inputs. Compatibility is never inferred
from branch names. A newer component may be tested independently, but it does
not become the integration baseline until its contract and safety gates pass.

## Evidence labels

Cross-project documentation uses these labels consistently:

- **Live verified** -- measured on the physical HomBot and recorded in
  `STATUS_LIVE.md`.
- **Host verified** -- executed on a development or companion host, not on the
  robot.
- **Contract tested** -- verified with fixtures, mocks or synthetic replay.
- **Integrated, not deployed** -- both real components communicated, but the
  resulting version is not active on the robot.
- **Planned** -- architecture or roadmap only.
- **Unknown** -- insufficient evidence; no value is guessed.

## Current boundary

As of 2026-08-27:

- the HomBot runs the standalone `hombotd 0.1.10` sidecar;
- `webagent-rs` remains an independent companion-host project;
- no Uberbot integration runtime is deployed;
- no agent-to-motion path exists;
- this document and `UBERBOT_ROADMAP.md` establish the integration scope, not
  a new live capability.
