# Live status

This file describes the state of the physical device, which moves faster than
commit history. Update it when you change what is actually running or
verified -- not when you merely write code for it. Every line is either
something measured on the device or explicitly marked as not yet confirmed.

Last updated: 2026-08-27.
Last device measurement: 2026-08-27. Older receipts stay dated 2026-08-23.
Nothing is marked live unless a session measured it on the robot. Code that
is only in the tree stays under "In the tree, not on the device".

## Deployed

- `hombotd 0.1.10` is the active service, started from `rc.local`. Reconfirmed
  2026-08-27.
- `HOMBOTD_RAWSENSOR=1` is on. The RawSensor subscriber reported `state`
  `connected` in that session.
- Boot greeting: an audio clip plays on startup via a
  `# FRANKENHOMO_GREETING` block in `/usr/etc/rc.local`, installed with
  `tools/operator/deploy_greeting.py --at-boot`. Last confirmed 2026-08-23.

`main` currently builds as `0.1.11` (`hombotd/Cargo.toml` after `5278fe7`).
That binary is not what the robot is running. C2 is in `main` and is not
deployed.

## Verified live, 2026-08-27

- **`GET /api/v1/sensors`**: `available: true`, `raw_record_size: 158`,
  `age_ms` about 14. Subscriber `state` was `connected` with
  `HOMBOTD_RAWSENSOR=1`.
- **Baseline capture**: 15 rest frames taken (`01_baseline` in
  `docs/SENSOR_CAPTURE.md`). No bumper or cliff stimulus in this session.
- **Battery voltage field**: still `calibration: pending_multimeter_pair`.
  Do not quote it as volts. The centivolt raw and the JSON `voltage_v`
  number are a mapping, not a meter reading.
- **Voice**: still off. No live frame. Do not touch `Name.dat`.
- **C2 / 0.1.11 dashboard**: not on the device.

## Verified live, last measured 2026-08-23

These receipts are from the 23 August session. They were not re-run on
27 August. Treat them as last known good for those items, not as today's log.

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
  captured, including on 2026-08-27 -- the services only start when `Name.dat`
  names the voice variant (see `AGENTS.md`). `hombotd`'s `/api/v1/voice`
  endpoint reports `"live_confirmed": false` for exactly this reason. Do not
  touch `Name.dat` to force this.

## In the tree, not on the device

### Voice-Telemetry for 0.1.11 (`5278fe7`, 2026-08-26)

- `hombotd/Cargo.toml` version is `0.1.11`.
- Dashboard panel for the existing `/api/v1/voice` payload.
- Not deployed. Voice is still off on 0.1.10.

### C2 shell (PR #1, merged to `main`, not deployed)

- FPV stays at `GET /`. `GET /c2` is a capability-slot shell. Drive controls
  are visible and disabled. No motor path.
- Default is demo data. Not running on the robot as of 2026-08-27.

### Operator raw_record_hex logger (PR #2)

- Capture helper for `docs/SENSOR_CAPTURE.md`. The 27 August session took 15
  baseline frames through `GET /api/v1/sensors`. That does not make the
  operator tool a device service.

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
  `AGENTS.md`. C2 does not change this.
- **The chip's H.264 hardware encoder has no driver.** The SoC declares it
  (`nx_chip_p2120.h`); nothing in the kernel tree touches it. Camera frames
  are transmitted raw. See `docs/ROADMAP.md` for the measured impact.
- **USB hub has no independent power supply.** A second USB device beyond
  the WLAN adapter reliably fails to stay enumerated; see the USB power
  section in `AGENTS.md`.
- **Sensor inventory is incomplete.** The RawSensor frame is 158 bytes wide;
  four fields are decoded (`hombotd/src/rawsensor.rs`), the rest are not.
  15 baseline frames exist as of 2026-08-27; bumper/cliff XOR has not been
  done. Bytes 4-9 are control (battery/charger), not bumper/cliff signal.
- **Battery voltage is not a calibrated volt reading.** Pending a
  multimeter pair.

## What you can show today without lying

Show the robot on **0.1.10** with RawSensor connected: `available true`,
158-byte records, `age_ms` ~14, 15 baseline frames, voltage uncalibrated.
Camera, SmartControl, and the 23 August audio/tether receipts still stand as
older measurements. Voice is decoded / not live-confirmed. `/c2` and the
0.1.11 dashboard are tree, not the device.

## Credentials

Never stored in this repository. See `docs/OPERATOR_TOOLS.md` for how to
supply `HOMBOT_HOST` and `HOMBOT_LOGIN_SECRET` from your own environment.
