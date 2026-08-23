//! Live audio from the robot, and what its sound cards are doing.
//!
//! The built-in WM8960 has no microphones fitted on this board variant, and
//! LG's own application holds its single playback substream for long stretches,
//! so everything here is aimed at a USB audio device in the robot's hub.
//!
//! Capture goes through `arecord` rather than ALSA directly: the daemon has no
//! ALSA bindings, `arecord` is already on the device, and the plug plugin it
//! provides does the resampling that the USB devices seen here need -- they run
//! at 48 kHz and will not deliver 16 kHz on their own.
//!
//! The WAV header is written here rather than by `arecord -t wav`. Into a pipe
//! `arecord` cannot seek back to correct the length field once the stream ends,
//! so the sizes it leaves behind are not trustworthy. Writing the header
//! ourselves, with a length that says "keeps going", is deterministic and is
//! what browsers expect from a streaming wave.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::rawsensor::json_string;
use crate::response;

pub(crate) const CAPTURE_RATE: u32 = 16000;
const CHANNELS: u16 = 1;
const BITS: u16 = 16;
/// A wave whose data chunk claims almost four gigabytes: the honest way to say
/// "this does not end" in a format that has no such thing.
const STREAMING_SIZE: u32 = 0xFFFF_FFFF - 36;

static STREAM_GENERATION: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_STREAMS: AtomicUsize = AtomicUsize::new(0);

// ARMv5TE has no 64-bit atomics, so the generation counter is usize like
// the camera's and capture's. It only ever needs to differ from the previous
// value.
static PLAY_GENERATION: AtomicUsize = AtomicUsize::new(0);

/// A guard against a client that never stops sending: half an hour of
/// 48 kHz stereo 16 bit, far past anything a voice answer needs.
const MAX_PLAYBACK_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub(crate) struct Card {
    pub(crate) index: u32,
    pub(crate) id: String,
    pub(crate) name: String,
}

impl Card {
    /// The built-in codec is the one whose microphone inputs are unpopulated,
    /// so a USB device is always the better capture choice when one is present.
    fn is_usb(&self) -> bool {
        self.name.to_lowercase().contains("usb") || self.id.to_lowercase().contains("usb")
    }
}

/// Parses `/proc/asound/cards`, whose entries look like
/// ` 1 [Device         ]: USB-Audio - USB2.0 Device`.
pub(crate) fn parse_cards(listing: &str) -> Vec<Card> {
    let mut cards = Vec::new();
    for line in listing.lines() {
        let trimmed = line.trim_start();
        let Some(first) = trimmed.chars().next() else {
            continue;
        };
        if !first.is_ascii_digit() {
            continue;
        }
        let Ok(index) = trimmed
            .split_whitespace()
            .next()
            .unwrap_or("")
            .parse::<u32>()
        else {
            continue;
        };
        let id = trimmed
            .split_once('[')
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(id, _)| id.trim().to_owned())
            .unwrap_or_default();
        // Everything after "]: " is the human-readable description.
        let name = trimmed
            .split_once("]:")
            .map(|(_, rest)| rest.trim().to_owned())
            .filter(|rest| !rest.is_empty())
            .unwrap_or_else(|| id.clone());
        cards.push(Card { index, id, name });
    }
    cards
}

fn read_cards() -> Vec<Card> {
    parse_cards(&fs::read_to_string("/proc/asound/cards").unwrap_or_default())
}

/// The card a capture should use: a USB device if there is one, else the first.
pub(crate) fn choose_capture(cards: &[Card]) -> Option<&Card> {
    cards
        .iter()
        .find(|card| card.is_usb())
        .or_else(|| cards.first())
}

fn substream_state(index: u32, direction: char) -> Option<String> {
    let path = format!("/proc/asound/card{index}/pcm0{direction}/sub0/status");
    fs::read_to_string(path).ok().and_then(|text| {
        text.lines()
            .next()
            .map(|line| line.trim().trim_start_matches("state:").trim().to_owned())
    })
}

