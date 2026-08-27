"""Install a new hombotd release and point rc.local at it.

    python deploy_hombotd.py 0.1.6
    python deploy_hombotd.py 0.1.4 --rollback-only   # just switch rc.local back

The release directory keeps every version, so switching is a path change and a
restart -- the previous binary stays on disk to go back to.

Two things this does not do, both learned the hard way:

* It does not rewrite the whole managed block in rc.local. The boot greeting
  lives inside that block, and a wholesale rewrite would silently delete it.
  Only the version in the path is substituted.
* It never moves a file into /usr/etc/rc.local without setting the mode
  afterwards and then asking the robot to confirm the file is executable.
  /etc/rc.d/rcS starts it with `if [ -x ... ]`, so losing that bit means the
  robot boots without networking or SSH and cannot be fixed remotely.
"""

import argparse
import hashlib
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "pydeps_legacy"))

import paramiko  # noqa: E402
from ssh_auth import connect_auth  # noqa: E402


def _require(name):
    value = os.environ.get(name)
    if not value:
        raise SystemExit(
            f"{name} is not set. This repo ships no LAN addresses -- "
            f"export {name} for your own network (see docs/OPERATOR_TOOLS.md)."
        )
    return value

RC_LOCAL = "/usr/etc/rc.local"
BACKUP_DIR = "/usr/data/frankenhomo-backup"
RELEASES = "/usr/data/frankenhomo/releases"
BINARY = os.path.join(
    HERE,
    "hombotd-prototype",
    "target",
    "armv5te-unknown-linux-musleabi",
    "release",
    "hombotd-prototype",
)


def secret():
    value = os.environ.get("HOMBOT_LOGIN_SECRET")
    if value:
        return value
    path = os.path.join(HERE, ".hombot_secret")
    if os.path.exists(path):
        with open(path, encoding="utf-8") as handle:
            return handle.read().strip()
    raise SystemExit("No credential: set HOMBOT_LOGIN_SECRET or create .hombot_secret")


def connect(attempts=5):
    last = None
    for attempt in range(attempts):
        client = paramiko.SSHClient()
        client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        try:
            client.connect(
                _require("HOMBOT_HOST"),
                username=os.environ.get("HOMBOT_USER", "root"),
                timeout=10,
                banner_timeout=10,
                auth_timeout=10,
                **connect_auth(secret),
            )
            return client
        except Exception as error:  # the WLAN link drops often enough to matter
            last = error
            print(f"  connect attempt {attempt + 1} failed: {error}")
            time.sleep(4)
    raise SystemExit(f"could not reach the robot: {last}")


def run(client, command, timeout=120, check=True):
    _, stdout, stderr = client.exec_command(command, timeout=timeout)
    code = stdout.channel.recv_exit_status()
    out = stdout.read().decode("utf-8", "replace").strip()
    err = stderr.read().decode("utf-8", "replace").strip()
    if check and code != 0:
        raise SystemExit(f"remote failed ({code}): {command}\n{err or out}")
    return out


def push(client, payload, remote):
    _, stdout, stderr = client.exec_command(f"cat > {remote}", timeout=300)
    stdout.channel.sendall(payload)
    stdout.channel.shutdown_write()
    if stdout.channel.recv_exit_status() != 0:
        raise SystemExit(stderr.read().decode("utf-8", "replace"))


def upload_release(client, version):
    with open(BINARY, "rb") as handle:
        payload = handle.read()
    target_dir = f"{RELEASES}/{version}"
    remote = f"{target_dir}/hombotd"

    run(client, f"mkdir -p {target_dir}")
    push(client, payload, remote)
    run(client, f"chmod 755 {remote}")

    digest = hashlib.md5(payload).hexdigest()
    remote_digest = run(client, f"md5sum {remote}").split()[0]
    if remote_digest != digest:
        raise SystemExit(f"upload mismatch: {digest} != {remote_digest}")
    print(f"uploaded {remote}  {len(payload)} bytes  md5 {digest}")
    return remote


