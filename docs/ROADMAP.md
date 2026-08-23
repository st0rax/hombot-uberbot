# Roadmap

## Live checkpoint: release 0.1.3

- Deployed as the active service on the research device.
- Managed startup restores loopback and pins the device-local SmartControl
  adapter to localhost.
- SmartControl reports connected. Port 4000 remains established for status and
  keepalive traffic; the one-shot port-4002 admission descriptor is closed after
  `CONNECT/ENABLE` so it does not linger in `CLOSE_WAIT`.
- `CONNECT_INIT` fields are currently null and remain represented as unknown.
  No battery, state or mode values are synthesized.
- Bounded stream checks produced 20/20 unique frames at 10.29 FPS color and 16.53
  FPS grayscale on the tested WLAN path.

## Stage 1: observable sidecar

- Stable camera streaming and health endpoints.
- Read-only SmartControl status.
- Read-only raw-sensor subscriber through the message broker.
- Resource limits, bounded connections and a watchdog.
- 24-hour stability test with CPU, RSS, sockets and frame drops recorded.

## Stage 2: safe local control

- Local authentication with a token stored mode `0600`.
- Host/Origin validation and no wildcard CORS.
- Semantic command allowlist; no shell, arbitrary frame or upload execution.
- Exclusive control lease, frequent heartbeat and automatic stop.
- Fresh sensor state plus cliff, wheel-drop, bumper, battery and transport
  interlocks.
- Motion tests only with stable power and a controlled test area.

## Stage 3: companion compute

- Move compression, SLAM, object recognition and long-term data storage to a
  modern host.
- Keep the HomBot as a low-latency chassis and sensor gateway.
- Define versioned telemetry and command schemas with replayable synthetic tests.

## Stage 4: recovery operating system

- Identify and verify UART0 physically.
- Record complete U-Boot environment and NAND bad-block information.
- Produce an OOB-aware NAND recovery image.
- Rebuild the matching legacy kernel from source.
- Boot a tiny BusyBox/initramfs image through YMODEM entirely in RAM.
- Validate UART, NAND read-only, USB, camera and audio without flash writes.

## Stage 5: optional alternative userspace/kernel

A small Buildroot userspace on USB or persistent data storage is practical while
retaining the original kernel. A rebuilt legacy kernel can also be tested safely
in RAM. A current mainline kernel is a separate board-support project requiring
clock, interrupt, timer, GPIO, UART/DMA, NAND, USB, I2C, audio, camera and other
driver work.

## Exit criteria

- A new developer can build, deploy and roll back from repository documentation.
- Camera, system, sensor and battery-source health are locally observable.
- No cloud dependency or unauthenticated control surface.
- Every persistent change has a verified recovery route.
- Persistent firmware changes begin only after RAM recovery and complete NAND/OOB
  restoration have been demonstrated.