pub(crate) fn audio_json() -> String {
    let cards = read_cards();
    let chosen = choose_capture(&cards).map(|card| card.index);
    let chosen_playback = choose_playback(&cards, |index| {
        substream_state(index, 'p').map(|state| state == "closed")
    });
    let entries: Vec<String> = cards
        .iter()
        .map(|card| {
            let playback = substream_state(card.index, 'p');
            format!(
                concat!(
                    "{{\"index\":{},\"id\":{},\"name\":{},\"usb\":{},",
                    "\"playback_state\":{},\"playback_free\":{}}}"
                ),
                card.index,
                json_string(Some(&card.id)),
                json_string(Some(&card.name)),
                card.is_usb(),
                json_string(playback.as_deref()),
                playback
                    .as_deref()
                    .map(|state| state == "closed")
                    .unwrap_or(false),
            )
        })
        .collect();
    format!(
        concat!(
            "{{\"capture_card\":{},\"playback_card\":{},\"rate\":{},\"channels\":{},",
            "\"active_streams\":{},\"cards\":[{}]}}"
        ),
        chosen
            .map(|index| index.to_string())
            .unwrap_or_else(|| "null".to_owned()),
        chosen_playback
            .map(|index| index.to_string())
            .unwrap_or_else(|| "null".to_owned()),
        CAPTURE_RATE,
        CHANNELS,
        ACTIVE_STREAMS.load(Ordering::Acquire),
        entries.join(","),
    )
}

/// A 44-byte canonical WAV header for a stream of unknown length.
pub(crate) fn wav_header(rate: u32, channels: u16, bits: u16) -> Vec<u8> {
    let block_align = channels * bits / 8;
    let byte_rate = rate * u32::from(block_align);
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(STREAMING_SIZE + 36).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&rate.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&STREAMING_SIZE.to_le_bytes());
    header
}

/// Makes sure `arecord` goes away when the stream does. Without this a client
/// that disconnects leaves the capture device held, and the next request fails
/// with "device busy" for reasons nothing on the robot explains.
struct Recorder(Child);

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        ACTIVE_STREAMS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Same contract as `Recorder`, for the playback side: a client that walks
/// away mid-song must not leave an `aplay` holding the device.
struct Player(Child);

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn requested_card(path: &str) -> Option<u32> {
    requested_param(path, "card")
}

fn requested_param(path: &str, key: &str) -> Option<u32> {
    let marker = format!("{key}=");
    path.split_once('?')?
        .1
        .split('&')
        .find_map(|item| item.strip_prefix(marker.as_str()))
        .and_then(|value| value.parse().ok())
}

pub(crate) fn stream_audio(stream: &mut TcpStream, path: &str) -> std::io::Result<()> {
    let cards = read_cards();
    let card = requested_card(path)
        .or_else(|| choose_capture(&cards).map(|card| card.index))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no sound card"))?;

    // Only one capture stream can exist, so a newer request supersedes the
    // older one rather than both failing on a busy device.
    let generation = STREAM_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;

    let device = format!("plughw:{card},0");
    let child = Command::new("arecord")
        .args([
            "-D",
            &device,
            "-c",
            &CHANNELS.to_string(),
            "-r",
            &CAPTURE_RATE.to_string(),
            "-f",
            "S16_LE",
            "-t",
            "raw",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    ACTIVE_STREAMS.fetch_add(1, Ordering::AcqRel);
    let mut recorder = Recorder(child);
    let mut source = recorder
        .0
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("arecord produced no output"))?;

    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\n\
         Cache-Control: no-store, no-cache, must-revalidate\r\nConnection: close\r\n\
         X-Audio-Card: {card}\r\nX-Audio-Rate: {CAPTURE_RATE}\r\n\r\n"
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&wav_header(CAPTURE_RATE, CHANNELS, BITS))?;

    let mut buffer = vec![0_u8; 4096];
    loop {
        if STREAM_GENERATION.load(Ordering::Acquire) != generation {
            return Ok(());
        }
        let read = source.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        stream.write_all(&buffer[..read])?;
    }
}

