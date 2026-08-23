# Security policy

## Project status

UBERBOT is experimental software for a mobile robot with motors, a battery,
sensors, a camera and a microphone. A software defect can cause physical motion,
property damage, privacy exposure or battery damage. Treat every deployment as
an untrusted laboratory system.

## Supported scope

Only the latest commit on the default branch is maintained. The original LG
firmware and third-party components are outside this project's support scope.

## Non-negotiable controls

- Do not expose the service directly to the internet.
- Do not add unauthenticated actuator, upload, shell or reboot endpoints.
- Keep device credentials and network identifiers out of source control.
- Bind write-capable control to a trusted interface and require an unpredictable
  local token stored with mode `0600`.
- Reject wildcard CORS for control APIs and validate both Host and Origin.
- Enforce request-size, connection-count and command-duration limits.
- Require an exclusive motion lease and a frequent heartbeat. Disconnect,
  expired telemetry or missed heartbeat must stop motion.
- Preserve cliff, bumper, wheel-drop, thermal, battery and transport interlocks.
- Never replace real battery telemetry with a fabricated healthy value.
- Store high-rate logs in `/tmp`, not persistent UBIFS.

## Secrets and captures

Never commit `.env` files, passwords, private keys, device addresses, Wi-Fi
configuration, diagnostic captures, camera/audio samples, filesystem archives,
NAND/OOB dumps or vendor firmware. The repository ignore rules are a guardrail,
not a substitute for reviewing `git diff --cached` before every commit.

If a secret is committed, rotate it immediately and remove it from the complete
Git history before sharing the repository.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting for this repository. Do not open a
public issue containing credentials, network details, private media or a working
remote-control exploit. Include affected commit, impact, reproduction steps and
a proposed mitigation when possible.
