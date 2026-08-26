# Live status

This file describes the state of the physical device, which moves faster than
commit history. Update it when you change what is actually running or
verified -- not when you merely write code for it. Every line is either
something measured on the device or explicitly marked as not yet confirmed.

Last updated: 2026-08-26.
Last device measurement: 2026-08-23. Nothing in this file is marked live
unless that session measured it on the robot. Code that landed after that
date stays in "In the tree, not on the device" until someone measures it
again.

## Deployed

- `hombotd 0.1.10` is the active service, started from `rc.local`.
- Boot greeting: an audio clip plays on startup via a
  `# FRANKENHOMO_GREETING` block in `/usr/etc/rc.local`, installed with
  `tools/operator/deploy_greeting.py --at-boot`.

`main` currently builds as `0.1.11` (`hombotd/Cargo.toml` after `5278fe7`).
That binary is not what the robot is running.

## Verified live, last measured 2026-08-23

These receipts are from the 23 August session. They have not been re-run
today. Treat them as the last known good device state, not as a 26 August
demo log.

- **USB tethering**: `usbnet.ko`, `cdc_ether.ko`, `rndis_host.ko` built by
  `.github/workflows/build-usb-tether-modules.yml`, ABI-checked offline with
  `tools/verify-module-abi.py`, then `insmod`-accepted on the device (which is
  the real proof -- it compares symbol CRCs against the running kernel and
  refuses on mismatch). RNDIS bound to a phone in USB tethering mode, DHCP
  lease obtained, gateway ping 2.5-2.9 ms. Not persisted at boot yet.
- **USB audio**: `snd-hwdep.ko`, `snd-rawmidi.ko`, `snd-usb-lib.ko`,
  `snd-usb-audio.ko` from `.github/workflows/build-usb-audio-modules.yml`,
  same ABI process, `insmod`-accepted. A USB audio device in the hub enumerates
  as a second sound card with a working capture substream (`plughw:1,0`);
  measured rms in the hundreds on real speech, no clipping.
- **`/stream.wav`** on `hombotd 0.1.8`: confirmed to deliver a valid,
  continuously-growing 16 kHz mono WAV stream over HTTP from the chosen
  capture card.
- **`/api/v1/audio`**: confirmed to report the correct capture card and which
  sound card's playback substream is free vs held by LG's own application.
- **Speak/listen round trip**: text synthesised on the operator machine,
  uploaded, played through the robot's USB speaker; a spoken answer recorded
  through the robot's USB microphone, transferred back, and recognised --
  full loop, no PC-side microphone needed once a USB audio device is present.
- **Internal WM8960 microphone inputs: not populated.** At maximum analog
  gain (capture 100%, ALC on, boost stages on, mic bias High) both channels
  read a flat noise floor (peak ~12 of 32768) with a gain-independent DC
  offset -- the signature of an unconnected input, not a quiet one.
- **The 4-pin connector next to the camera module is USB, not a microphone
  connector.** Confirmed by comparing it to the identical connector on the
  cable running to the USB port daughterboard.

- **`/api/v1/system`'s `network` field**: confirmed live on the device --
  reports the WLAN interface (`ra0`, default route) correctly when no phone is
  attached.
- **`/api/v1/audio` and the dashboard's audio panel** (0.1.9): confirmed live
  -- correctly reports the built-in codec as busy (LG's own application holds
  it) when no USB audio device is present, and the dashboard's card selector
  reflects that rather than showing anything invented. Streaming audio to the
  browser itself needs a USB audio device plugged in to test end to end; not
  yet done.
- **Dashboard "sensor envelope" radar removed.** It was a static decorative
  graphic that never plotted anything -- see `AGENTS.md`'s evidence rule.
  Replaced with plain text tied to the real `/api/v1/sensors` state.

- **`POST /api/v1/audio/play`**: the daemon's first write-capable endpoint --
  plays an uploaded WAV or raw-PCM clip through a free sound card. Confirmed
  live end to end: an unauthorized request (no `X-Hombot-Token` header) gets
  `401` before any audio logic runs; an authorized request reaches the real
  handler, which correctly reported `503` (no free playback substream) with
  only the busy built-in codec present. A full play-through still needs a USB
  audio device attached to test.
- **Local control token**: generated on first use at
  `/usr/data/frankenhomo/control.token`, mode `0600`, confirmed on the
  device. This closes a real gap -- the endpoint above was added to this
  codebase without one, which is exactly what `SECURITY.md`'s "no
  unauthenticated upload endpoints" rule exists to prevent. See `AGENTS.md`.

