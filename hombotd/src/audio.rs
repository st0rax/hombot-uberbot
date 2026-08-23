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
use std::process::{Child, Command, Stdio};
// ARMv5TE has no 64-bit atomics, so the generation counter is usize like
// the camera's. It only ever needs to differ from the previous value.
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::rawsensor::json_string;

pub(crate) const CAPTURE_RATE: u32 = 16000;
const CHANNELS: u16 = 1;
const BITS: u16 = 16;
/// A wave whose data chunk claims almost four gigabytes: the honest way to say
/// "this does not end" in a format that has no such thing.
const STREAMING_SIZE: u32 = 0xFFFF_FFFF - 36;

static STREAM_GENERATION: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_STREAMS: AtomicUsize = AtomicUsize::new(0);

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
            "{{\"capture_card\":{},\"rate\":{},\"channels\":{},",
            "\"active_streams\":{},\"cards\":[{}]}}"
        ),
        chosen
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

fn requested_card(path: &str) -> Option<u32> {
    path.split_once("card=")
        .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse().ok())
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
    }
}
