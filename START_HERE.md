# Start here

You are picking up work on **UBERBOT**, the independent integration project
between `webagent-rs` and the reversible modernization of one specific LG
HomBot VR6340LV. Read this file, then `STATUS_LIVE.md`, then `AGENTS.md`,
before touching the device.

## What this project is

Today, the implemented part is a small Rust daemon (`hombotd/`) that replaces
LG's unauthenticated web/camera layer while leaving the original real-time
motion and safety stack untouched. Everything else -- USB tethering, USB
audio, the voice service protocol, the operator tools -- exists to extend what
that daemon can do without ever needing to modify LG's own application.

The new integration track connects that body to the agent/Brain capabilities
maintained independently in
[`webagent-rs`](https://github.com/st0rax/webagent-rs). It does not merge the
repositories or make one project's release depend on the other's `main` or
`master` branch. Integration happens through versioned contracts and pinned,
reproducible revisions or artifacts. Read `docs/PROJECT_BOUNDARIES.md` before
moving code or responsibilities across that boundary.

The goal stated by the device's owner is one sentence: **call it, and it
comes.** Two capabilities are missing before that is literally true --
knowing which direction a voice came from, and a command path to the motor
controller. Everything documented here either leads to those two, or is
useful alongside them. See `docs/ROADMAP.md` for the staged plan.

## Read in this order

1. `README.md` -- what exists, current status, build instructions
2. `STATUS_LIVE.md` -- what is verified on the physical device *right now*,
   which changes faster than this repo's commit history
3. `AGENTS.md` -- working rules distilled from incidents on this exact
   device; several of them exist because something broke
4. `docs/PROJECT_BOUNDARIES.md`, `docs/ARCHITECTURE.md` -- repository and
   runtime boundaries
5. `docs/ROADMAP.md`, `docs/UBERBOT_ROADMAP.md` -- the device and integration
   tracks
6. `docs/PROTOCOL.md` -- the verified and reconstructed device protocols
7. `docs/OPERATOR_TOOLS.md` -- the scripts that drive the robot from a
   development machine, and why they are shaped the way they are

## The one fact that overrides convenience

**There is currently no confirmed serial console access to this device.**
Until there is, any change that could leave the robot without a working
network path is close to unrecoverable -- the one USB recovery path found so
far itself depends on a file inside the normal boot sequence. Read the
`Boot Safety` section of `AGENTS.md` before editing anything under
`/usr/etc/rc.local` equivalents, `deploy/`, or the boot-time module loading
described in `docs/USB_TETHERING.md`.

## Talking to the robot

Nothing in this repository contains the device's IP address or its ssh
password. Both come from your own environment; see
`docs/OPERATOR_TOOLS.md` for the exact variables. If you don't have them,
ask the device's owner -- do not guess a LAN address or scan for it.
