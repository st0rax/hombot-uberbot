"""Put a spoken greeting on the HomBot and play it.

The robot's own prompts are headerless 16 kHz mono S16_LE PCM, which is what
`aplay` is invoked with everywhere in LG's scripts. This uploads a file in that
same format, verifies it arrived byte for byte, and plays it once.

    python deploy_greeting.py hallo_storax.snd            # upload + play once
    python deploy_greeting.py hallo_storax.snd --at-boot  # also greet on start
    python deploy_greeting.py --remove-boot               # undo the boot change

Nothing is written into LG's own /usr/SNDDATA. The file lives under
/usr/data/frankenhomo/sounds/, and the boot hook goes inside the existing
FRANKENHOMO block in rc.local, which is backed up before it is touched.

HOMBOT_LOGIN_SECRET must be set, or work/.hombot_secret must exist.
"""

import argparse
import hashlib
import os
import posixpath
import sys

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

REMOTE_DIR = "/usr/data/frankenhomo/sounds"
RC_LOCAL = "/usr/etc/rc.local"
BACKUP_DIR = "/usr/data/frankenhomo-backup"
BOOT_MARKER = "# FRANKENHOMO_GREETING"
APLAY = "aplay -q -c 1 -r 16000 -f S16_LE"


def secret():
    value = os.environ.get("HOMBOT_LOGIN_SECRET")
    if value:
        return value
    path = os.path.join(HERE, ".hombot_secret")
    if os.path.exists(path):
        with open(path, encoding="utf-8") as handle:
            return handle.read().strip()
    raise SystemExit(
        "No credential. Set HOMBOT_LOGIN_SECRET, or put the password in "
        f"{path} (that file is read directly and never printed)."
    )


def connect():
    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(
        _require("HOMBOT_HOST"),
        username=os.environ.get("HOMBOT_USER", "root"),
        timeout=10,
        banner_timeout=10,
        auth_timeout=10,
        **connect_auth(secret),
    )
    return client


def run(client, command, check=True):
    _, stdout, stderr = client.exec_command(command, timeout=60)
    code = stdout.channel.recv_exit_status()
    out = stdout.read().decode("utf-8", "replace").strip()
    err = stderr.read().decode("utf-8", "replace").strip()
    if check and code != 0:
        raise SystemExit(f"remote command failed ({code}): {command}\n{err or out}")
    return code, out, err


def upload(client, local, remote):
    with open(local, "rb") as handle:
        payload = handle.read()
    if len(payload) % 2:
        raise SystemExit(f"{local}: odd byte count -- not 16-bit PCM")

    run(client, f"mkdir -p {REMOTE_DIR}")

    # Dropbear here has no SFTP subsystem and busybox has no base64, so the
    # bytes go straight down the command channel. SSH channels are 8-bit clean;
    # md5sum afterwards is what actually proves the transfer.
    _, stdout, stderr = client.exec_command(f"cat > {remote}", timeout=120)
    channel = stdout.channel
    channel.sendall(payload)
    channel.shutdown_write()
    code = channel.recv_exit_status()
    if code != 0:
        err = stderr.read().decode("utf-8", "replace").strip()
        raise SystemExit(f"upload failed ({code}): {err}")

    digest = hashlib.md5(payload).hexdigest()
    _, out, _ = run(client, f"md5sum {remote}")
    remote_digest = out.split()[0] if out else ""
    if remote_digest != digest:
        raise SystemExit(f"upload mismatch: local {digest}, remote {remote_digest}")
    _, size, _ = run(client, f"wc -c < {remote}")
    if int(size.split()[0]) != len(payload):
        raise SystemExit(f"upload truncated: {size} of {len(payload)} bytes")

    seconds = len(payload) / 2 / 16000
    print(f"uploaded {remote}  {len(payload)} bytes  {seconds:.2f} s")


