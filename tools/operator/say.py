"""Make the robot say something out loud.

    python say.py "Hallo Paul"

The file goes over HTTP rather than through the ssh channel on purpose. Writing
to stdin of a remote command stopped working on this device -- `cat > file`
fails instantly with "Socket is closed" even for four kilobytes, while `echo >`
still works and reading a 192 KB file back works fine. Having the robot fetch
the audio itself sidesteps that completely and carried 259 KB in one go.

The announcement is 16 kHz mono raw PCM, the format every LG prompt on the
device uses, with 200 ms of leading silence because the amplifier swallows the
start of a clip.
"""

import argparse
import array
import http.server
import os
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import wave

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

HOST = _require("HOMBOT_HOST")
# The address the robot has to reach us on. Deliberately not derived from the
# hostname: that resolves to a VirtualBox adapter on this machine.
SERVE_IP = _require("HOMBOT_SERVE_IP")
SERVE_PORT = int(os.environ.get("HOMBOT_SERVE_PORT", "8099"))
RATE = 16000
LEAD_SILENCE_MS = 200
TARGET_PEAK = 30000
VOICE = os.environ.get("HOMBOT_VOICE", "Microsoft Hedda Desktop")


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
    last = None
    for _ in range(attempts):
        client = paramiko.SSHClient()
        client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        try:
            client.connect(
                HOST,
                username=os.environ.get("HOMBOT_USER", "root"),
                timeout=20,
                banner_timeout=20,
                auth_timeout=20,
                **connect_auth(secret),
            )
            return client
        except Exception as error:  # the WLAN link drops for seconds at a time
            last = error
            time.sleep(3)
    raise SystemExit(f"robot unreachable: {last}")


def run(client, command, timeout=180):
    _, stdout, stderr = client.exec_command(command, timeout=timeout)
    stdout.channel.recv_exit_status()
    return (
        stdout.read().decode("utf-8", "replace").strip()
        + stderr.read().decode("utf-8", "replace").strip()
    )


def synthesise(text, path):
    """Windows speech synthesis straight to 16 kHz mono, then level-matched."""
    scratch = os.path.join(tempfile.gettempdir(), "hombot_say.wav")
    escaped = text.replace("'", "''")
    result = subprocess.run(
        [
            "powershell",
            "-NoProfile",
            "-Command",
            "Add-Type -AssemblyName System.Speech;"
            "$f=New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo("
            f"{RATE},[System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,"
            "[System.Speech.AudioFormat.AudioChannel]::Mono);"
            "$s=New-Object System.Speech.Synthesis.SpeechSynthesizer;"
            f"$s.SelectVoice('{VOICE}');"
            f"$s.SetOutputToWaveFile('{scratch}',$f);"
            f"$s.Speak('{escaped}');$s.Dispose()",
        ],
        capture_output=True,
        text=True,
    )
    if not os.path.exists(scratch):
        raise SystemExit(f"speech synthesis failed:\n{result.stderr.strip()}")

    with wave.open(scratch, "rb") as reader:
        samples = array.array("h")
        samples.frombytes(reader.readframes(reader.getnframes()))
    peak = max(abs(min(samples)), abs(max(samples))) or 1
    gain = TARGET_PEAK / peak
    pad = array.array("h", [0]) * (RATE * LEAD_SILENCE_MS // 1000)
    body = array.array("h", (max(-32768, min(32767, int(v * gain))) for v in samples))
    payload = (pad + body).tobytes()
    with open(path, "wb") as handle:
        handle.write(payload)
    return len(payload)


def play_anywhere(client):
    """Play the clip on the first output whose playback stream is actually free.

    LG's own application holds the built-in codec's single playback substream
    for long stretches. Handing a busy device to aplay does not fail -- it
    blocks, which hangs the whole call, so the state is read from
    /proc/asound/cardN/pcm0p/sub0/status first and busy cards are skipped.
    """
    listing = run(client, "aplay -l 2>/dev/null")
    cards = []
    for line in listing.splitlines():
        if not line.startswith("card "):
            continue
        index = int(line.split()[1].rstrip(":"))
        label = line.split("[", 1)[1].split("]", 1)[0] if "[" in line else str(index)
        if index not in [existing for existing, _ in cards]:
            cards.append((index, label))
    if not cards:
        cards = [(0, "default")]

    skipped = []
    for index, label in cards:
        status = run(
            client,
            f"head -1 /proc/asound/card{index}/pcm0p/sub0/status 2>/dev/null",
        )
        # "closed" means nothing holds it; anything else is an owner.
        if status.strip() and status.strip() != "closed":
            skipped.append(f"card {index} ({label}) busy: {status.strip()}")
            continue
        # plughw rather than hw: USB audio devices here only run at 48 kHz and
        # will not resample a 16 kHz clip on their own.
        result = run(
            client,
            f"aplay -q -D plughw:{index},0 -c 1 -r {RATE} -f S16_LE "
            f"/tmp/say.snd 2>&1 && echo PLAYED",
        )
        if result.strip().endswith("PLAYED"):
            return f"card {index} ({label})"
        skipped.append(f"card {index} ({label}): {result.strip() or 'no output'}")

    raise SystemExit("no free playback device: " + "; ".join(skipped))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("text", nargs="+")
    parser.add_argument("--keep", action="store_true",
                        help="leave the clip on the robot as /tmp/say.snd")
    args = parser.parse_args()
    text = " ".join(args.text)

    serve_dir = tempfile.mkdtemp(prefix="hombot_say_")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *a, **k):
            super().__init__(*a, directory=serve_dir, **k)

        def log_message(self, *a):
            pass

    size = synthesise(text, os.path.join(serve_dir, "say.snd"))

    socketserver.TCPServer.allow_reuse_address = True
    httpd = socketserver.TCPServer(("0.0.0.0", SERVE_PORT), Handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

    client = connect()
    try:
        fetched = run(
            client,
            f"rm -f /tmp/say.snd; "
            f"wget -q -O /tmp/say.snd http://{SERVE_IP}:{SERVE_PORT}/say.snd 2>&1; "
            f"wc -c < /tmp/say.snd",
        )
        if not fetched.strip().isdigit() or int(fetched.strip()) != size:
            raise SystemExit(
                f"robot fetched {fetched!r} of {size} bytes -- is {SERVE_IP} the "
                f"address it can reach this machine on?"
            )
        played = play_anywhere(client)
        if not args.keep:
            run(client, "rm -f /tmp/say.snd")
    finally:
        client.close()
        httpd.shutdown()

    print(f'gesagt ({size / 2 / RATE:.1f}s) ueber {played}: "{text}"')


if __name__ == "__main__":
    main()
