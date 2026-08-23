# Voice message formats

`docs/VOICE_STACK.md` establishes that LG's speech services exist on this unit
but are not started. This file records what they publish, so a subscriber can be
written before the services ever run.

Everything here was read out of `/usr/rbin/rpmain.axf` with `llvm-objdump`. The
four publishing methods survive as demangled C++ symbols, and each one writes
its arguments into the message buffer at fixed offsets before calling
`AServiceMessage::PublishMessage(char const*, unsigned short, int, int)`. Nothing
below is inferred from message names or guessed from field sizes.

## Framing

Identical to the RawSensor path already implemented in `hombotd/src/rawsensor.rs`:
a `aa 55` magic and a little-endian length, then the LGRP message -- 32-byte
header, 12-byte body header, payload. Header fields that matter: sender service
at 12, receiver service at 14, kind at 16 (`3` = publish), topic at 18, body
checksum at 24, total length at 28, message id at 32, payload length at 40.

Only the numbers differ, which is why the subscriber reuses the same primitives
rather than reimplementing them.

## The four messages

| Service | id | Topic | id | Message | id | Payload |
| --- | --- | --- | --- | --- | --- | --- |
| `/SSL` | 232 | `SSLResult` | 242 | `SSLPublishResult` | `0x1403` | 8 B |
| `/VR` | 233 | `VRResult` | 243 | `VRPublishResult` | `0x1504` | variable |
| `/Keyword` | 234 | `Keyword` | 244 | `KeywordPublishResult` | `0x1105` | 20 B |
| `/Keyword` | 234 | `Keyword` | 244 | `ClapPublishResult` | `0x1106` | 20 B |

### SSLResult -- a bearing in degrees

`CSSLServiceMessage::SSLPublishResult(int, short, short)` at `0x16ff0`:

```asm
ldr  r0, [r0, #0x1c]     ; the message payload buffer
str  r1, [r0]            ; +0  int32
strh r2, [r0, #4]        ; +4  int16
strh r3, [r0, #6]        ; +6  int16
mov  r3, #8              ; payload length = 8
ldr  r2, =0x1403         ; message id
add  r1, pc, #16         ; topic name "SSLResult"
bl   AServiceMessage::PublishMessage
```

The second field is the useful one. Its only caller, `CSSLWork::DoProcess` at
`0x28594`, computes it like this: sort the candidate bearings, walk them to find
the widest circular gap, take that gap's midpoint, subtract a signed 16-bit
mounting offset loaded from `[r7, #116]`, scale, and then normalise:

```asm
cmp  r1, #0
adds r1, r1, #360        ; loop while negative
cmp  r1, #360
sub  r1, r1, #360        ; loop while >= 360
sxth r2, r1
bl   CSSLServiceMessage::SSLPublishResult
```

So the published value is a whole-degree bearing in `0..=359`, already corrected
for how the microphone pair sits in the chassis. A value outside that range means
the field was decoded in the wrong place, which is why the parser rejects it
rather than passing it on.

The third field is a constant `0` at this call site. It is carried through as
`reserved` instead of being named.

`DoProcess` also opens a shared memory region called `/HIT_PCM_SHARED_BUFF`, so
the localiser reads its audio from shared memory rather than from ALSA directly.

### Keyword and Clap

`ClapPublishResult(int)` at `0x1985c` writes one int32 at offset 0 and declares a
20-byte payload; `KeywordPublishResult(int, int, int, int, int)` at `0x19894`
writes five int32s at 0, 4, 8, 12 and 16. Both publish on the same `Keyword`
topic and are told apart by message id alone.

The clap detector is worth calling out: it needs no acoustic model and no
language, so it is unaffected by `/usr/VRDB/` being absent from this unit.

### VRResult

`VRPublishResult(int, char*, unsigned int)` at `0x15808` writes an int32 at 0,
the length at 8, and copies the recognised text to offset 12. Payload length is
computed from the string, so this message is variable-sized.

This is the one that cannot work as shipped: the Korean models it would need are
not installed.

## Implementation

`hombotd/src/voice.rs` subscribes to all three topics, decodes the four messages
and exposes them at `/api/v1/voice`. It is off unless `HOMBOTD_VOICE=1`, for the
same reason the RawSensor subscriber is: opening a second cross-service route
into the robot's own bus is not something a daemon should start doing by itself.

Eleven tests drive the parser from synthetic frames built the way
`PublishMessage` builds them, covering the full `0..=359` bearing range, the
rejection of impossible bearings, a truncated text length, a corrupted checksum,
and RawSensor traffic passing through untouched.

Those tests prove the parser matches the disassembly. They do not prove the
disassembly was read correctly. The endpoint therefore reports
`"live_confirmed": false`, and it keeps reporting it until a real frame has been
captured -- which cannot happen until the microphone question is settled.
