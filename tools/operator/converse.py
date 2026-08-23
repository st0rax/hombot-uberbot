"""Talk to the robot: it listens through a PC microphone and answers out loud.

    python converse.py                 # one exchange
    python converse.py -n 5            # five in a row
    python converse.py --list          # show input devices

The two halves live in different places on purpose. The robot has no microphone
fitted on this board variant, so the ear is a microphone on this PC. The voice
is the robot's own speaker, because that is what should sound like the robot.

Three things the hardware forced, all learned the hard way:

* Audio reaches the robot over HTTP, not through the ssh channel. Writing to
  stdin of a remote command fails instantly here -- even four kilobytes -- while
  having the robot fetch a file with wget carries a quarter megabyte in one go.
* Playback picks the first device whose stream is actually free. LG's own
  application holds the built-in codec's single playback substream for long
  stretches, and handing a busy device to aplay blocks rather than failing.
* Recording goes through sounddevice rather than .NET's default audio input,
  which yields no audio at all on this machine even with the right device set as
  the Windows default.
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

import numpy as np
import sounddevice as sd

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
LEAD_SILENCE_MS = 200
TARGET_PEAK = 28000
VOICE = os.environ.get("HOMBOT_VOICE", "Microsoft Hedda Desktop")


# --------------------------------------------------------------------- robot

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
        except Exception as error:  # the link drops for seconds at a time
            last = error
            time.sleep(3)
    raise SystemExit(f"robot unreachable: {last}")


def run(client, command, timeout=180):
    _, stdout, stderr = client.exec_command(command, timeout=timeout)
    stdout.channel.recv_exit_status()
    return (stdout.read().decode("utf-8", "replace").strip()
            + stderr.read().decode("utf-8", "replace").strip())


def free_output(client):
    """First playback device whose substream nothing holds, or None."""
    for line in run(client, "aplay -l 2>/dev/null").splitlines():
        if not line.startswith("card "):
            continue
        index = int(line.split()[1].rstrip(":"))
        label = line.split("[", 1)[1].split("]", 1)[0] if "[" in line else str(index)
        status = run(
            client, f"head -1 /proc/asound/card{index}/pcm0p/sub0/status 2>/dev/null"
        ).strip()
        if status in ("", "closed"):
            return index, label
    return None


def synthesise(text, path):
    scratch = os.path.join(tempfile.gettempdir(), "converse_say.wav")
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
    gain = 30000 / peak
    pad = array.array("h", [0]) * (RATE * LEAD_SILENCE_MS // 1000)
    body = array.array("h", (max(-32768, min(32767, int(v * gain))) for v in samples))
    payload = (pad + body).tobytes()
    with open(path, "wb") as handle:
        handle.write(payload)
    return len(payload)


# --------------------------------------------------------------------- ear

def resample(samples, source_rate, target_rate):
    if source_rate == target_rate:
        return samples
    count = int(round(len(samples) * target_rate / source_rate))
    return np.interp(np.linspace(0, len(samples) - 1, num=count),
                     np.arange(len(samples)), samples)


def listen(device, seconds):
    info = sd.query_devices(device if device is not None else sd.default.device[0])
    rate = int(info["default_samplerate"])
    audio = sd.rec(int(seconds * rate), samplerate=rate, channels=1,
                   dtype="int16", device=device)
    sd.wait()

    values = audio.astype(np.float64).ravel()
    centred = values - values.mean()
    rms = float(np.sqrt((centred ** 2).mean()))
    peak = float(np.abs(centred).max())
    note = ""
    if peak > 32000:
        note = "  (übersteuert -- Quelle leiser stellen)"
    elif rms < 5:
        note = "  (fast nur Rauschen)"
    print(f"[{info['name']} · rms={rms:.0f} peak={peak:.0f}{note}]", flush=True)

    resampled = resample(centred, rate, RATE)
    gain = min(TARGET_PEAK / max(np.abs(resampled).max(), 1), 40.0)
    out = np.clip(resampled * gain, -32768, 32767).astype(np.int16)
    path = os.path.join(tempfile.gettempdir(), "converse_in.wav")
    with wave.open(path, "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(RATE)
        handle.writeframes(out.tobytes())
    return path


def recognise(path):
    result = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "Add-Type -AssemblyName System.Speech;"
         "$e=New-Object System.Speech.Recognition.SpeechRecognitionEngine("
         "[System.Globalization.CultureInfo]'de-DE');"
         "$e.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar));"
         f"$e.SetInputToWaveFile('{path}');$r=$e.Recognize();"
         "if ($r) { [string]$r.Confidence; $r.Text } else { 'NICHTS';'' }"],
        capture_output=True, text=True, encoding="utf-8",
    )
    lines = (result.stdout or "").strip().splitlines()
    return (lines[0] if lines else "-",
            lines[1].strip() if len(lines) > 1 else "")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    parser.add_argument("-d", "--device", type=int, default=None)
    parser.add_argument("-s", "--seconds", type=int, default=6)
    parser.add_argument("-n", "--rounds", type=int, default=1)
    args = parser.parse_args()

    if args.list:
        default = sd.default.device[0]
        for index, device in enumerate(sd.query_devices()):
            if device["max_input_channels"] > 0:
                mark = "  <= Standard" if index == default else ""
                print(f"  [{index}] {device['name'][:52]:54} {mark}")
        return

    serve_dir = tempfile.mkdtemp(prefix="converse_")

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
        output = free_output(client)
        if output is None:
            print("Kein freies Wiedergabegeraet am Bot -- er kann gerade nicht antworten.")
        else:
            print(f"Bot spricht ueber card {output[0]} ({output[1]})")

        def say(text):
            if output is None:
                print(f'  [stumm] "{text}"')
                return
            size = synthesise(text, os.path.join(serve_dir, "say.snd"))
            got = run(client, f"rm -f /tmp/say.snd; wget -q -O /tmp/say.snd "
                              f"http://{SERVE_IP}:{SERVE_PORT}/say.snd 2>&1; "
                              f"wc -c < /tmp/say.snd")
            if not got.strip().isdigit() or int(got.strip()) != size:
                print(f"  [Uebertragung unvollstaendig: {got!r} von {size}]")
                return
            run(client, f"aplay -q -D plughw:{output[0]},0 -c 1 -r {RATE} "
                        f"-f S16_LE /tmp/say.snd")
            print(f'  [Bot] "{text}"', flush=True)

        for round_number in range(1, args.rounds + 1):
            if args.rounds > 1:
                print(f"\n--- Runde {round_number}/{args.rounds} ---")
            say("Sag jetzt bitte etwas.")
            path = listen(args.device, max(2, min(args.seconds, 60)))
            confidence, text = recognise(path)
            print(f"Sicherheit: {confidence}")
            print(f"VERSTANDEN: {text or '(nichts)'}")
            say(f"Ich habe verstanden: {text}" if text
                else "Ich habe leider nichts verstanden.")
    finally:
        client.close()
        httpd.shutdown()


if __name__ == "__main__":
    main()
