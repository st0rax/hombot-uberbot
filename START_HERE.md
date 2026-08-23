# Start here

You are picking up work on **UBERBOT**, a reversible modernization of one
specific LG HomBot VR6340LV. Read this file, then `STATUS_LIVE.md`, then
`AGENTS.md`, before touching the device.

## What this project is

A small Rust daemon (`hombotd/`) that replaces LG's unauthenticated
web/camera layer while leaving the original real-time motion and safety stack
untouched. Everything else -- USB tethering, USB audio, the voice service
protocol, the operator tools -- exists to extend what that daemon can do
without ever needing to modify LG's own application.

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
4. `docs/ARCHITECTURE.md`, `docs/PROTOCOL.md` -- how the pieces fit together
5. `docs/OPERATOR_TOOLS.md` -- the scripts that drive the robot from a
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
