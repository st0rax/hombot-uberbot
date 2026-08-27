"""Listen for a wake word on the robot and answer out loud.

    python wakeword.py                    # listen until stopped
    python wakeword.py -n 5               # five listening windows
    python wakeword.py --test             # play the answer once and exit

The robot records through the USB audio device in its hub, this PC does the
recognition, and the robot plays the answer. Recognition uses a fixed word list
rather than dictation: with a search space of a handful of phrases instead of a
hundred thousand words, a weak recogniser becomes usable -- and, more
importantly, it says "not one of mine" instead of inventing something.

The answer clip -- chirp plus sentence -- is built once and uploaded before
listening starts, so reacting is a single aplay with nothing to transfer.
"""

import argparse
import array
import http.server
import io
import math
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
SERVE_IP = _require("HOMBOT_SERVE_IP")
SERVE_PORT = int(os.environ.get("HOMBOT_SERVE_PORT", "8099"))
RATE = 16000
VOICE = os.environ.get("HOMBOT_VOICE", "Microsoft Hedda Desktop")

# Spelling variants matter more than the word count here: the recogniser has to
# match German phonetics, and "Homebot" is not a German word.
WAKE_WORDS = [
    "homebot", "hom bot", "home bot", "hombot",
    "hallo homebot", "hey homebot",
]
ANSWER = "Ja Meister, hast du mich gerufen?"


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
                HOST, username=os.environ.get("HOMBOT_USER", "root"),
                timeout=20, banner_timeout=20, auth_timeout=20,
                **connect_auth(secret),
            )
            return client
        except Exception as error:
            last = error
            time.sleep(3)
    raise SystemExit(f"robot unreachable: {last}")


def run(client, command, timeout=180):
    _, stdout, stderr = client.exec_command(command, timeout=timeout)
    stdout.channel.recv_exit_status()
    return (stdout.read().decode("utf-8", "replace").strip()
            + stderr.read().decode("utf-8", "replace").strip())


def usb_card(client):
    """The USB audio device, which is the only one not held by LG's app."""
    for line in run(client, "cat /proc/asound/cards").splitlines():
        stripped = line.strip()
        if stripped and stripped[0].isdigit() and "usb" in line.lower():
            return int(stripped.split()[0])
    return None


# ------------------------------------------------------------------ the answer

def chirp():
    """A short rising warble. Two tones with vibrato, which is enough to read
    as 'robot noticed you' without sounding like an error beep."""
    samples = array.array("h")

    def tone(start_hz, end_hz, seconds, depth=0.0, wobble_hz=0.0):
        count = int(RATE * seconds)
        for index in range(count):
            position = index / count
            frequency = start_hz + (end_hz - start_hz) * position
            if depth:
                frequency += depth * math.sin(2 * math.pi * wobble_hz * index / RATE)
            # Short fades keep the tone from clicking at the joins.
            envelope = min(1.0, position * 12, (1.0 - position) * 12)
            value = math.sin(2 * math.pi * frequency * index / RATE)
            samples.append(int(max(-1.0, min(1.0, value)) * envelope * 11000))

    tone(620, 1180, 0.11, depth=60, wobble_hz=32)
    samples.extend(array.array("h", [0]) * int(RATE * 0.035))
    tone(1180, 1720, 0.09, depth=90, wobble_hz=45)
    samples.extend(array.array("h", [0]) * int(RATE * 0.12))
    return samples


def speech(text):
    scratch = os.path.join(tempfile.gettempdir(), "wake_say.wav")
    escaped = text.replace("'", "''")
    subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "Add-Type -AssemblyName System.Speech;"
         "$f=New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo("
         f"{RATE},[System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,"
         "[System.Speech.AudioFormat.AudioChannel]::Mono);"
         "$s=New-Object System.Speech.Synthesis.SpeechSynthesizer;"
         f"$s.SelectVoice('{VOICE}');$s.SetOutputToWaveFile('{scratch}',$f);"
         f"$s.Speak('{escaped}');$s.Dispose()"],
        capture_output=True, text=True,
    )
    if not os.path.exists(scratch):
        raise SystemExit("speech synthesis produced nothing")
    with wave.open(scratch, "rb") as reader:
        samples = array.array("h")
        samples.frombytes(reader.readframes(reader.getnframes()))
    peak = max(abs(min(samples)), abs(max(samples))) or 1
    gain = 28000 / peak
    return array.array("h", (max(-32768, min(32767, int(v * gain))) for v in samples))


