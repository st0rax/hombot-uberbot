"""Listen through a microphone on this PC and print what was understood.

    python listen_pc.py            # 8 s from the default input
    python listen_pc.py --list     # show input devices
    python listen_pc.py -d 1 -s 12 # pick a device, record longer

Why this exists rather than the obvious one-liner: .NET's
SpeechRecognitionEngine.SetInputToDefaultAudioDevice() produces no audio at all
on this machine -- zero AudioLevelUpdated events, no reported signal problem --
even though the intended microphone is the Windows default. Recording the audio
directly and handing the recogniser a finished wave file works reliably, so that
is the route taken here.

The spoken cue matters too: output from a tool call only becomes visible once
the call finishes, so a printed "speak now" arrives long after the window has
closed. The cue is played through the headset instead.
"""

import argparse
import os
import subprocess
import sys
import tempfile
import wave

import numpy as np
import sounddevice as sd

TARGET_RATE = 16000
TARGET_PEAK = 28000


def list_devices():
    default = sd.default.device[0]
    for index, device in enumerate(sd.query_devices()):
        if device["max_input_channels"] < 1:
            continue
        mark = "  <= Standard" if index == default else ""
        print(
            f"  [{index}] {device['name'][:52]:54} "
            f"{device['max_input_channels']}ch @{int(device['default_samplerate'])}{mark}"
        )


def speak(text):
    """Say something through the default output, so the cue is audible."""
    escaped = text.replace("'", "''")
    subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "Add-Type -AssemblyName System.Speech;"
         "$s=New-Object System.Speech.Synthesis.SpeechSynthesizer;"
         f"$s.Speak('{escaped}');$s.Dispose()"],
        capture_output=True, text=True,
    )


def resample(samples, source_rate, target_rate):
    """Plain linear resampling -- speech at these rates does not need better."""
    if source_rate == target_rate:
        return samples
    count = int(round(len(samples) * target_rate / source_rate))
    source_positions = np.linspace(0, len(samples) - 1, num=count)
    return np.interp(source_positions, np.arange(len(samples)), samples)


def record(device, seconds):
    info = sd.query_devices(device if device is not None else sd.default.device[0])
    rate = int(info["default_samplerate"])
    print(f"[{info['name']} @ {rate} Hz, {seconds} s]", flush=True)

    audio = sd.rec(int(seconds * rate), samplerate=rate, channels=1,
                   dtype="int16", device=device)
    sd.wait()

    values = audio.astype(np.float64).ravel()
    centred = values - values.mean()
    rms = float(np.sqrt((centred ** 2).mean()))
    peak = float(np.abs(centred).max())
    print(f"[rms={rms:.1f} peak={peak:.0f}]", flush=True)
    if rms < 5:
        print("[fast nur Rauschen -- Mikrofon stumm oder niemand hat gesprochen]")

    resampled = resample(centred, rate, TARGET_RATE)
    gain = min(TARGET_PEAK / max(np.abs(resampled).max(), 1), 40.0)
    out = np.clip(resampled * gain, -32768, 32767).astype(np.int16)

    path = os.path.join(tempfile.gettempdir(), "listen_pc.wav")
    with wave.open(path, "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(TARGET_RATE)
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
    confidence = lines[0] if lines else "-"
    text = lines[1].strip() if len(lines) > 1 else ""
    return confidence, text


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    parser.add_argument("-d", "--device", type=int, default=None)
    parser.add_argument("-s", "--seconds", type=int, default=8)
    parser.add_argument("--no-cue", action="store_true")
    parser.add_argument("--no-answer", action="store_true")
    args = parser.parse_args()

    if args.list:
        list_devices()
        return

    if not args.no_cue:
        speak("Sprich jetzt bitte.")
    path = record(args.device, max(2, min(args.seconds, 60)))
    confidence, text = recognise(path)
    print(f"Sicherheit: {confidence}")
    print(f"VERSTANDEN: {text or '(nichts)'}")
    if not args.no_answer:
        speak(f"Ich habe verstanden: {text}" if text else "Ich habe nichts verstanden.")


if __name__ == "__main__":
    main()
