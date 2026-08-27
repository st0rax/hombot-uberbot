# Architecture

## Repository and product boundary

Uberbot combines capabilities from two independent projects without combining
their Git histories or release ownership:

```text
webagent-rs repository                 hombot-uberbot repository
agent loop, Brains, memory             HomBot sidecar and device evidence
             |                                      |
             +---- versioned integration contracts -+
                                |
                         Uberbot integration
```

`webagent-rs` remains a general local agent product. `hombotd` remains the
HomBot body service and can be built, deployed and rolled back without
WebAgent. The new Uberbot layer owns only the adapters, capability model and
end-to-end integration needed to connect them. Detailed ownership and
versioning rules are in [`PROJECT_BOUNDARIES.md`](PROJECT_BOUNDARIES.md).

No integration component is live on the physical robot yet. The following
target diagram must not be read as a deployed-state claim:

```text
webagent-rs on companion host
        |
 versioned Brain interface
        |
Uberbot integration runtime
        |
 versioned semantic Body API
        |
hombotd on the HomBot
        |
rpmain -> Micom -> original safety logic
```

The initial integration should run on a modern companion host. ARMv6 remains a
body target, not the place where every Brain, vision or memory capability must
run.

## Platform boundary

The HomBot has two important control domains:

```text
Browser / local automation
           |
      hombotd API
       |       |
   camera   read-only LG message adapter
                 |
              rpmain
                 |
       Micom and original safety logic
```

`rpmain` remains the hardware abstraction and real-time behavior layer during
the sidecar phase. `hombotd` must not open the productive Micom UART in parallel,
because a competing reader can consume messages or corrupt transport state.

Heavy computer vision, SLAM, object recognition and long-term logging should run
on a current companion host. The robot is best treated as a real-time chassis,
sensor and actuator gateway.

## hombotd

The daemon is a dependency-free Rust program built as a static ARM binary. Its
current responsibilities are:

- serve a standalone FPV web interface;
- read `/dev/camclone` using a persistent descriptor;
- stream color YUV422P or Y8 luma with bounded frame pacing;
- invalidate the previous logical stream when a new stream opens;
- perform the device-local SmartControl handshake and expose status as JSON;
- expose a small read-only health surface.

The only current write-capable endpoint is token-gated audio playback. Motion
APIs are deliberately absent. Before adding motion, implement authentication,
bounded connections, a watchdog, an exclusive lease, a heartbeat and fresh
sensor interlocks. The complete live-evidence release gate is defined in
[`MOTION_READINESS.md`](MOTION_READINESS.md).

The observed legacy network setup binds the LG services broadly but does not route
the nominal loopback range correctly. The daemon therefore discovers its active
interface address and uses that address for its connections back to ports 4002 and
4000. `HOMBOTD_SMARTCONTROL_HOST` provides an explicit override.

## Camera data path

The camera produces 320x240 YUV422P frames of 153,600 bytes. The browser converts
raw data client-side so the slow ARM CPU does not spend cycles on JPEG encoding.
Y8 halves payload size and is preferred for lowest-latency teleoperation.

Only one pending frame should exist per client. When a client cannot keep up,
drop old frames instead of building latency.

## Persistence and rollback

Deploy releases into versioned directories. A startup-file change is staged,
syntax checked and atomically renamed. The original webserver executable remains
on disk. Keep logs and PIDs in `/tmp`; persistent UBIFS already has meaningful
wear and should not receive high-rate telemetry.

## Future control boundary

A write-capable API should accept semantic, allowlisted commands rather than raw
shell or opaque LG frames. Motion must stop on any of these conditions:

- lease or heartbeat expiry;
- controller disconnect;
- stale or invalid sensor state;
- cliff, wheel-drop or bumper interlock as appropriate;
- invalid battery/power state;
- loss of the LG transport.

The stop path must be independent from the external navigation/AI process.