/// What a client sent to the playback endpoint, after its own header.
#[derive(Debug, PartialEq)]
struct WavFormat {
    rate: u32,
    channels: u16,
    bits: u16,
    /// Where the PCM starts. Zero when the caller chose raw mode via query
    /// parameters and there is nothing to strip.
    data_offset: usize,
}

/// Walks the RIFF chunk list instead of trusting a fixed offset: browsers and
/// tools disagree on whether an 18-byte or extended fmt chunk precedes data.
fn parse_wav_header(header: &[u8]) -> Option<WavFormat> {
    if header.len() < 12 || &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }
    let mut format = None;
    let mut position = 12;
    while position + 8 <= header.len() {
        let id = &header[position..position + 4];
        let size = u32::from_le_bytes(header[position + 4..position + 8].try_into().ok()?) as usize;
        if id == b"fmt " {
            let body = header.get(position + 8..position + 8 + size)?;
            if size < 16 {
                return None;
            }
            // PCM (1) and WAVE_FORMAT_EXTENSIBLE (0xFFFE) both carry plain
            // samples; anything else would need a decoder this daemon has no
            // business containing.
            let tag = u16::from_le_bytes(body[0..2].try_into().ok()?);
            if !matches!(tag, 1 | 0xFFFE) {
                return None;
            }
            format = Some((
                u32::from_le_bytes(body[4..8].try_into().ok()?),
                u16::from_le_bytes(body[2..4].try_into().ok()?),
                u16::from_le_bytes(body[14..16].try_into().ok()?),
            ));
        } else if id == b"data" {
            let Some((rate, channels, bits)) = format else {
                return None;
            };
            return Some(WavFormat {
                rate,
                channels,
                bits,
                data_offset: position + 8,
            });
        }
        // Chunks are word-aligned; a corrupt size must not wrap the offset.
        position = position
            .saturating_add(8)
            .saturating_add(size)
            .saturating_add(size & 1);
    }
    None
}

fn sample_format(bits: u16) -> Option<&'static str> {
    match bits {
        8 => Some("U8"),
        16 => Some("S16_LE"),
        24 => Some("S24_LE"),
        32 => Some("S32_LE"),
        _ => None,
    }
}

/// The card whose playback can actually be opened right now, USB preferred:
/// the built-in codec's substream is usually held by LG's application. An
/// unreadable status is treated as "try it anyway" -- aplay then tells the
/// truth in its stderr.
fn choose_playback(cards: &[Card], is_free: impl Fn(u32) -> Option<bool>) -> Option<u32> {
    let known_free = |card: &Card| matches!(is_free(card.index), Some(true));
    cards
        .iter()
        .find(|card| card.is_usb() && known_free(card))
        .or_else(|| cards.iter().find(|card| known_free(card)))
        .or_else(|| cards.iter().find(|card| is_free(card.index).is_none()))
        .map(|card| card.index)
}

fn content_length(head: &str) -> Option<usize> {
    head.lines()
        .find(|line| line.to_lowercase().starts_with("content-length:"))?
        .split_once(':')?
        .1
        .trim()
        .parse()
        .ok()
}

