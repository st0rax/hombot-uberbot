# Live status

This file describes the state of the physical device, which moves faster than
commit history. Update it when you change what is actually running or
verified -- not when you merely write code for it. Every line is either
something measured on the device or explicitly marked as not yet confirmed.

Last updated: 2026-08-23.

## Deployed

- `hombotd 0.1.8` is the active service, started from `rc.local`.
- Boot greeting: an audio clip plays on startup via a
  `# FRANKENHOMO_GREETING` block in `/usr/etc/rc.local`, installed with
  `tools/operator/deploy_greeting.py --at-boot`.

## Verified live, this session

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

## Decoded, not yet live-confirmed

- **Factory voice service protocol** (`docs/VOICE_PROTOCOL.md`,
  `hombotd/src/voice.rs`): four message formats read out of `rpmain.axf`'s
  disassembly, including the `SSLResult` bearing field (0-359 degrees). Unit
  tests exercise the parser against synthetic frames built the same way
  `AServiceMessage::PublishMessage` builds them. No real frame has been
  captured, because the services only start when `Name.dat` names the voice
  variant (see `AGENTS.md`) -- `hombotd`'s `/api/v1/voice` endpoint reports
  `"live_confirmed": false` for exactly this reason and will keep doing so
  until a real frame is observed.
- **`/api/v1/system`'s `network` field** (`hombotd/src/net.rs`): unit-tested
  against synthetic `/proc/net/route` and `/proc/net/dev` fixtures, compiled
  into the deployed 0.1.8 build, but not independently re-queried on the
  device after this deployment in this session.

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
  set up anywhere.
- **No motor/drive command path.** `hombotd` holds the SmartControl session
  and sends only keepalive traffic. Deliberately not built yet -- see
  `AGENTS.md` on why this needs more care than the rest.
- **The chip's H.264 hardware encoder has no driver.** The SoC declares it
  (`nx_chip_p2120.h`); nothing in the kernel tree touches it. Camera frames
  are transmitted raw. See `docs/ROADMAP.md` for the measured impact.
- **USB hub has no independent power supply.** A second USB device beyond
  the WLAN adapter reliably fails to stay enumerated; see the USB power
  section in `AGENTS.md`.
- **Sensor inventory is incomplete.** The RawSensor frame is 158 bytes wide;
  four fields are decoded (`hombotd/src/rawsensor.rs`), the rest are not.

## Credentials

Never stored in this repository. See `docs/OPERATOR_TOOLS.md` for how to
supply `HOMBOT_HOST` and `HOMBOT_LOGIN_SECRET` from your own environment.
