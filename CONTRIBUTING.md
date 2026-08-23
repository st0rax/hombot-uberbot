# Contributing

## Working agreement

Every technical claim should be tied to a reproducible measurement, source file,
symbol, disassembly location or captured message. Clearly label hypotheses and
record the test that would confirm them.

Keep experiments staged:

1. offline parser or unit test;
2. read-only live observation;
3. bounded command with stable power and a clear test area;
4. persistent change only after backup and rollback verification.

Do not run LG's built-in Smart Diagnosis as a passive sensor test: known paths
include driving, rotation and docking movements.

## Code changes

- Run `cargo fmt --all -- --check`.
- Run `cargo test` on the host.
- Build the ARM target used by the device.
- Run `git diff --check`.
- Review staged files for secrets, private network identifiers and binaries.
- Update protocol or architecture documentation when behavior changes.

Keep the daemon dependency-light. The deployed system has roughly 109 MiB of
usable RAM, an old kernel and limited persistent flash endurance.

## Commit scope

Do not commit firmware, dumps, extracted filesystems, proprietary sound assets,
captured camera/audio data, generated binaries or credentials. Small synthetic
protocol fixtures are acceptable when they contain no user/device data and their
origin is documented.
