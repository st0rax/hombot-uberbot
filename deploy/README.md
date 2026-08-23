# Deployment scripts

These scripts edit only the robot startup file. They do not copy binaries and
contain no device address or credential.

- `install.sh` replaces the expected stock `lg.srv` startup block with a managed
  `hombotd` block. It preserves both a canonical rollback copy and a timestamped
  audit copy, validates shell syntax and uses an atomic rename.
- `upgrade-0.1.3.sh` is the version-specific, live-tested update from the first
  managed release to 0.1.3. It restarts the daemon and restores the previous
  startup file if the new process exits immediately.
- `rollback.sh` restores the canonical original startup file, stops `hombotd`
  and launches the retained `lg.srv` executable.

Defaults can be overridden with environment variables. Read every resolved path
before running as root. Stage the binary as executable in the versioned release
directory and verify its digest before changing startup.

The scripts assume BusyBox `/bin/sh` and the startup layout observed on the
VR6340LV research device. They intentionally fail closed when expected markers or
the original `lg.srv` block are absent.
