# Working rules for AI agents

This file is for an agent (or a human moving fast) about to run commands
against the physical device. Every rule below exists because its absence
already caused a real incident on this exact robot. Read `START_HERE.md`
first if you have not.

## Boot safety comes before features

`/etc/rc.d/rcS` starts the vendor's `rc.local` unconditionally, then starts
`/usr/etc/rc.local` **only if it is executable**:

```sh
if [ -x /usr/etc/rc.local ]
then
    /usr/etc/rc.local
fi
```

A file written with a shell redirect (`cmd > file`) or moved into place with
`mv` is not executable by default. Losing that bit is silent -- the robot
boots with no error, just without networking, ssh, or `hombotd`, because none
of it ever runs. This happened once already.

**Rule:** any time you write or move a file into `/usr/etc/rc.local`, run
`chmod 755` on it explicitly afterward, then have the robot itself confirm
`[ -x /usr/etc/rc.local ]` before you consider the change done. Never rely on
the source file's original permissions surviving a transfer.

`deploy_hombotd.py` and `deploy_greeting.py` in `tools/operator/` both do this
and refuse to proceed if the check fails -- use them rather than hand-rolling
the same sequence.

## The USB recovery path is not a safety net by itself

`/etc/rc.d/rc.local` (on the read-only squashfs, always runs) checks for
`/dev/uba1` and executes `/mnt/usb/root_update.sh` with no marker check and no
permission check, before `/usr/etc/rc.local` is even considered -- see
`docs/USB_TETHERING.md`'s history for why this matters. It is a real recovery
path and it is what actually saved this device once. But:

* the script name must be exactly `root_update.sh`, not `update.sh` (the
  latter is a different hook, called from *inside* `/usr/etc/rc.local`, which
  is exactly the file this path exists to fix)
* it runs before `/usr` is mounted -- a repair script must mount it itself
* keep such a stick prepared and labelled, but do not treat its existence as
  a reason to skip the execute-bit discipline above

## No physical UART access is confirmed yet

Until someone has actually logged into the serial console on this board,
there is no proven recovery path that does not depend on the boot sequence
already working. Treat any change to boot-time behavior, kernel modules
loaded at start, or `rc.local` as higher-risk than it would be with a
confirmed hardware fallback.

## Kernel modules: verify before you load

This device runs a reconstructed 2.6.33.7.2-rt30 kernel built from
`larixer/kernel.rk`. `CONFIG_MODVERSIONS` means `insmod` compares a CRC per
imported symbol against the running kernel and refuses on any mismatch --
that check is real and it fails closed. Still, verify offline first with
`tools/verify-module-abi.py` against modules LG's own firmware ships (e.g.
`8192cu.ko`, `rt3370sta.ko`), so a broken build is caught before you are
depending on the network path you might be about to break.

## USB power is a shared, finite budget

The robot has exactly one USB root port
(`hub 1-1:1.0: 1 port detected`; OHCI is not built into this kernel). A USB
hub without its own power supply divides a single port's ~500 mA among every
downstream device. The WLAN adapter alone reports 450 mA. A second device
that enumerates and then immediately disconnects
(`cannot get freq at ep 0x3` followed by `USB disconnect`) is this power
budget being exceeded, not a driver bug -- do not spend time debugging drivers
for that symptom before checking `bMaxPower` on every device at
`/sys/bus/usb/devices/*/bMaxPower`.

## ssh on this device has a broken stdin path

Writing to the stdin of an `exec_command` channel fails instantly here, even
for a handful of bytes, while reading a file back over the same channel and
using an interactive shell both work normally. Do not spend time debugging
this as a size or timing problem. Use the pattern in `tools/operator/`: start
a short-lived HTTP server on the operator machine and have the robot `wget`
from it.

## The WLAN link drops for seconds at a time

Retry ssh connection attempts with backoff; a single timeout is not evidence
of anything being actually wrong. The scripts in `tools/operator/` already do
this -- reuse them rather than writing a one-shot connection.

## USB audio devices here need `plughw`, not `hw`

The USB audio hardware tested on this robot only runs at 48 kHz. `arecord -D
hw:N,0 -r 16000` does not resample; it silently produces audio that is
mislabeled as 16 kHz and plays three times too fast. Always use `plughw:N,0`
for capture and playback on a USB card.

## Do not touch `Name.dat` to "just try" the factory voice services

`/usr/rscript/run_hit.sh` picks the entire boot configuration by comparing
`/usr/rcfg/Name.dat` against a fixed string. The voice-service branch starts
`/Sound /SSL /VR` but explicitly does **not** start `/SmartControl` or
`/SmartData`, and bypasses the `WIFI_ATTACHED` branch the current setup
depends on. Changing this file would silently remove the SmartControl channel
`hombotd` uses today. See `docs/VOICE_STACK.md` before going near it.

## Never run LG's built-in Smart Diagnosis as a passive test

Known paths through it include driving, rotation and docking movements. This
is already stated in `CONTRIBUTING.md`; it is repeated here because it is a
safety rule, not a style rule.

## The battery is explicitly outside this project's scope

The device owner handles battery work themselves. Do not propose, script, or
attempt battery-related hardware changes.

## Every technical claim needs a receipt

State what you measured and how, not what seems plausible. If you have not
run something on the actual device, say so plainly rather than writing it as
fact. `STATUS_LIVE.md`'s verified/unverified split exists because this
distinction mattered in practice -- a decoded protocol was correct on paper
for hours before a live frame confirmed it.