def install_boot(client, remote):
    _, present, _ = run(client, f"grep -c '{BOOT_MARKER}' {RC_LOCAL} || true")
    if present.strip() not in ("", "0"):
        print("boot greeting already installed")
        return

    _, anchor, _ = run(
        client, f"grep -c '^# FRANKENHOMO_SERVER_START$' {RC_LOCAL} || true"
    )
    if anchor.strip() in ("", "0"):
        raise SystemExit(
            "FRANKENHOMO_SERVER_START block not found in rc.local -- refusing to "
            "guess where the greeting belongs."
        )

    run(client, f"mkdir -p {BACKUP_DIR}")
    stamp = "`date +%Y%m%d-%H%M%S 2>/dev/null || echo unknown`"
    backup = f"{BACKUP_DIR}/rc.local.pre-greeting-{stamp}"
    run(client, f"cp -p {RC_LOCAL} {backup}")

    # Greet after the server block, in the background, so a missing sound file
    # or a busy audio device can never hold up the rest of the boot.
    block = "\\n".join(
        [
            BOOT_MARKER,
            f"if [ -f {remote} ]",
            "then",
            f"  ({APLAY} {remote} >/dev/null 2>&1) &",
            "fi",
            f"{BOOT_MARKER}_END",
        ]
    )
    run(
        client,
        f"awk '/^# FRANKENHOMO_SERVER_START$/ && !done "
        f'{{ print; print "{block}"; done=1; next }} {{ print }}\' '
        f"{RC_LOCAL} > {RC_LOCAL}.greeting.new",
    )
    run(client, f"sh -n {RC_LOCAL}.greeting.new")
    install_rc_local(client)
    print(f"boot greeting installed; previous rc.local saved under {BACKUP_DIR}")


def install_rc_local(client):
    """Moves the staged rc.local into place with its execute bit intact.

    /etc/rc.d/rcS starts it with `if [ -x /usr/etc/rc.local ]`, and a file
    created by shell redirection is not executable. Losing that bit does not
    fail loudly -- the robot simply boots without ever running rc.local, so
    there is no SSH left to fix it with. Set the mode explicitly and refuse to
    continue unless the robot agrees the file is executable.
    """
    run(client, f"chmod 755 {RC_LOCAL}.greeting.new")
    run(client, f"mv {RC_LOCAL}.greeting.new {RC_LOCAL}")
    run(client, f"chmod 755 {RC_LOCAL}")
    _, mode, _ = run(client, f"[ -x {RC_LOCAL} ] && echo executable || echo BROKEN")
    if mode.strip() != "executable":
        raise SystemExit(
            f"{RC_LOCAL} is not executable after install -- the robot would "
            f"boot without it. Restore from {BACKUP_DIR} over the serial "
            f"console before rebooting."
        )


def remove_boot(client):
    _, present, _ = run(client, f"grep -c '{BOOT_MARKER}' {RC_LOCAL} || true")
    if present.strip() in ("", "0"):
        print("no boot greeting installed")
        return
    run(
        client,
        f"awk '/^{BOOT_MARKER}$/ {{ skip=1 }} "
        f"/^{BOOT_MARKER}_END$/ {{ skip=0; next }} "
        f"!skip {{ print }}' {RC_LOCAL} > {RC_LOCAL}.greeting.new",
    )
    run(client, f"sh -n {RC_LOCAL}.greeting.new")
    install_rc_local(client)
    print("boot greeting removed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("sound", nargs="?", help="local .snd file (16 kHz mono S16_LE)")
    parser.add_argument("--at-boot", action="store_true", help="also greet on startup")
    parser.add_argument("--remove-boot", action="store_true")
    parser.add_argument("--no-play", action="store_true")
    args = parser.parse_args()

    client = connect()
    try:
        if args.remove_boot:
            remove_boot(client)
            return

        if not args.sound:
            parser.error("give a .snd file, or use --remove-boot")

        remote = posixpath.join(REMOTE_DIR, os.path.basename(args.sound))
        upload(client, args.sound, remote)

        if not args.no_play:
            code, _, err = run(client, f"{APLAY} {remote}", check=False)
            print("played" if code == 0 else f"aplay failed ({code}): {err}")

        if args.at_boot:
            install_boot(client, remote)
    finally:
        client.close()


if __name__ == "__main__":
    main()
