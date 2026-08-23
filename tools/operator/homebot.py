"""Wake word and spoken commands for the HomBot.

    python homebot.py                 # listen until interrupted
    python homebot.py -n 3            # three listening windows
    python homebot.py --say "text"    # speak something once
    python homebot.py --list          # show the phrases it knows

The robot records through the USB audio device in its hub and plays the answers
there too, because LG's own application holds the built-in codec's single
playback substream. This PC only does the recognition.

Recognition uses a closed phrase list, not dictation. That is the whole trick:
with a search space of a dozen phrases instead of a hundred thousand words, a
weak recogniser becomes usable, and -- more valuable for a robot -- it answers
"not one of mine" instead of inventing something. Free dictation on the same
audio returned "Weiterhin" at confidence 0.001.

Answers that never change are built and uploaded once at startup, so reacting
is a single aplay with nothing to transfer. Answers that depend on the robot's
state are synthesised when asked.
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

# Phrase -> intent. Spelling variants are here because the recogniser matches
# German phonetics and "Homebot" is not a German word; each spelling gives it
# another pronunciation to match against.
PHRASES = {
    "homobot": "wake",
    "homo bot": "wake",
    "homobott": "wake",
    "hallo homobot": "wake",
    "hey homobot": "wake",
    "ey homo": "wake",
    "hey homo": "wake",
    "ey homobot": "wake",
    "homebot": "wake",
    "hombot": "wake",

    "homobot wie spät ist es": "time",
    "homebot wie spät ist es": "time",
    "hombot wie spät ist es": "time",
    "wie spät ist es": "time",
    "wie viel uhr ist es": "time",

    "homobot was hast du heute gemacht": "today",
    "homebot was hast du heute gemacht": "today",
    "hombot was hast du heute gemacht": "today",
    "was hast du heute gemacht": "today",
    "was hast du heute so gemacht": "today",
    "wie geht es dir": "today",

    "homobot erkunde die gegend": "drive",
    "homebot erkunde die gegend": "drive",
    "hombot erkunde die gegend": "drive",
    "erkunde die gegend": "drive",
    "homobot komm mit": "drive",
    "homebot komm mit": "drive",
    "hombot komm mit": "drive",
    "komm mit": "drive",
    "komm her": "drive",
    "fahr los": "drive",
}

STATIC = {
    "wake": "Ja Meister, hast du mich gerufen?",
    "drive": "Fahren kann ich noch nicht, Meister. Mir fehlt der Befehlsweg zur "
             "Motorsteuerung. Aber hören und reden kann ich schon.",
}


# ------------------------------------------------------------------- robot

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
                password=secret(), look_for_keys=False, allow_agent=False,
                timeout=20, banner_timeout=20, auth_timeout=20,
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
    for line in run(client, "cat /proc/asound/cards").splitlines():
        stripped = line.strip()
        if stripped and stripped[0].isdigit() and "usb" in line.lower():
            return int(stripped.split()[0])
    return None


# ------------------------------------------------------------------ audio

def chirp():
    """A short rising warble, so an answer is recognisable before the words."""
    samples = array.array("h")

    def tone(start_hz, end_hz, seconds, depth=0.0, wobble_hz=0.0):
        count = int(RATE * seconds)
        for index in range(count):
            position = index / count
            frequency = start_hz + (end_hz - start_hz) * position
            if depth:
                frequency += depth * math.sin(2 * math.pi * wobble_hz * index / RATE)
            envelope = min(1.0, position * 12, (1.0 - position) * 12)
            value = math.sin(2 * math.pi * frequency * index / RATE)
            samples.append(int(max(-1.0, min(1.0, value)) * envelope * 11000))

    tone(620, 1180, 0.11, depth=60, wobble_hz=32)
    samples.extend(array.array("h", [0]) * int(RATE * 0.035))
    tone(1180, 1720, 0.09, depth=90, wobble_hz=45)
    samples.extend(array.array("h", [0]) * int(RATE * 0.12))
    return samples


def speech(text):
    scratch = os.path.join(tempfile.gettempdir(), "homebot_say.wav")
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


def clip(text):
    """Chirp plus sentence, with the 200 ms of leading silence the amplifier
    needs -- every LG prompt on this device is padded the same way."""
    payload = array.array("h", [0]) * (RATE // 5)
    payload.extend(chirp())
    payload.extend(speech(text))
    return payload.tobytes()


# ------------------------------------------------------------- recognition

def recognise(wave_bytes, phrases):
    reader = wave.open(io.BytesIO(wave_bytes))
    rate = reader.getframerate()
    frames = array.array("h")
    frames.frombytes(reader.readframes(reader.getnframes()))
    offset = sum(frames) / len(frames)
    centred = [value - offset for value in frames]
    rms = (sum(v * v for v in centred) / len(centred)) ** 0.5
    peak = max(abs(min(centred)), abs(max(centred))) or 1

    gain = min(28000 / peak, 40.0)
    boosted = array.array("h", (max(-32768, min(32767, int(v * gain))) for v in centred))
    path = os.path.join(tempfile.gettempdir(), "homebot_in.wav")
    with wave.open(path, "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        handle.writeframes(boosted.tobytes())

    choices = "','".join(phrases)
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
    heard = lines[1].strip().lower() if len(lines) > 1 else ""
    return confidence, heard, rms, peak


# ---------------------------------------------------------------- answers

def answer_time(client):
    clock = run(client, "date '+%H %M'").split()
    if len(clock) != 2:
        return "Meine Uhr ist mir gerade abhanden gekommen."
    hour, minute = int(clock[0]), int(clock[1])
    return f"Es ist {hour} Uhr {minute}." if minute else f"Es ist genau {hour} Uhr."


def answer_today(client):
    uptime = run(client, "cut -d. -f1 /proc/uptime")
    status = run(client, "wget -q -O - http://127.0.0.1:6260/api/v1/status || true")
    parts = []
    try:
        minutes = int(uptime) // 60
        parts.append(f"Ich laufe seit {minutes} Minuten" if minutes
                     else "Ich bin gerade erst aufgewacht")
    except ValueError:
        pass
    if '"robot_state":"CHARGING"' in status:
        parts.append("und hänge an der Ladestation")
    if '"smartcontrol":"connected"' in status:
        parts.append("meine Steuerung ist verbunden")
    parts.append("und heute habe ich zum ersten Mal gehört und gesprochen")
    return ", ".join(parts) + "."


# -------------------------------------------------------------------- main

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("-n", "--rounds", type=int, default=0)
    parser.add_argument("-s", "--seconds", type=int, default=4)
    parser.add_argument("-t", "--threshold", type=float, default=0.30)
    parser.add_argument("--say")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()

    if args.list:
        for intent in ("wake", "time", "today", "drive"):
            said = [p for p, i in PHRASES.items() if i == intent]
            print(f"{intent:8} {', '.join(said)}")
        return

    serve_dir = tempfile.mkdtemp(prefix="homebot_")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *a, **k):
            super().__init__(*a, directory=serve_dir, **k)

        def log_message(self, *a):
            pass

    socketserver.TCPServer.allow_reuse_address = True
    httpd = socketserver.TCPServer(("0.0.0.0", SERVE_PORT), Handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

    client = connect()
    try:
        card = usb_card(client)
        if card is None:
            raise SystemExit("Keine USB-Audiokarte am Bot -- Box einstecken.")

        def play(payload, name):
            with open(os.path.join(serve_dir, name), "wb") as handle:
                handle.write(payload)
            got = run(client, f"rm -f /tmp/{name}; wget -q -O /tmp/{name} "
                              f"http://{SERVE_IP}:{SERVE_PORT}/{name} 2>&1; "
                              f"wc -c < /tmp/{name}")
            if not got.strip().isdigit() or int(got.strip()) != len(payload):
                print(f"  [Uebertragung unvollstaendig: {got!r} von {len(payload)}]")
                return
            run(client, f"aplay -q -D plughw:{card},0 -c 1 -r {RATE} -f S16_LE /tmp/{name}")

        if args.say:
            play(clip(args.say), "say.snd")
            print(f'gesagt: "{args.say}"')
            return

        # Fixed answers go up once; only the state-dependent ones cost a
        # transfer at the moment they are asked.
        for intent, text in STATIC.items():
            play_bytes = clip(text)
            with open(os.path.join(serve_dir, f"{intent}.snd"), "wb") as handle:
                handle.write(play_bytes)
            got = run(client, f"rm -f /tmp/{intent}.snd; wget -q -O /tmp/{intent}.snd "
                              f"http://{SERVE_IP}:{SERVE_PORT}/{intent}.snd 2>&1; "
                              f"wc -c < /tmp/{intent}.snd")
            if not got.strip().isdigit() or int(got.strip()) != len(play_bytes):
                raise SystemExit(f"{intent}: Uebertragung unvollstaendig")
        cue = array.array("h", [0]) * (RATE // 20)
        for i in range(int(RATE * 0.08)):
            env = min(1.0, i / 300, (int(RATE * 0.08) - i) / 300)
            cue.append(int(math.sin(2 * math.pi * 1500 * i / RATE) * env * 9000))
        play(cue.tobytes(), "cue.snd")

        print(f"Karte {card}, feste Antworten liegen bereit. "
              f"Fenster {args.seconds}s, Schwelle {args.threshold}.")
        print(f'Sag "homobot" -- Abbruch mit Strg-C.')

        round_number = 0
        while args.rounds == 0 or round_number < args.rounds:
            round_number += 1
            run(client, f"aplay -q -D plughw:{card},0 -c 1 -r {RATE} "
                        f"-f S16_LE /tmp/cue.snd 2>/dev/null || true")
            time.sleep(0.4)
            run(client, f"rm -f /tmp/in.wav; arecord -D plughw:{card},0 -c 1 "
                        f"-r {RATE} -f S16_LE -d {args.seconds} /tmp/in.wav "
                        f">/dev/null 2>&1", timeout=args.seconds + 60)
            _, stdout, _ = client.exec_command("cat /tmp/in.wav", timeout=120)
            data = stdout.read()
            stdout.channel.recv_exit_status()
            if len(data) < 1000:
                print(f"  [{round_number}] Aufnahme unvollstaendig")
                continue

            confidence, heard, rms, peak = recognise(data, list(PHRASES))
            flag = " (uebersteuert)" if peak > 32000 else (" (still)" if rms < 5 else "")
            intent = PHRASES.get(heard) if confidence >= args.threshold else None
            if not intent:
                detail = f"'{heard}' {confidence:.2f}" if heard else "-"
                print(f"  [{round_number}] rms={rms:.0f}{flag}  {detail}")
                continue

            print(f"  [{round_number}] rms={rms:.0f}{flag}  "
                  f"VERSTANDEN: '{heard}' ({confidence:.2f}) -> {intent}")
            if intent in STATIC:
                run(client, f"aplay -q -D plughw:{card},0 -c 1 -r {RATE} "
                            f"-f S16_LE /tmp/{intent}.snd")
            elif intent == "time":
                play(clip(answer_time(client)), "dyn.snd")
            elif intent == "today":
                play(clip(answer_today(client)), "dyn.snd")
    except KeyboardInterrupt:
        print("\nbeendet")
    finally:
        client.close()
        httpd.shutdown()


if __name__ == "__main__":
    main()
