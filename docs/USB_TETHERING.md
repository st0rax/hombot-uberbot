# Android USB tethering / UBERPHONE Relay

## Current finding

The stock HomBot kernel configuration enables these drivers as modules:

```text
CONFIG_USB_USBNET=m
CONFIG_USB_NET_CDCETHER=m
CONFIG_USB_NET_RNDIS_HOST=m
```

LG shipped `usbnet.ko`, but omitted `cdc_ether.ko` and `rndis_host.ko` from the
device filesystem. The workflow in this repository reconstructs LG's
2.6.33.7.2-rt30 kernel from the public `larixer/kernel.rk` source and builds all
three modules with symbol versioning enabled.

Do not load an artifact merely because its filename is correct. Before a live
test, compare its `vermagic`, dependencies and exported-symbol CRCs with the
running kernel and the stock `usbnet.ko`. Test with temporary `insmod` first;
only a validated bundle may be added to startup.

## Build status

The reproducible build is green. Four legacy-toolchain problems stood between
the 2013 source and a current runner, all fixed in
`tools/build-usb-tether-modules.sh` by restoring the original semantics rather
than by weakening a check:

| Symptom | Cause | Fix |
| --- | --- | --- |
| `.err` in `kernel/fork.o` | `put_user(0, ...)` -- GCC folds `register const ... asm("r2")` into a constant and picks another register, tripping the kernel's own `__asmeq()` guard | drop the `const`, as upstream did |
| `timeconst.h` exit 255 | `defined(@array)` removed in Perl 5.22 | use the array's own truthiness |
| `NX_GPIO_SetBit` undefined at link | plain `__inline` emits no out-of-line copy under C99; LG built with GCC 4.3 where GNU89 did | `KCFLAGS=-fgnu89-inline` |
| `junk at end of line ... '#'` | `.section .piggydata,#alloc` -- legacy flag spelling | translate all spellings, then assert none survive |

## ABI verification

Live-test gate 1 no longer needs the robot. LG ships its own modules in the
firmware, so every symbol that both a stock module and a freshly built one
import can be compared offline:

```sh
python tools/verify-module-abi.py   --reference /path/to/stock/8192cu.ko /path/to/stock/rt3370sta.ko   -- out/usb-tether-modules/*.ko
```

Result for the current bundle:

```text
reference vermagic: '2.6.33.7.2-rt30 preempt mod_unload modversions ARMv6 '
ok   usbnet.ko:     57/78 symbols match, 21 not covered by any reference module
ok   cdc_ether.ko:   8/18 symbols match, 10 not covered by any reference module
ok   rndis_host.ko: 16/33 symbols match, 17 not covered by any reference module
```

`vermagic` is byte-identical to LG's own modules and no compared CRC differs,
which means the reconstructed tree reproduces LG's struct layouts across the
networking, memory and locking symbols the two sides share.

This is evidence, not proof. The uncovered symbols are ones no stock module
imports, so nothing local can check them -- although most of `cdc_ether`'s and
`rndis_host`'s uncovered symbols are `usbnet_*`, which come from the `usbnet.ko`
built in the same run and are therefore consistent by construction. `insmod` on
the device stays the final authority, and it fails closed: a CRC mismatch is
refused, not loaded.

## Live result

The link is proven end to end on the robot, 2026-08-23:

```text
insmod usbnet.ko      exit 0
insmod cdc_ether.ko   exit 0
insmod rndis_host.ko  exit 0

usb0: register 'rndis_host' at usb-nx-ehci-1.3, RNDIS device, 56:c0:72:58:4a:08
Lease of 10.202.167.50 obtained, lease time 3599

--- 10.202.167.41 ping statistics ---
3 packets transmitted, 3 packets received, 0% packet loss
round-trip min/avg/max = 2.5/2.6/2.9 ms
```

`insmod` is the check that mattered: it compares every imported symbol's CRC
against the running kernel and refuses on any mismatch. All three modules were
accepted, so the reconstructed tree really does reproduce LG's ABI -- the
offline comparison in the section above was right, and now it is confirmed
rather than merely consistent.

Two defects surfaced during the live run and are fixed in `deploy/`:

* The scripts assumed `ifconfig`, `route` and `udhcpc` were on `PATH`. They are
  in `/sbin`, which a non-interactive ssh command does not get, so the manager
  aborted before touching anything. It now sets `PATH` itself.
* `udhcpc -t 5` never saw a lease. Android brings its tethering DHCP server up a
  moment after the RNDIS interface appears, and five quick discovers miss it;
  `-t 20 -T 3` gets a lease reliably on the same hardware.

The default route stayed on WLAN throughout, which is the point of `link` mode:
the phone is reachable as a local link without mobile data silently carrying the
robot's traffic. Promoting it to an uplink is a separate, deliberate step.

Still outstanding before any boot-time change: the three plug/unplug cycles in
the gate list below, and a measured throughput figure. Nothing here has been
made persistent.

## Intended topology

```text
Android phone (RNDIS + 4G/5G + GNSS/IMU/camera)
                         |
                    USB 2.0 data
                         |
        powered hub without upstream backfeed
                  |                 |
             HomBot USB host     optional WLAN
                  |
        usbnet -> rndis_host -> usb0
                  |
          DHCP from Android tethering
```

The phone initiates an outbound authenticated relay connection. Port 6260 is
never exposed directly to the mobile network. Loss of USB, Android service or
the remote link must expire any future motion lease locally.

## Live-test gates

1. Modules have matching kernel version and symbol CRCs.
2. Phone is connected with a real data cable and USB tethering enabled.
3. The device enumerates in `dmesg` before any network configuration.
4. The generated interface is identified through sysfs; never assume `usb0`.
5. DHCP runs only on that interface.
6. Existing WLAN and default route are recorded before the test.
7. Unplug restores the previous route without reboot.
8. No persistent boot change until three plug/unplug cycles pass.

## Transient test manager

`deploy/uberphone-tether.sh` implements the live-test gates without changing
boot configuration. Install it and `deploy/udhcpc-uberphone.sh` under
`/usr/data/frankenhomo/bin/`, and place the validated `cdc_ether.ko` and
`rndis_host.ko` bundle under `/usr/data/frankenhomo/modules/usb-tether/`.

```sh
# Keep WLAN/default routing untouched; phone is a local relay link only.
uberphone-tether.sh start link

# Add the phone as a lower-priority (metric 50) Internet route.
uberphone-tether.sh start uplink

uberphone-tether.sh status
uberphone-tether.sh log
uberphone-tether.sh stop
```

The manager verifies each external module's `vermagic` against the running
kernel before `insmod`, discovers the interface from its sysfs driver instead
of assuming `usb0`, confines DHCP to that interface, records the pre-test route
table and removes only the phone route on stop. `link` is deliberately the
default for the first three plug/unplug tests.

## Power boundary

USB data support does not imply that the HomBot USB port can charge a modern
phone. Phone/hub power is a separate hardware design and is outside the driver
installer. A passive Y cable must not backfeed the HomBot.
