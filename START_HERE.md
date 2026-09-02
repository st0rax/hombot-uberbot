# Start here

You are picking up work on **UBERBOT** (`st0rax/hombot-uberbot`), a reversible
modernization of one LG HomBot VR6340LV plus an integration track toward
`webagent-rs`. This file is the Bazaar **entry**. It is **not** the only file.

**Before any claim, any branch, any device thought:** read `AGENTS.md`,
`STATUS_LIVE.md`, and `SECURITY.md` in full. Those three override convenience.
If they conflict with this file or with Dummy-Bazaar habits, the stricter
HomBot protection wins.

Dummy (`st0rax/dummy-bazaar`) is a process template only. Do not copy its
"only file" sentence here. Do not copy `opencode@webagent.local` identities.
Do not patch `webagent-rs` from this repo.

## Device ice (now)

The owner reported a maintenance window (bumpers physically blocked,
2026-08-27, no end date in `STATUS_LIVE.md`). **No SSH, no deploy, no
Name.dat, no motors, no serial attach, no secrets in git.** Tree work only
until the owner explicitly ends maintenance.

Nothing is live unless `STATUS_LIVE.md` has a device measurement. Tree ≠ live.
Decoded ≠ live. `cargo test` is never enough to mark a robot fact done.

## What this project is

A small Rust daemon (`hombotd/`) replaces LG's unauthenticated web/camera
layer and leaves the original real-time motion and safety stack untouched.
The owner's one-sentence goal: **call it, and it comes.** That needs a live
voice bearing and a motor command path. Both are **not** free tasks today.
Motors stay blocked until interlocks and UART rules are actually satisfied.
See `docs/FEASIBILITY.md` and `docs/ROADMAP.md`.

## Read before you claim (this order)

1. `AGENTS.md` — incident rules for this exact robot
2. `STATUS_LIVE.md` — last device measurement; tree-only stays tree-only
3. `SECURITY.md` — no unauthenticated writes; token rules
4. `GOALS.md` — G-001 is the Nordstern, **not** a claim cell
5. `docs/WORK_CONTRACT.md` — how claims and inspection work here
6. `docs/TASKBOARD.json` — **only** claim board (JSON is truth)
7. Then `README.md`, `docs/PROJECT_BOUNDARIES.md`, `docs/HARDWARE.md` as needed

## How to take a task

1. If a task would need the robot, **stop**. Ice holds.
2. Pick one `free` row in `docs/TASKBOARD.json` whose `depends_on` are all
   `done`. One JSON `id`, one developer.
3. Claim **only in JSON**: `status=claimed`, `owner`, `branch`, `claimed_at`.
4. Branch from `main`: `docs/<id>-<kurz>` or `feature/<id>-<kurz>`.
5. Small commits. Author from `docs/GIT_AGENTS.md` (project-local). Never
   `*@webagent.local`.
6. Verify with the task's own `verification` field. Live claims need a
   `STATUS_LIVE.md` receipt. `cargo test --lib` does not make the robot true.
7. Open or update a PR. Do **not** merge PR #3 or PR #4 unless the owner says
   so. Do not force-push `main`.

## Tabu

- `Name.dat`
- Motor / drive / new actuator endpoints
- UART1 parallel to `rpmain`; unconfirmed UART0 as a recovery story
- Credentials, hosts, tokens, NAND dumps, vendor binaries in git
- Unauthenticated write endpoints
- Inventing wire field names into `rawsensor.rs`

## Serial console

There is **no confirmed serial console**. Boot and `rc.local` changes are
higher risk than they look. Read Boot Safety in `AGENTS.md` before touching
deploy paths — and then still do not deploy while ice holds.