def build_answer(path, text):
    # 200 ms of silence first: the amplifier swallows the start of a clip, which
    # is why every LG prompt on this device is padded the same way.
    payload = array.array("h", [0]) * (RATE // 5)
    payload.extend(chirp())
    payload.extend(speech(text))
    with open(path, "wb") as handle:
        handle.write(payload.tobytes())
    return len(payload) * 2


# ------------------------------------------------------------------ the ear

def recognise(wave_bytes, words):
    reader = wave.open(io.BytesIO(wave_bytes))
    rate = reader.getframerate()
    frames = array.array("h")
    frames.frombytes(reader.readframes(reader.getnframes()))
    offset = sum(frames) / len(frames)
    centred = [value - offset for value in frames]
    rms = (sum(v * v for v in centred) / len(centred)) ** 0.5
    peak = max(abs(min(centred)), abs(max(centred))) or 1

    gain = min(28000 / peak, 40.0)
    boosted = array.array(
        "h", (max(-32768, min(32767, int(v * gain))) for v in centred)
    )
    path = os.path.join(tempfile.gettempdir(), "wake_in.wav")
    with wave.open(path, "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        handle.writeframes(boosted.tobytes())

    choices = "','".join(words)
    result = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "Add-Type -AssemblyName System.Speech;"
         "$e=New-Object System.Speech.Recognition.SpeechRecognitionEngine("
         "[System.Globalization.CultureInfo]'de-DE');"
         "$c=New-Object System.Speech.Recognition.Choices;"
         f"$c.Add([string[]]@('{choices}'));"
         "$g=New-Object System.Speech.Recognition.GrammarBuilder($c);"
         "$e.LoadGrammar((New-Object System.Speech.Recognition.Grammar($g)));"
         f"$e.SetInputToWaveFile('{path}');$r=$e.Recognize();"
         "if ($r) { [string]$r.Confidence; $r.Text } else { '0'; '' }"],
        capture_output=True, text=True, encoding="utf-8",
    )
    lines = (result.stdout or "").strip().splitlines()
    try:
        confidence = float(lines[0]) if lines else 0.0
    except ValueError:
        confidence = 0.0
    heard = lines[1].strip() if len(lines) > 1 else ""
    return confidence, heard, rms, peak


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("-n", "--rounds", type=int, default=0,
                        help="listening windows; 0 means until interrupted")
    parser.add_argument("-s", "--seconds", type=int, default=4,
                        help="length of each listening window")
    parser.add_argument("-t", "--threshold", type=float, default=0.35,
                        help="confidence needed to count as a hit")
    parser.add_argument("--phrase", default=ANSWER)
    parser.add_argument("--test", action="store_true",
                        help="play the answer once and exit")
    args = parser.parse_args()

    serve_dir = tempfile.mkdtemp(prefix="wake_")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *a, **k):
            super().__init__(*a, directory=serve_dir, **k)

        def log_message(self, *a):
            pass

    size = build_answer(os.path.join(serve_dir, "answer.snd"), args.phrase)
    socketserver.TCPServer.allow_reuse_address = True
    httpd = socketserver.TCPServer(("0.0.0.0", SERVE_PORT), Handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

    client = connect()
    try:
        card = usb_card(client)
        if card is None:
            raise SystemExit(
                "Keine USB-Audiokarte am Bot. Box einstecken und die Module "
                "unter /usr/data/frankenhomo/modules/usb-audio laden."
            )

        got = run(client, f"rm -f /tmp/answer.snd; wget -q -O /tmp/answer.snd "
                          f"http://{SERVE_IP}:{SERVE_PORT}/answer.snd 2>&1; "
                          f"wc -c < /tmp/answer.snd")
        if not got.strip().isdigit() or int(got.strip()) != size:
            raise SystemExit(f"Antwort nicht vollstaendig uebertragen: {got!r} von {size}")
        print(f"Antwort auf dem Bot ({size} Bytes, {size / 2 / RATE:.1f}s), "
              f"Karte {card}")

        def answer():
            run(client, f"aplay -q -D plughw:{card},0 -c 1 -r {RATE} "
                        f"-f S16_LE /tmp/answer.snd")

        if args.test:
            answer()
            print(f'Antwort abgespielt: Chirp + "{args.phrase}"')
            return

        print(f'Warte auf "{WAKE_WORDS[0]}" -- Fenster {args.seconds}s, '
              f'Schwelle {args.threshold}. Abbruch mit Strg-C.')
        round_number = 0
        while args.rounds == 0 or round_number < args.rounds:
            round_number += 1
            run(client, f"rm -f /tmp/wake.wav; arecord -D plughw:{card},0 -c 1 "
                        f"-r {RATE} -f S16_LE -d {args.seconds} /tmp/wake.wav "
                        f">/dev/null 2>&1", timeout=args.seconds + 60)
            _, stdout, _ = client.exec_command("cat /tmp/wake.wav", timeout=120)
            data = stdout.read()
            stdout.channel.recv_exit_status()
            if len(data) < 1000:
                print(f"  [{round_number}] Aufnahme unvollstaendig")
                continue

            confidence, heard, rms, peak = recognise(data, WAKE_WORDS)
            flag = ""
            if peak > 32000:
                flag = " (uebersteuert)"
            elif rms < 5:
                flag = " (still)"
            if heard and confidence >= args.threshold:
                print(f"  [{round_number}] rms={rms:.0f}{flag}  "
                      f"GERUFEN: '{heard}' ({confidence:.2f})")
                answer()
            else:
                detail = f"'{heard}' {confidence:.2f}" if heard else "-"
                print(f"  [{round_number}] rms={rms:.0f}{flag}  {detail}")
    except KeyboardInterrupt:
        print("\nbeendet")
    finally:
        client.close()
        httpd.shutdown()


if __name__ == "__main__":
    main()