def smoke_test(client, remote, port=6297):
    """Start the new binary on a spare port before trusting it with 6260.

    hombotd takes no arguments -- it binds and serves -- so there is no
    --version to ask. The only way to find out whether this build runs on the
    robot is to run it, which is why it goes on a spare port first.
    """
    run(
        client,
        f"HOMBOTD_PORT={port} HOMBOTD_SMARTCONTROL_HOST=127.0.0.1 "
        f"{remote} >/tmp/hombotd-smoke.log 2>&1 & echo $! > /tmp/hombotd-smoke.pid",
    )
    time.sleep(3)
    health = run(client, f"wget -q -O - http://127.0.0.1:{port}/healthz || true", check=False)
    pid = run(client, "cat /tmp/hombotd-smoke.pid", check=False)
    if pid:
        run(client, f"kill {pid} 2>/dev/null || true", check=False)
    if '"status":"ok"' not in health:
        log = run(client, "cat /tmp/hombotd-smoke.log", check=False)
        raise SystemExit(f"smoke test failed on port {port}:\n{health}\n{log}")
    print(f"smoke test ok on port {port}: {health}")


def switch(client, version):
    current = run(client, f"grep -o 'releases/[^/]*/hombotd' {RC_LOCAL} | sort -u")
    print(f"rc.local currently references: {current or '(nothing)'}")
    if f"releases/{version}/hombotd" in current and current.count("releases") == 1:
        print("rc.local already points at this version")
        return

    stamp = "`date +%Y%m%d-%H%M%S 2>/dev/null || echo unknown`"
    run(client, f"mkdir -p {BACKUP_DIR}")
    run(client, f"cp -p {RC_LOCAL} {BACKUP_DIR}/rc.local.pre-{version}-{stamp}")

    staged = f"{RC_LOCAL}.deploy.new"
    run(
        client,
        f"sed 's|releases/[^/]*/hombotd|releases/{version}/hombotd|g' "
        f"{RC_LOCAL} > {staged}",
    )
    run(client, f"sh -n {staged}")

    # Guard the two blocks that must survive a version switch.
    for marker, expected in (("FRANKENHOMO_SERVER", 2), ("FRANKENHOMO_GREETING", 2)):
        count = run(client, f"grep -c {marker} {staged} || true")
        if int(count or 0) != expected:
            run(client, f"rm -f {staged}")
            raise SystemExit(
                f"{marker} block would change ({count} lines, expected {expected}) "
                f"-- refusing to install"
            )

    run(client, f"chmod 755 {staged}")
    run(client, f"mv -f {staged} {RC_LOCAL}")
    run(client, f"chmod 755 {RC_LOCAL}")
    state = run(client, f"[ -x {RC_LOCAL} ] && echo executable || echo BROKEN")
    if state != "executable":
        raise SystemExit(
            f"{RC_LOCAL} is not executable -- restore from {BACKUP_DIR} over the "
            f"serial console before rebooting"
        )
    print(f"rc.local now references releases/{version}/hombotd, mode confirmed")


def restart(client, version):
    old = run(client, "cat /tmp/hombotd.pid 2>/dev/null || true", check=False)
    if old:
        run(client, f"kill {old} 2>/dev/null || true", check=False)
        time.sleep(2)
    run(
        client,
        f"HOMBOTD_PORT=6260 HOMBOTD_SMARTCONTROL_HOST=127.0.0.1 "
        f"{RELEASES}/{version}/hombotd >/tmp/hombotd.log 2>&1 & "
        f"echo $! > /tmp/hombotd.pid",
    )
    time.sleep(3)
    health = run(client, "wget -q -O - http://127.0.0.1:6260/healthz || true", check=False)
    print(f"running: {health}")
    if f'"version":"{version}"' not in health:
        raise SystemExit("restart did not bring up the expected version")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("version")
    parser.add_argument("--rollback-only", action="store_true",
                        help="only switch rc.local and restart, upload nothing")
    args = parser.parse_args()

    client = connect()
    try:
        if not args.rollback_only:
            remote = upload_release(client, args.version)
            smoke_test(client, remote)
        switch(client, args.version)
        restart(client, args.version)
    finally:
        client.close()


if __name__ == "__main__":
    main()