/// Streams a request body into `aplay`. The body is either a WAV file
/// (detected by magic) or raw PCM described by `?rate=&channels=&bits=`.
///
/// The body is piped through as it arrives -- a five minute answer must not
/// cost five minutes of RAM. Only one playback exists at a time; a newer
/// request supersedes the older one, exactly like the camera streams.
pub(crate) fn play_audio(
    stream: &mut TcpStream,
    path: &str,
    head: &str,
    mut pending: Vec<u8>,
) -> std::io::Result<()> {
    let generation = PLAY_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;

    let content_length = content_length(head);
    if let Some(length) = content_length {
        if length > MAX_PLAYBACK_BYTES {
            response(
                stream,
                "413 Payload Too Large",
                "application/json",
                br#"{"error":"body too large"}"#,
            );
            return Ok(());
        }
    }
    if content_length == Some(0) && pending.is_empty() {
        response(
            stream,
            "400 Bad Request",
            "application/json",
            br#"{"error":"empty body"}"#,
        );
        return Ok(());
    }

    let cards = read_cards();
    let card = match requested_card(path) {
        Some(index) => index,
        None => match choose_playback(&cards, |index| {
            substream_state(index, 'p').map(|state| state == "closed")
        }) {
            Some(index) => index,
            None => {
                response(
                    stream,
                    "503 Service Unavailable",
                    "application/json",
                    concat!(
                        r#"{"error":"no free playback substream","hint":"#,
                        r#"lg.srv holds plughw:0; attach a USB sound device or pass ?card=N"}"#
                    )
                    .as_bytes(),
                );
                return Ok(());
            }
        },
    };

    stream.set_read_timeout(Some(Duration::from_secs(10)))?;

    // Establish the sample format before spawning anything.
    let wav = loop {
        if requested_param(path, "rate").is_some() {
            break None;
        }
        if let Some(format) = parse_wav_header(&pending) {
            break Some(format);
        }
        if pending.len() >= 4096 {
            response(
                stream,
                "415 Unsupported Media Type",
                "application/json",
                br#"{"error":"body is neither WAV nor raw PCM with ?rate="}"#,
            );
            return Ok(());
        }
        if content_length.is_some_and(|total| pending.len() >= total) && !pending.is_empty() {
            response(
                stream,
                "415 Unsupported Media Type",
                "application/json",
                br#"{"error":"body is neither WAV nor raw PCM with ?rate="}"#,
            );
            return Ok(());
        }
        let mut chunk = [0_u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) => {
                let message: &[u8] = if pending.is_empty() {
                    br#"{"error":"empty body"}"#
                } else {
                    br#"{"error":"truncated body"}"#
                };
                response(stream, "400 Bad Request", "application/json", message);
                return Ok(());
            }
            Ok(count) => pending.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                response(
                    stream,
                    "400 Bad Request",
                    "application/json",
                    br#"{"error":"incomplete body"}"#,
                );
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    };

    let (rate, channels, bits, data_offset) = match wav {
        Some(format) => (
            format.rate,
            format.channels,
            format.bits,
            format.data_offset,
        ),
        None => (
            requested_param(path, "rate").unwrap_or(CAPTURE_RATE),
            requested_param(path, "channels")
                .map(|value| value as u16)
                .unwrap_or(CHANNELS),
            requested_param(path, "bits")
                .map(|value| value as u16)
                .unwrap_or(BITS),
            0,
        ),
    };
    let Some(sample_format) = sample_format(bits) else {
        response(
            stream,
            "415 Unsupported Media Type",
            "application/json",
            br#"{"error":"unsupported bit depth"}"#,
        );
        return Ok(());
    };
    if rate == 0 || channels == 0 {
        response(
            stream,
            "400 Bad Request",
            "application/json",
            br#"{"error":"degenerate format"}"#,
        );
        return Ok(());
    }

    let device = format!("plughw:{card},0");
    let child = Command::new("aplay")
        .args([
            "-D",
            &device,
            "-t",
            "raw",
            "-f",
            sample_format,
            "-r",
            &rate.to_string(),
            "-c",
            &channels.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut player = Player(child);
    let mut stdin: ChildStdin = player
        .0
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("aplay produced no stdin"))?;
    let stderr_text = Arc::new(Mutex::new(String::new()));
    if let Some(stderr) = player.0.stderr.take() {
        let sink = Arc::clone(&stderr_text);
        thread::spawn(move || {
            let mut stderr = stderr;
            let mut buffer = [0_u8; 512];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let mut text = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        if text.len() < 4096 {
                            text.push_str(&String::from_utf8_lossy(&buffer[..count]));
                        }
                    }
                }
            }
        });
    }

    // Whatever arrived together with the headers may already contain PCM.
    let mut sent = 0_usize;
    if pending.len() > data_offset {
        stdin.write_all(&pending[data_offset..])?;
        sent += pending.len() - data_offset;
    }
    let mut chunk = vec![0_u8; 8192];
    loop {
        if PLAY_GENERATION.load(Ordering::Acquire) != generation {
            // Superseded: the winner answers, this connection just ends.
            return Ok(());
        }
        let total_left = content_length.map(|total| total.saturating_sub(data_offset + sent));
        if total_left == Some(0) {
            break;
        }
        let want = total_left.unwrap_or(chunk.len()).min(chunk.len()).max(1);
        match stream.read(&mut chunk[..want]) {
            Ok(0) => break,
            Ok(count) => {
                stdin.write_all(&chunk[..count])?;
                sent += count;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if content_length.is_none() && sent > 0 {
                    break; // lengthless streams end with a quiet gap
                }
                response(
                    stream,
                    "400 Bad Request",
                    "application/json",
                    br#"{"error":"incomplete body"}"#,
                );
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        if sent > MAX_PLAYBACK_BYTES {
            break; // graceful truncation beats an unbounded stranger
        }
    }
    drop(stdin); // EOF tells aplay to drain and exit

    let status = player.0.wait()?;
    let bytes_per_second = u64::from(rate) * u64::from(channels) * u64::from(bits / 8);
    let seconds = sent as f64 / bytes_per_second.max(1) as f64;
    if status.success() {
        let body = format!(
            "{{\"status\":\"played\",\"card\":{},\"bytes\":{},\"seconds\":{:.2}}}",
            card, sent, seconds
        );
        response(stream, "200 OK", "application/json", body.as_bytes());
    } else {
        let stderr = stderr_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .chars()
            .take(400)
            .collect::<String>();
        let body = format!(
            concat!(
                "{{\"error\":\"aplay failed\",\"exit\":{},",
                "\"stderr\":{}}}"
            ),
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_owned()),
            json_string(Some(stderr.trim()))
        );
        response(
            stream,
            "503 Service Unavailable",
            "application/json",
            body.as_bytes(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = concat!(
        "0 [mostwm8960     ]: WM8960 - most-wm8960\n",
        "                      most-wm8960 (WM8960)\n",
        " 1 [Device         ]: USB-Audio - USB2.0 Device\n",
        "                      Generic USB2.0 Device at usb-nx-ehci-1.1, full speed\n",
    );

    #[test]
    fn reads_both_cards_with_their_indices() {
        let cards = parse_cards(LISTING);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].index, 0);
        assert_eq!(cards[0].id, "mostwm8960");
        assert_eq!(cards[1].index, 1);
        assert_eq!(cards[1].id, "Device");
    }

    #[test]
    fn prefers_the_usb_card_over_the_built_in_codec() {
        let cards = parse_cards(LISTING);
        assert_eq!(choose_capture(&cards).map(|c| c.index), Some(1));
    }

    #[test]
    fn falls_back_to_the_only_card_when_no_usb_device_is_present() {
        let cards = parse_cards("0 [mostwm8960     ]: WM8960 - most-wm8960\n");
        assert_eq!(choose_capture(&cards).map(|c| c.index), Some(0));
    }

    #[test]
    fn survives_a_kernel_with_no_sound_at_all() {
        assert!(parse_cards("").is_empty());
        assert!(choose_capture(&[]).is_none());
    }

    #[test]
    fn header_describes_sixteen_kilohertz_mono() {
        let header = wav_header(16000, 1, 16);
        assert_eq!(header.len(), 44);
        assert_eq!(&header[0..4], b"RIFF");
        assert_eq!(&header[8..12], b"WAVE");
        assert_eq!(&header[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(header[24..28].try_into().unwrap()),
            16000
        );
        // byte rate = rate * channels * bytes per sample
        assert_eq!(
            u32::from_le_bytes(header[28..32].try_into().unwrap()),
            32000
        );
        assert_eq!(u16::from_le_bytes(header[32..34].try_into().unwrap()), 2);
    }

    #[test]
    fn header_length_says_the_stream_does_not_end() {
        let header = wav_header(16000, 1, 16);
        let declared = u32::from_le_bytes(header[40..44].try_into().unwrap());
        assert!(declared > 0xFFFF_0000, "data chunk should not look finite");
    }

    #[test]
    fn a_card_can_be_requested_explicitly() {
        assert_eq!(requested_card("/stream.wav?card=1"), Some(1));
        assert_eq!(requested_card("/stream.wav?card=0&x=2"), Some(0));
        assert_eq!(requested_card("/stream.wav"), None);
        assert_eq!(
            requested_param("/api/v1/audio/play?rate=24000", "rate"),
            Some(24_000)
        );
        assert_eq!(requested_param("/x?playback_rate=5", "rate"), None);
    }

    #[test]
    fn wav_header_parser_reads_the_canonical_form() {
        let header = wav_header(16_000, 1, 16);
        let format = parse_wav_header(&header).expect("canonical header should parse");
        assert_eq!(format.rate, 16_000);
        assert_eq!(format.channels, 1);
        assert_eq!(format.bits, 16);
        assert_eq!(format.data_offset, 44);
    }

    #[test]
    fn wav_header_parser_tolerates_an_extended_fmt_chunk() {
        // fmt chunk of 18 bytes (cbSize = 0), as many encoders emit it.
        let mut header = Vec::new();
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&(36_u32 + 18 + 8).to_le_bytes());
        header.extend_from_slice(b"WAVE");
        header.extend_from_slice(b"fmt ");
        header.extend_from_slice(&18_u32.to_le_bytes());
        header.extend_from_slice(&1_u16.to_le_bytes()); // PCM
        header.extend_from_slice(&2_u16.to_le_bytes()); // stereo
        header.extend_from_slice(&48_000_u32.to_le_bytes());
        header.extend_from_slice(&192_000_u32.to_le_bytes());
        header.extend_from_slice(&4_u16.to_le_bytes());
        header.extend_from_slice(&16_u16.to_le_bytes());
        header.extend_from_slice(&0_u16.to_le_bytes()); // cbSize
        header.extend_from_slice(b"data");
        header.extend_from_slice(&1024_u32.to_le_bytes());

        let format = parse_wav_header(&header).expect("extended header should parse");
        assert_eq!(format.rate, 48_000);
        assert_eq!(format.channels, 2);
        assert_eq!(format.bits, 16);
        assert_eq!(format.data_offset, 12 + 8 + 18 + 8);
    }

    #[test]
    fn wav_header_parser_rejects_non_pcm_and_garbage() {
        let mut compressed = wav_header(16_000, 1, 16);
        compressed[20..22].copy_from_slice(&6_u16.to_le_bytes()); // a-law
        assert_eq!(parse_wav_header(&compressed), None);
        assert_eq!(parse_wav_header(b"not a wave at all"), None);
        assert_eq!(parse_wav_header(&wav_header(16_000, 1, 16)[..40]), None);
    }

    #[test]
    fn sample_format_covers_the_depths_aplay_knows() {
        assert_eq!(sample_format(8), Some("U8"));
        assert_eq!(sample_format(16), Some("S16_LE"));
        assert_eq!(sample_format(24), Some("S24_LE"));
        assert_eq!(sample_format(32), Some("S32_LE"));
        assert_eq!(sample_format(12), None);
    }

    #[test]
    fn playback_prefers_a_free_usb_card_over_any_other_free_card() {
        let cards = vec![
            Card {
                index: 0,
                id: "mostwm8960".into(),
                name: "WM8960 - most-wm8960".into(),
            },
            Card {
                index: 1,
                id: "Device".into(),
                name: "USB-Audio - USB2.0 Device".into(),
            },
        ];
        let free = |index| Some(index == 0); // USB card busy, codec free
        assert_eq!(choose_playback(&cards, free), Some(0));

        let usb_free = |index| Some(index == 1);
        assert_eq!(choose_playback(&cards, usb_free), Some(1));

        let all_busy = |_| Some(false);
        assert_eq!(choose_playback(&cards, all_busy), None);

        // An unreadable status is a blind spot, not a refusal: attempt card 0.
        let unknown = |_| None;
        assert_eq!(choose_playback(&cards, unknown), Some(0));
    }

    #[test]
    fn content_length_is_read_case_insensitively() {
        let head = "POST /x HTTP/1.1\r\ncontent-Length:  42 \r\nHost: bot\r\n\r\n";
        assert_eq!(content_length(head), Some(42));
        assert_eq!(content_length("POST /x HTTP/1.1\r\n\r\n"), None);
    }
}
