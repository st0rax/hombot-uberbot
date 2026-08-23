# Reverse-engineering findings

## System

- Model: LG HomBot VR6340LV, board family `RK_HIT_V2`.
- SoC: Nexell NXP2120, ARM1176/ARMv6, approximately 700 MHz with VFP.
- RAM: about 112 MiB physical, roughly 109 MiB usable, no swap.
- Kernel: Linux 2.6.33.7.2-rt30; a matching LG/Nexell source tree exists.
- Main application: `rpmain_13865.axf`, an ELF32 ARM service system with symbols.
- Camera: PixelPlus POA030 through Nexell VIP, 320x240 YUV422P.
- Audio: WM8960 through I2S/ALSA.
- Wi-Fi: RT5370 USB on the single external USB host port.

No proprietary firmware, extracted filesystem or dump is included here.

## Service architecture

The original application is an event-driven C++ service system rather than one
monolithic control loop. Identified domains include Event, Motion, Navigation,
Planning, Camera, Sound, Playback, VSLAM, BlackBox, SmartControl, SmartData and
factory Jig services.

The Linux application communicates with a secondary microcontroller that owns
motors and important sensors. Its productive serial path is active at runtime;
parallel access by a second process is unsafe.

## Camera bottleneck

The historic webserver starts a separate read operation for each frame and blocks
its central socket loop. It also inserts a fixed one-second browser delay. A
persistent camera descriptor proved that the device can deliver approximately 29
raw frames per second. WLAN throughput, not the sensor, becomes the main limit for
raw streaming.

Measured during development, color streaming was roughly 9-12 FPS and grayscale
roughly 14-20 FPS. These are setup-specific measurements, not guaranteed targets.

## Audio

The firmware contains regular mute-state and voice-disable paths. Prefer those
semantic paths over replacing sound assets. Audio files and device recordings are
not stored in this repository.

## Sensor transport

Known published sensor structures include:

| ID | Provisional type | Size |
|---:|---|---:|
| 101 | normal sensor data | 84 bytes |
| 104 | accelerometer/bumper | 40 bytes |
| 105 | raw sensors | 158 bytes |
| 106 | extended diagnostics | 28 bytes |

Offsets inferred from static code paths are hypotheses until correlated with
controlled physical stimuli. Preserve raw values, decoded values, timestamps and
confidence separately.

## NAND layout and recovery gap

| Partition | Size | Content |
|---|---:|---|
| mtd0 | 256 KiB | U-Boot 1.3.4 |
| mtd1 | 3.75 MiB | legacy uImage |
| mtd2 | 20 MiB | SquashFS root |
| mtd3 | 56 MiB | UBI/UBIFS `/usr` |
| mtd4 | 48 MiB | UBI/UBIFS `/usr/data` |

Ordinary MTD payload images do not preserve all OOB bytes, ECC information and
physical bad-block markers. They are therefore not a complete bare-metal recovery
route. Do not perform persistent flash experiments until OOB-aware recovery and a
RAM-only boot have been tested.

## Evidence discipline

The private research workspace contains device-specific dumps and captures. They
must remain outside Git. Publish only original notes, synthetic fixtures and code
needed to reproduce a claim without exposing device/user data or vendor binaries.
