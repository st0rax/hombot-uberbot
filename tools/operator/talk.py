"""Hold a conversation through the HomBot: it listens, it answers.

    python talk.py listen 5           # record 5 s from the robot's microphones
    python talk.py say "Hallo Storax" # speak a line through its speaker

The robot's codec is half duplex here -- opening capture while playback runs
truncates the playback -- so listening and speaking are deliberately separate
invocations and never overlap.

Two details the hardware forces:

* The ADC carries a large DC offset (about +600 left, -200 right) that swamps
  quiet speech. It is removed in software; the on-chip high-pass filter does
  not appear to take effect on this board.
* LG pads every spoken prompt with roughly 220 ms of silence, because the
  amplifier swallows the start of a clip. Generated speech gets the same pad.

HOMBOT_LOGIN_SECRET must be set, or work/.hombot_secret must exist.
"""

import argparse
import array
import io
import os
import subprocess
import sys
import tempfile
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
# The ALSA default device is a dmix plugin that only does playback here, so
# capture always has to name the hardware directly.
INTERNAL_CARD_ID = "mostwm8960"
RATE = 16000
REMOTE_DIR = "/usr/data/frankenhomo/sounds"
REMOTE_TMP = "/tmp/talk_in.wav"
LEAD_SILENCE_MS = 300
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


def connect(attempts=6):
    """The robot's WLAN link drops for a few seconds at a time, often enough
    that a single attempt fails regularly. Retry rather than making the caller
    do it."""
    last = None
    for attempt in range(attempts):
        client = paramiko.SSHClient()
        client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        try:
            client.connect(
                HOST,
                username=os.environ.get("HOMBOT_USER", "root"),
                timeout=10,
                banner_timeout=10,
                auth_timeout=10,
                **connect_auth(secret),
            )
            return client
        except Exception as error:
            last = error
            time.sleep(4)
    raise SystemExit(f"robot unreachable after {attempts} attempts: {last}")


def run(client, command, timeout=120, check=True):
    _, stdout, stderr = client.exec_command(command, timeout=timeout)
    code = stdout.channel.recv_exit_status()
    out = stdout.read().decode("utf-8", "replace").strip()
    err = stderr.read().decode("utf-8", "replace").strip()
    if check and code != 0:
        raise SystemExit(f"remote failed ({code}): {command}\n{err or out}")
    return out


def fetch(client, remote):
    _, stdout, _ = client.exec_command(f"cat {remote}", timeout=180)
    payload = stdout.read()
    stdout.channel.recv_exit_status()
    return payload


def push(client, payload, remote):
    _, stdout, stderr = client.exec_command(f"cat > {remote}", timeout=180)
    stdout.channel.sendall(payload)
    stdout.channel.shutdown_write()
    if stdout.channel.recv_exit_status() != 0:
        raise SystemExit(stderr.read().decode("utf-8", "replace"))


def powershell(script):
    result = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if result.returncode != 0:
        raise SystemExit(f"powershell failed:\n{result.stderr.strip()}")
    return result.stdout.strip()


# --------------------------------------------------------------------------- listen

def strongest_mono(stereo):
    """Drop DC per channel and keep whichever microphone heard more."""
    left, right = stereo[0::2], stereo[1::2]
    picked = []
    for channel in (left, right):
        offset = sum(channel) / len(channel)
        centred = [value - offset for value in channel]
        energy = (sum(v * v for v in centred) / len(centred)) ** 0.5
        picked.append((energy, centred, offset))
    picked.sort(key=lambda item: item[0], reverse=True)
    best, other = picked[0], picked[1]
    return best[1], best[0], other[0], best[2]


def find_capture_device(client):
    """Pick a capture device, preferring anything that is not the built-in codec.

    The WM8960 on card 0 has capture hardware but no microphones fitted on this
    board variant, so a USB audio device is always the better choice when one is
    present. Returns (alsa_device, channels, description).
    """
    cards = run(client, "cat /proc/asound/cards 2>/dev/null", check=False)
    candidates = []
    for line in cards.splitlines():
        # " 0 [mostwm8960     ]: WM8960 - most-wm8960"
        stripped = line.strip()
        if not stripped or not stripped[0].isdigit():
            continue
        index = int(stripped.split()[0])
        card_id = stripped.split("[", 1)[1].split("]", 1)[0].strip() if "[" in stripped else ""
        name = stripped.split(":", 1)[1].strip() if ":" in stripped else card_id
        # A card is only useful if it actually exposes a capture substream.
        has_capture = run(
            client,
            f"ls -d /proc/asound/card{index}/pcm*c 2>/dev/null | head -1",
            check=False,
        )
        if not has_capture:
            continue
        device = has_capture.rsplit("/", 1)[-1]          # e.g. "pcm0c"
        subdevice = device[3:-1] or "0"
        candidates.append((card_id, index, subdevice, name))

    if not candidates:
        raise SystemExit(
            "no ALSA capture device found. If a USB microphone is plugged in, "
            "load the modules in /usr/data/frankenhomo/modules/usb-audio/ first."
        )

    # Anything that is not the built-in codec wins.
    candidates.sort(key=lambda entry: entry[0] == INTERNAL_CARD_ID)
    card_id, index, subdevice, name = candidates[0]
    device = f"hw:{index},{subdevice}"

    # USB microphones are usually mono; the WM8960 is a stereo pair. Ask the
    # device rather than assuming, by trying stereo and falling back.
    channels = 2
    probe = run(
        client,
        f"arecord -D {device} -c 2 -r {RATE} -f S16_LE -d 1 /tmp/talk_probe.wav "
        f">/dev/null 2>&1 && echo stereo || echo mono",
        timeout=30,
        check=False,
    )
    if probe.strip() != "stereo":
        channels = 1
    run(client, "rm -f /tmp/talk_probe.wav", check=False)

    internal = " (built-in codec, no microphones fitted)" if card_id == INTERNAL_CARD_ID else ""
    return device, channels, f"{name}{internal}"


