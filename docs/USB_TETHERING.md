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

## Power boundary

USB data support does not imply that the HomBot USB port can charge a modern
phone. Phone/hub power is a separate hardware design and is outside the driver
installer. A passive Y cable must not backfeed the HomBot.
