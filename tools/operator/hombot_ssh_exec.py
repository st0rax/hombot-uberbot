import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "pydeps_legacy"))

import paramiko


def _require(name):
    value = os.environ.get(name)
    if not value:
        raise SystemExit(
            f"{name} is not set. This repo ships no LAN addresses -- "
            f"export {name} for your own network (see docs/OPERATOR_TOOLS.md)."
        )
    return value


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: hombot_ssh_exec.py COMMAND")

    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(
        _require("HOMBOT_HOST"),
        username=os.environ.get("HOMBOT_USER", "root"),
        password=os.environ["HOMBOT_LOGIN_SECRET"],
        look_for_keys=False,
        allow_agent=False,
        timeout=10,
        banner_timeout=10,
        auth_timeout=10,
    )
    _, stdout, stderr = client.exec_command(sys.argv[1], timeout=30)
    exit_code = stdout.channel.recv_exit_status()
    sys.stdout.write(stdout.read().decode("utf-8", errors="replace"))
    sys.stderr.write(stderr.read().decode("utf-8", errors="replace"))
    client.close()
    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()