## Decoded, not yet live-confirmed

- **Factory voice service protocol** (`docs/VOICE_PROTOCOL.md`,
  `hombotd/src/voice.rs`): four message formats read out of `rpmain.axf`'s
  disassembly, including the `SSLResult` bearing field (0-359 degrees). Unit
  tests exercise the parser against synthetic frames built the same way
  `AServiceMessage::PublishMessage` builds them. No real frame has been
  captured, because the services only start when `Name.dat` names the voice
  variant (see `AGENTS.md`) -- `hombotd`'s `/api/v1/voice` endpoint reports
  `"live_confirmed": false` for exactly this reason and will keep doing so
  until a real frame is observed. Do not touch `Name.dat` to force this.

## In the tree, not on the device

Neither of the 26 August UI pieces has been deployed or measured on the
robot. Do not demo them as live.

### `main` -- Voice-Telemetry for 0.1.11 (`5278fe7`, 2026-08-26)

- `hombotd/Cargo.toml` version is `0.1.11`.
- Dashboard panel for the existing `/api/v1/voice` payload: subscriber state,
  last decoded sound bearing, last event, event counter, confirmation state.
- Does not enable the voice subscriber, does not change boot settings, does
  not expose actuator control.
- Because no live voice frame has ever been captured, a 0.1.11 process would
  still have to report the panel as not live-confirmed. Showing numbers there
  without a device receipt would be invented telemetry.

### PR #1 -- C2 shell (`feat/c2-page`, not merged)

- https://github.com/st0rax/hombot-uberbot/pull/1
- FPV stays at `GET /`. New `GET /c2` (and `/c2.html`) is a capability-slot
  shell: camera / listen / speak / status as live-shaped slots; Come / Home /
  Dock / D-pad marked in progress; map and autonomy marked planned.
- Drive controls are visible and disabled. No motor path. No new actuator
  endpoints.
- Default is demo data (banner: no live robot data). `?live=1` is specified
  to talk to existing hombotd APIs; that mode has not been measured on the
  device.
- Open GitHub issues: none. PR #1 is the only open review item.

## Known and open

- **No confirmed serial console access.** `inittab` shows a respawning root
  shell on `tts0`, which is a strong lead, not a tested fallback. Treat this
  as the top safety item; see `START_HERE.md`.
- **Speech recognition on the operator machine is weak.** Windows SAPI
  dictation returns near-zero confidence on clean recordings of German
  speech. A closed phrase list (`tools/operator/homebot.py`) is markedly
  better because the search space is a dozen phrases instead of open
  dictation, but wake-word detection in live testing still missed more often
  than it caught. Whisper, run locally, is the planned replacement; not yet
  set up anywhere. Trigger words are operator-side phrases, not LG factory
  KWS.
- **No motor/drive command path.** `hombotd` holds the SmartControl session
  and sends only keepalive traffic. Deliberately not built yet -- see
  `AGENTS.md` on why this needs more care than the rest. PR #1 does not
  change this.
- **The chip's H.264 hardware encoder has no driver.** The SoC declares it
  (`nx_chip_p2120.h`); nothing in the kernel tree touches it. Camera frames
  are transmitted raw. See `docs/ROADMAP.md` for the measured impact.
- **USB hub has no independent power supply.** A second USB device beyond
  the WLAN adapter reliably fails to stay enumerated; see the USB power
  section in `AGENTS.md`.
- **Sensor inventory is incomplete.** The RawSensor frame is 158 bytes wide;
  four fields are decoded (`hombotd/src/rawsensor.rs`), the rest are not.
- **STATUS_LIVE itself was stale** from 2026-08-23 until this draft. README
  already said 0.1.10 is on the device and 0.1.11 is in the tree; this file
  had not caught up.

## What you can show today without lying

Show the robot on **0.1.10**: camera, SmartControl status, the 23 August
audio/tether receipts, and the voice endpoint as decoded / not live-confirmed.
If you open the 0.1.11 dashboard or `/c2`, say it is tree or PR, not the
device.

To turn Voice-Telemetry or C2 into live lines, someone has to run them on
the robot and write the measurement here. This draft does not do that.

## Credentials

Never stored in this repository. See `docs/OPERATOR_TOOLS.md` for how to
supply `HOMBOT_HOST` and `HOMBOT_LOGIN_SECRET` from your own environment.
