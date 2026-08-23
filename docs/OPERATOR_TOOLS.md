# Operator tools

Small Windows-side scripts used to drive the robot from a development machine.
None of these run on the robot; they connect to it over ssh and HTTP, the same
way `hombotd`'s own clients do. They exist because two things about this
device forced a specific shape on all of them:

* **ssh `exec_command` stdin is unusable here.** Writing to the stdin of a
  remote command fails instantly on this Dropbear build -- even four
  kilobytes -- while reading a file back over the same channel works fine, and
  an interactive shell accepts stdin normally. Every tool that has to put a
  file on the robot (a synthesised announcement, a new `hombotd` build) works
  around this by starting a short-lived HTTP server on the operator machine and
  having the robot `wget` from it, rather than piping bytes down ssh.
* **The WLAN link drops for seconds at a time.** Every ssh operation retries
  with backoff instead of failing on the first timeout.

## Setup

```powershell
python -m pip install -r tools/operator/requirements.txt
```

`paramiko` is the only hard dependency for the deployment and speech tools;
`numpy` and `sounddevice` are needed only by the scripts that record from a
microphone on the operator machine (`converse.py`, `listen_pc.py`).

## Required environment

No LAN address is hardcoded in this repository. Every tool that talks to the
robot needs:

| Variable | Meaning |
| --- | --- |
| `HOMBOT_HOST` | the robot's IP address on your network |
| `HOMBOT_LOGIN_SECRET` | the root ssh password, or create a `.hombot_secret` file next to the scripts (never commit it; `*_secret*` is gitignored) |
| `HOMBOT_SERVE_IP` | this machine's own address, as the robot can reach it -- needed by any tool that uploads audio (`say.py`, `converse.py`, `homebot.py`, `wakeword.py`) |

Optional: `HOMBOT_USER` (default `root`), `HOMBOT_SERVE_PORT` (default
`8099`), `HOMBOT_VOICE` (a Windows SAPI voice name, default `Microsoft Hedda
Desktop`).

## The scripts

| Script | What it does |
| --- | --- |
| `deploy_hombotd.py VERSION` | Uploads a new `hombotd` build, smoke-tests it on a spare port, then repoints `rc.local` at it -- verifying the file is executable afterward rather than assuming it. `--rollback-only` switches back to an already-installed version. |
| `deploy_greeting.py FILE.snd [--at-boot]` | Uploads a 16 kHz mono raw PCM clip and plays it once; `--at-boot` adds it to the `rc.local` startup block. |
| `hombot_ssh_exec.py COMMAND` | Runs one shell command on the robot and prints its output. The thinnest possible building block; every other tool here is built on the same connection pattern. |
| `say.py "text"` | Synthesises text with Windows speech, uploads it, and plays it on whichever of the robot's sound cards is not currently held by LG's own application. |
| `talk.py say/listen` | The original two-way primitive: speak a line, or record from the robot's microphone and run it through Windows dictation. |
| `converse.py` | One full exchange: the robot asks a question over its own speaker, a microphone on *this* PC listens, Windows recognises it, the robot speaks the answer. Built after discovering the robot has no working microphone on this board variant -- the ear and the voice deliberately live on different machines. |
| `homebot.py` | A closed-vocabulary wake word and command listener ("Homobot", time, status, drive intents) entirely on the robot's own USB microphone and speaker. Recognition against a dozen fixed phrases rather than open dictation is why this works at all -- free dictation on the same audio returned nonsense at 2% confidence. |
| `wakeword.py` | The single-purpose predecessor to `homebot.py`: one wake phrase, one canned answer, built to prove the loop before the full command set existed. |
| `listen_pc.py` | Records from a named input device on this PC and runs it through Windows dictation. Exists because `SpeechRecognitionEngine.SetInputToDefaultAudioDevice()` silently produces no audio at all on some machines even when the intended device is the Windows default; recording explicitly with `sounddevice` and handing the recogniser a finished file is the reliable path. |

## Known limits, so the next person does not re-derive them

* Windows SAPI dictation is not adequate for this project. On a clean,
  well-leveled recording it returned "Weiterhin" and "EIH E I wurde Rom und"
  for spoken German sentences, at confidence scores near zero. A closed phrase
  list works because the search space shrinks from hundreds of thousands of
  words to a dozen; free dictation does not. See
  [`ROADMAP.md`](ROADMAP.md) for the on-device alternatives that were
  evaluated and why none of them fit the CPU.
* The robot has exactly one USB port behind its onboard hub
  (`hub 1-1:1.0: 1 port detected`, OHCI not built). A hub without its own
  power supply cannot run the WLAN stick and a second USB device at once: the
  WLAN adapter alone reports 450 mA of its own 500 mA budget. A device that
  enumerates and then immediately disconnects (`cannot get freq at ep 0x3` /
  `USB disconnect`) is this, not a driver problem.
* Recording from the robot's USB audio device must use `plughw:N,0`, not
  `hw:N,0`. The devices tested here only run at 48 kHz; `hw:` does not
  resample and silently mislabels the output as 16 kHz, producing audio that
  plays back three times too fast.