def listen(seconds):
    client = connect()
    try:
        device, channels, description = find_capture_device(client)
        print(f"[aufnehmen von {device} -- {description}, {channels} Kanal(e)]")
        run(client, "amixer sset 'Capture' 46,46 cap", check=False)
        run(
            client,
            f"arecord -D {device} -c {channels} -r {RATE} -f S16_LE "
            f"-d {seconds} {REMOTE_TMP}",
            timeout=seconds + 60,
        )
        payload = fetch(client, REMOTE_TMP)
    finally:
        client.close()

    reader = wave.open(io.BytesIO(payload))
    frames = array.array("h")
    frames.frombytes(reader.readframes(reader.getnframes()))
    if reader.getnchannels() == 1:
        offset = sum(frames) / len(frames)
        mono = [value - offset for value in frames]
        loud = (sum(v * v for v in mono) / len(mono)) ** 0.5
        quiet = 0.0
    else:
        mono, loud, quiet, offset = strongest_mono(frames)

    peak = max(abs(min(mono)), abs(max(mono))) or 1
    gain = min(TARGET_PEAK / peak, 60.0)
    boosted = array.array(
        "h", (max(-32768, min(32767, int(v * gain))) for v in mono)
    )

    path = os.path.join(tempfile.gettempdir(), "hombot_listen.wav")
    with wave.open(path, "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(RATE)
        out.writeframes(boosted.tobytes())

    print(
        f"[{seconds}s  lauteres Mikrofon rms={loud:.1f}  anderes={quiet:.1f}  "
        f"DC={offset:.0f}  Verstaerkung x{gain:.0f}]"
    )
    if loud < 3:
        print("[fast nur Rauschen -- vermutlich hat niemand gesprochen]")

    heard = powershell(
        "Add-Type -AssemblyName System.Speech; "
        "$e = New-Object System.Speech.Recognition.SpeechRecognitionEngine("
        "[System.Globalization.CultureInfo]'de-DE'); "
        "$e.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar)); "
        f"$e.SetInputToWaveFile('{path}'); "
        "$r = $e.Recognize(); "
        "if ($r) { [string]$r.Confidence; $r.Text } else { 'NICHTS'; '' }"
    )
    lines = heard.splitlines()
    confidence = lines[0] if lines else "NICHTS"
    text = lines[1].strip() if len(lines) > 1 else ""
    print(f"Sicherheit: {confidence}")
    print(f"Verstanden: {text or '(nichts)'}")
    return text


# ----------------------------------------------------------------------------- say

def synthesise(text):
    wav = os.path.join(tempfile.gettempdir(), "hombot_say.wav")
    escaped = text.replace("'", "''")
    powershell(
        "Add-Type -AssemblyName System.Speech; "
        "$f = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo("
        f"{RATE}, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, "
        "[System.Speech.AudioFormat.AudioChannel]::Mono); "
        "$s = New-Object System.Speech.Synthesis.SpeechSynthesizer; "
        f"$s.SelectVoice('{VOICE}'); "
        f"$s.SetOutputToWaveFile('{wav}', $f); "
        f"$s.Speak('{escaped}'); $s.Dispose()"
    )
    with wave.open(wav, "rb") as reader:
        samples = array.array("h")
        samples.frombytes(reader.readframes(reader.getnframes()))

    peak = max(abs(min(samples)), abs(max(samples))) or 1
    gain = TARGET_PEAK / peak
    pad = array.array("h", [0]) * (RATE * LEAD_SILENCE_MS // 1000)
    body = array.array("h", (max(-32768, min(32767, int(v * gain))) for v in samples))
    return (pad + body).tobytes()


def say(text):
    payload = synthesise(text)
    remote = f"{REMOTE_DIR}/talk_out.snd"
    client = connect()
    try:
        run(client, f"mkdir -p {REMOTE_DIR}")
        push(client, payload, remote)
        run(client, f"aplay -q -c 1 -r {RATE} -f S16_LE {remote}", timeout=120)
    finally:
        client.close()
    seconds = len(payload) / 2 / RATE
    print(f'gesagt ({seconds:.2f}s): "{text}"')


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="mode", required=True)
    hear = sub.add_parser("listen")
    hear.add_argument("seconds", nargs="?", type=int, default=5)
    speak = sub.add_parser("say")
    speak.add_argument("text")
    args = parser.parse_args()

    if args.mode == "listen":
        listen(max(1, min(args.seconds, 30)))
    else:
        say(args.text)


if __name__ == "__main__":
    main()
