# The dormant voice stack

LG built voice control into this robot, including sound source localisation,
and left it switched off on this board. Everything below is read off files on
the unit itself; nothing here is inferred from other models or datasheets.

## What is on the device

`/usr/rbin/rpmain.axf` -- the main application binary this unit actually runs --
contains a complete speech front end and keyword recogniser:

| Symbol group | Examples | Meaning |
| --- | --- | --- |
| Keyword spotting | `CKeywordService`, `KeywordEngineRun`, `KWS_TREE_GetKeywordN`, `DLGR_USE_MULTIKEYWORD` | multi-keyword recogniser with its own service |
| Acoustic models | `HMM`, `GMM`, `DoFastSceneRecognition_KR` | hidden Markov / Gaussian mixture classifiers |
| Feature extraction | `DynamicMFCC_Init`, `DynamicMFCC_makeFrame` | MFCC front end |
| Multi-mic processing | `LGEIT_ISQE_FE_SSET_BSS`, `FE_SSET_Voice_Activity_FP`, `FE_SSET_NoiseReduction_FP` | blind source separation, VAD, noise reduction |
| Microphone plumbing | `CheckMICWorker`, `RequestJigCheckMic`, `PublishMICStatus`, `gFE_SSE_iMic` | per-microphone status and a factory mic test |

`rpmain.axf` and `rpmain_13865.axf` are the same size (4,464,376 bytes): LG ships
one binary for both variants and selects behaviour at startup.

142 prompts sit in `/usr/SNDDATA/`, among them `SND_VOICE_MOVE_SOUND_SOURCE.snd`,
`SND_VOICE_SPEAK_COMMAND.snd`, `SND_JIG_MICTEST.snd` and
`SND_FUNCTION_VOICE_ENABLED.snd`. The robot has an audio prompt for driving
towards a sound.

## The service map

`/usr/rcfg/config_voice.xml` declares four services the running configuration
never starts, on the same broker `hombotd` already speaks to:

| Service | ID | Topic | ID |
| --- | --- | --- | --- |
| `/Sound` (`CSoundService`) | 231 | `SoundAck` | 241 |
| `/SSL` (`CSSLService`) | 232 | `SSLResult` | 242 |
| `/VR` (`CVRService`) | 233 | `VRResult` | 243 |
| `/Keyword` (`CKeywordService`) | 234 | `Keyword` | 244 |

SSL is sound source localisation. `hombotd`'s existing RawSensor subscriber
already talks to this broker as service 13 against `/DAS` (110) topic 105, so
reaching topics 242--244 is the same mechanism against different ids -- not a
new transport.

## The microphones

`/usr/rcfg/Sound.xml` is present on this unit and configures the stack:

```xml
<parameter name="VREngineType"  value="FIONA" />
<parameter name="VRLanguage"    value="KOREAN" />
<parameter name="VRSamplingRate" value="16000" />
<parameter name="VRnCommand"    value="9" />
<parameter name="RK5_SSL"       value="TRUE" />
<parameter name="ENABLE_2CH_ONLY"        value="FALSE" />
<parameter name="ENABLE_1CH_2CH_COMBINE" value="TRUE"  />
```

The two-channel parameters are the important ones: direction finding needs at
least two microphones, and the configuration is written for two. The codec on
the board is a WM8960, which has an ADC, and LG's own network script already
sets a capture level (`amixer sset Capture 46,46`).

## Why it is off

`/usr/rscript/run_hit.sh` picks the configuration from a single file:

```sh
getpartnum=`cat /usr/rcfg/Name.dat`
...
if [ $getpartnum = "EBR74755203" ]
then
    config_param="config_voice.xml"
    rpmain.axf /vision ... /Sound /SSL /VR &
```

This unit's `/usr/rcfg/Name.dat` reads **`EBR74755235`**. The voice variant is
**`EBR74755203`** -- same family, different suffix. One string decides whether
the robot boots with ears.

## What this does not prove

The software is unambiguously present. Three things are not settled:

1. **Whether the microphones are fitted.** `EBR...235` is a different board part
   number, and the likeliest reason LG gates on it is that the microphones are
   not populated. Only opening the unit or a successful capture settles this.
2. **Word recognition will not work as shipped.** `/usr/VRDB/`, the model
   directory named by `VRPath`, does not exist on this unit, and the models are
   Korean regardless. `/VR` has nothing to load.
3. **`/SSL` is the interesting one anyway.** Localisation is signal processing
   across two microphones -- no acoustic model, no language. If the hardware is
   there, a clap should be locatable even with `VRDB` missing.

## Do not simply flip Name.dat

The voice branch of `run_hit.sh` starts `/Sound /SSL /VR` but **not**
`/SmartControl /SmartData`, and it bypasses the `WIFI_ATTACHED` branch that the
current setup depends on. Changing `Name.dat` would therefore take away the
SmartControl channel `hombotd` uses today. Test the microphones first.

## Suggested order

1. `arecord -c 2 -r 16000 -f S16_LE -d 3 /tmp/mic.wav` on the running unit, then
   look at the two channels. This needs no configuration change and answers the
   only question that matters.
2. If there is signal: capture `SSLResult` (topic 242) wire format the same way
   `RawSensor` was captured, before writing any decoder.
3. Only then consider a boot configuration that keeps SmartControl.
