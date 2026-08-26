"""Append timestamp + raw_record_hex (+ optional label) from GET /api/v1/sensors.

    python log_sensors.py
    python log_sensors.py -o capture.jsonl -i 0.5
    python log_sensors.py --label bumper
    python log_sensors.py --prompt-labels   # type a new label on stdin, empty clears it

Does not decode the 158-byte frame. Stores the hex exactly as the API returned it.
Null/missing samples are logged as null, never invented.
"""

import argparse
import json
import os
import sys
import threading
import time
from datetime import datetime, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "pydeps_legacy"))

import paramiko  # noqa: E402

SENSORS_CMD = "wget -q -O - http://127.0.0.1:6260/api/v1/sensors || true"


def _require(name):
    value = os.environ.get(name)
    if not value:
        raise SystemExit(
            f"{name} is not set. This repo ships no LAN addresses -- "
            f"export {name} for your own network (see docs/OPERATOR_TOOLS.md)."
        )
    return value


def secret():
    value = os.environ.get("HOMBOT_LOGIN_SECRET")
    if value:
        return value
    path = os.path.join(HERE, ".hombot_secret")
    if os.path.exists(path):
        with open(path, encoding="utf-8") as handle:
            return handle.read().strip()
    raise SystemExit("No credential: set HOMBOT_LOGIN_SECRET or create .hombot_secret")


def connect(attempts=12):
    host = _require("HOMBOT_HOST")
    last = None
    for _ in range(attempts):
        client = paramiko.SSHClient()
        client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        try:
            client.connect(
                host, username=os.environ.get("HOMBOT_USER", "root"),
                password=secret(), look_for_keys=False, allow_agent=False,
                timeout=20, banner_timeout=20, auth_timeout=20,
            )
            return client
        except Exception as error:
            last = error
            time.sleep(3)
    raise SystemExit(f"robot unreachable: {last}")


def run(client, command, timeout=30):
    _, stdout, stderr = client.exec_command(command, timeout=timeout)
    stdout.channel.recv_exit_status()
    return (stdout.read().decode("utf-8", "replace").strip()
            + stderr.read().decode("utf-8", "replace").strip())


def extract_hex(body):
    if not body:
        return None
    try:
        payload = json.loads(body)
    except json.JSONDecodeError:
        return None
    if not isinstance(payload, dict):
        return None
    value = payload.get("raw_record_hex")
    if value is None:
        return None
    if not isinstance(value, str):
        return None
    return value


def parse_args():
    parser = argparse.ArgumentParser(
        description="Log GET /api/v1/sensors raw_record_hex. Does not decode fields."
    )
    parser.add_argument(
        "-o", "--output", default="sensors_hex.jsonl",
        help="local JSONL file (default: sensors_hex.jsonl)",
    )
    parser.add_argument(
        "-i", "--interval", type=float, default=0.5,
        help="seconds between polls (default: 0.5)",
    )
    parser.add_argument(
        "--label", default="",
        help="optional label stamped on every line until changed",
    )
    parser.add_argument(
        "--prompt-labels", action="store_true",
        help="read new labels from stdin while logging; empty line clears the label",
    )
    return parser.parse_args()


def start_label_reader(state):
    def loop():
        for line in sys.stdin:
            state["label"] = line.strip()
    thread = threading.Thread(target=loop, name="labels", daemon=True)
    thread.start()


def main():
    args = parse_args()
    if args.interval <= 0:
        raise SystemExit("--interval must be > 0")
    state = {"label": args.label}
    if args.prompt_labels:
        start_label_reader(state)

    client = connect()
    print(
        f"logging {args.output} every {args.interval}s. "
        f"Ctrl-C stops. Labels via --label or stdin (--prompt-labels).",
        file=sys.stderr,
    )
    try:
        with open(args.output, "a", encoding="utf-8") as handle:
            while True:
                try:
                    body = run(client, SENSORS_CMD)
                except Exception as error:
                    try:
                        client.close()
                    except Exception:
                        pass
                    print(f"ssh dropped ({error}); reconnecting", file=sys.stderr)
                    client = connect()
                    continue
                record = {
                    "ts": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ"),
                    "raw_record_hex": extract_hex(body),
                    "label": state["label"] or None,
                }
                handle.write(json.dumps(record, ensure_ascii=True) + "\n")
                handle.flush()
                time.sleep(args.interval)
    except KeyboardInterrupt:
        print("stopped", file=sys.stderr)
    finally:
        try:
            client.close()
        except Exception:
            pass


if __name__ == "__main__":
    main()
