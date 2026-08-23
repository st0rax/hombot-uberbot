//! Subscriber for LG's factory voice services.
//!
//! The robot's own application carries a complete speech stack -- keyword
//! spotting, a clap detector, Korean command recognition, and sound source
//! localisation -- and publishes each result on the same broker this daemon
//! already reads RawSensor from. Only the service and topic numbers differ.
//!
//! Every layout below was read out of `/usr/rbin/rpmain.axf`, from the four
//! `*PublishResult` methods, not guessed:
//!
//! | method | topic | id | payload |
//! | --- | --- | --- | --- |
//! | `CSSLServiceMessage::SSLPublishResult(int, short, short)` | `SSLResult` (242) | `0x1403` | 8 B: i32, i16, i16 |
//! | `CKeywordServiceMessage::KeywordPublishResult(int × 5)` | `Keyword` (244) | `0x1105` | 20 B: 5 × i32 |
//! | `CKeywordServiceMessage::ClapPublishResult(int)` | `Keyword` (244) | `0x1106` | 20 B: i32 |
//! | `CVRServiceMessage::VRPublishResult(int, char*, unsigned)` | `VRResult` (243) | `0x1504` | i32, u32 length, then text |
//!
//! The angle is the interesting one. `CSSLWork::DoProcess` sorts its candidate
//! bearings, takes the midpoint of the widest circular gap, subtracts a stored
//! per-unit mounting offset, and then normalises with an explicit `+360` /
//! `-360` loop before narrowing to 16 bits -- so the published value is a
//! bearing in whole degrees, 0..=359.
//!
//! IMPORTANT: these layouts are decoded from disassembly and exercised here
//! against synthetic frames. No live frame has been observed yet, because the
//! services only start when `/usr/rcfg/Name.dat` names the voice variant. Treat
//! a first live capture as the real confirmation.

use std::io::{ErrorKind, Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::rawsensor::{
    FRAMEWORK_HEADER_SIZE, FRAMEWORK_PUBLISH, FRAMEWORK_SUBSCRIBE, FRAMEWORK_UNSUBSCRIBE,
    SUBSCRIBER_SERVICE, broker_address, checksum, drain_tcp_frames, get_u16, get_u32, json_string,
    put_u16, put_u32, quiet_too_long, tcp_frame, unix_ms,
};

const BODY_HEADER_SIZE: usize = 12;

const SSL_SERVICE: u16 = 232;
const VR_SERVICE: u16 = 233;
const KEYWORD_SERVICE: u16 = 234;

const SSL_TOPIC: u16 = 242;
const VR_TOPIC: u16 = 243;
const KEYWORD_TOPIC: u16 = 244;

const SSL_RESULT_ID: u16 = 0x1403;
const KEYWORD_RESULT_ID: u16 = 0x1105;
const CLAP_RESULT_ID: u16 = 0x1106;
const VR_RESULT_ID: u16 = 0x1504;

const SSL_PAYLOAD_SIZE: usize = 8;
const KEYWORD_PAYLOAD_SIZE: usize = 20;
const VR_TEXT_OFFSET: usize = 12;
const MAX_VR_TEXT: usize = 512;

const EVENT_STALE_AFTER: Duration = Duration::from_secs(10);

/// Every topic this subscriber asks the broker for, with the service that
/// publishes it. The broker routes by the receiver field, so each topic needs
/// its own subscription message.
const SUBSCRIPTIONS: [(u16, u16); 3] = [
    (SSL_TOPIC, SSL_SERVICE),
    (VR_TOPIC, VR_SERVICE),
    (KEYWORD_TOPIC, KEYWORD_SERVICE),
];

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum VoiceEvent {
    /// A localised sound. `bearing_degrees` is measured in the robot's own
    /// frame after its mounting offset has been applied.
    SoundSource {
        bearing_degrees: u16,
        handle: i32,
        reserved: i16,
    },
    /// The clap detector fired. LG publishes a single integer with it.
    Clap { value: i32 },
    /// A spotted keyword. The five integers are published positionally; only
    /// the first is confidently the keyword index, so the rest are kept raw
    /// rather than given invented names.
    Keyword { index: i32, fields: [i32; 4] },
    /// A recognised command from the Korean engine, with its text.
    Recognition { command: i32, text: String },
}

impl VoiceEvent {
    fn kind(&self) -> &'static str {
        match self {
            VoiceEvent::SoundSource { .. } => "sound_source",
            VoiceEvent::Clap { .. } => "clap",
            VoiceEvent::Keyword { .. } => "keyword",
            VoiceEvent::Recognition { .. } => "recognition",
        }
    }

    fn detail_json(&self) -> String {
        match self {
            VoiceEvent::SoundSource {
                bearing_degrees,
                handle,
                reserved,
            } => format!(
                "\"bearing_degrees\":{bearing_degrees},\"handle\":{handle},\"reserved\":{reserved}"
            ),
            VoiceEvent::Clap { value } => format!("\"value\":{value}"),
            VoiceEvent::Keyword { index, fields } => format!(
                "\"index\":{index},\"fields\":[{},{},{},{}]",
                fields[0], fields[1], fields[2], fields[3]
            ),
            VoiceEvent::Recognition { command, text } => {
                format!("\"command\":{command},\"text\":{}", json_string(Some(text)))
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct VoiceStatus {
    state: String,
    last_event: Option<VoiceEvent>,
    last_update: Option<Instant>,
    last_update_unix_ms: Option<u128>,
    /// Kept separately so a bearing survives a later clap or keyword event --
    /// "drive towards the last sound" needs the bearing, not the newest thing.
    last_bearing: Option<(u16, u128)>,
    events_seen: u64,
    reconnects: u64,
    last_error: Option<String>,
}

impl VoiceStatus {
    pub(crate) fn new() -> Self {
        Self {
            state: "starting".to_owned(),
            last_event: None,
            last_update: None,
            last_update_unix_ms: None,
            last_bearing: None,
            events_seen: 0,
            reconnects: 0,
            last_error: None,
        }
    }

    /// Status for a daemon started without the voice subscriber switched on.
    pub(crate) fn disabled() -> Self {
        let mut status = Self::new();
        status.state = "disabled".to_owned();
        status
    }

    pub(crate) fn json(&self) -> String {
        self.json_at(Instant::now())
    }

    fn json_at(&self, now: Instant) -> String {
        let age_ms = self
            .last_update
            .map(|at| now.saturating_duration_since(at).as_millis());
        let fresh = age_ms
            .map(|age| age <= EVENT_STALE_AFTER.as_millis())
            .unwrap_or(false);
        let event = match (fresh, self.last_event.as_ref()) {
            (true, Some(event)) => format!(
                "{{\"kind\":{},{}}}",
                json_string(Some(event.kind())),
                event.detail_json()
            ),
            _ => "null".to_owned(),
        };
        let bearing = match self.last_bearing {
            Some((degrees, at_unix_ms)) => {
                format!("{{\"degrees\":{degrees},\"at_unix_ms\":{at_unix_ms}}}")
            }
            None => "null".to_owned(),
        };
        format!(
            concat!(
                "{{\"available\":{},\"source\":\"broker_voice\",\"state\":{},",
                "\"age_ms\":{},\"last_update_unix_ms\":{},\"event\":{},",
                "\"last_bearing\":{},\"events_seen\":{},\"reconnects\":{},",
                "\"last_error\":{},\"layout_source\":\"decoded_from_rpmain_disassembly\",",
                "\"live_confirmed\":false}}"
            ),
            fresh,
            json_string(Some(&self.state)),
            age_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            self.last_update_unix_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            event,
            bearing,
            self.events_seen,
            self.reconnects,
            json_string(self.last_error.as_deref()),
        )
    }
}

/// Builds the subscribe/unsubscribe message for one topic.
fn subscription_message(kind: u16, topic: u16, publisher: u16) -> Vec<u8> {
    debug_assert!(matches!(kind, FRAMEWORK_SUBSCRIBE | FRAMEWORK_UNSUBSCRIBE));
    let mut message = vec![0_u8; 52];
    message[0] = 1;
    put_u16(&mut message, 12, SUBSCRIBER_SERVICE);
    put_u16(&mut message, 14, publisher);
    put_u16(&mut message, 16, kind);
    put_u32(&mut message, 20, u32::MAX);
    put_u16(&mut message, 32, topic);
    put_u16(&mut message, 34, SUBSCRIBER_SERVICE);
    let body_checksum = checksum(&message[FRAMEWORK_HEADER_SIZE..]);
    put_u32(&mut message, 24, body_checksum);
    let len = message.len() as u32;
    put_u32(&mut message, 28, len);
    message
}

fn get_i32(buffer: &[u8], offset: usize) -> i32 {
    get_u32(buffer, offset) as i32
}

fn get_i16(buffer: &[u8], offset: usize) -> i16 {
    get_u16(buffer, offset) as i16
}

/// Decodes one published LGRP message into a voice event.
///
/// `Ok(None)` means a well-formed message this subscriber does not care about,
/// which is normal: several topics share the connection.
pub(crate) fn parse_voice(message: &[u8]) -> Result<Option<VoiceEvent>, &'static str> {
    if message.len() < FRAMEWORK_HEADER_SIZE + BODY_HEADER_SIZE {
        return Err("truncated LGRP header");
    }
    if message[0] != 1 {
        return Err("unsupported LGRP version");
    }
    if get_u32(message, 28) as usize != message.len() {
        return Err("LGRP size mismatch");
    }
    if checksum(&message[FRAMEWORK_HEADER_SIZE..]) != get_u32(message, 24) {
        return Err("LGRP checksum mismatch");
    }
    if get_u16(message, 16) != FRAMEWORK_PUBLISH {
        return Ok(None);
    }

    let topic = get_u16(message, 18);
    if !SUBSCRIPTIONS.iter().any(|(known, _)| *known == topic) {
        return Ok(None);
    }
    if get_u16(message, 14) != SUBSCRIBER_SERVICE {
        return Err("voice route identity mismatch");
    }

    let message_id = get_u16(message, 32);
    let declared = get_u32(message, 40) as usize;
    let payload = &message[FRAMEWORK_HEADER_SIZE + BODY_HEADER_SIZE..];
    if declared != payload.len() {
        return Err("voice payload size mismatch");
    }

    match (topic, message_id) {
        (SSL_TOPIC, SSL_RESULT_ID) => {
            if payload.len() != SSL_PAYLOAD_SIZE {
                return Err("SSLResult payload size mismatch");
            }
            let bearing = get_i16(payload, 4);
            // DoProcess normalises into 0..360 before narrowing to 16 bits, so
            // anything outside that range means we decoded the wrong field.
            if !(0..360).contains(&i32::from(bearing)) {
                return Err("SSLResult bearing out of range");
            }
            Ok(Some(VoiceEvent::SoundSource {
                bearing_degrees: bearing as u16,
                handle: get_i32(payload, 0),
                reserved: get_i16(payload, 6),
            }))
        }
        (KEYWORD_TOPIC, CLAP_RESULT_ID) => {
            if payload.len() != KEYWORD_PAYLOAD_SIZE {
                return Err("ClapResult payload size mismatch");
            }
            Ok(Some(VoiceEvent::Clap {
                value: get_i32(payload, 0),
            }))
        }
        (KEYWORD_TOPIC, KEYWORD_RESULT_ID) => {
            if payload.len() != KEYWORD_PAYLOAD_SIZE {
                return Err("KeywordResult payload size mismatch");
            }
            Ok(Some(VoiceEvent::Keyword {
                index: get_i32(payload, 0),
                fields: [
                    get_i32(payload, 4),
                    get_i32(payload, 8),
                    get_i32(payload, 12),
                    get_i32(payload, 16),
                ],
            }))
        }
        (VR_TOPIC, VR_RESULT_ID) => {
            if payload.len() < VR_TEXT_OFFSET {
                return Err("VRResult payload too short");
            }
            let length = get_u32(payload, 8) as usize;
            let available = payload.len() - VR_TEXT_OFFSET;
            if length > available || length > MAX_VR_TEXT {
                return Err("VRResult text length out of range");
            }
            let raw = &payload[VR_TEXT_OFFSET..VR_TEXT_OFFSET + length];
            let raw = raw.split(|byte| *byte == 0).next().unwrap_or(raw);
            Ok(Some(VoiceEvent::Recognition {
                command: get_i32(payload, 0),
                text: String::from_utf8_lossy(raw).into_owned(),
            }))
        }
        _ => Ok(None),
    }
}

fn record_event(status: &Arc<Mutex<VoiceStatus>>, event: VoiceEvent) {
    let mut current = status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let stamp = unix_ms();
    if let VoiceEvent::SoundSource {
        bearing_degrees, ..
    } = &event
    {
        current.last_bearing = Some((*bearing_degrees, stamp));
    }
    current.last_event = Some(event);
    current.last_update = Some(Instant::now());
    current.last_update_unix_ms = Some(stamp);
    current.events_seen = current.events_seen.saturating_add(1);
    current.last_error = None;
}

fn record_error(status: &Arc<Mutex<VoiceStatus>>, error: &str) {
    let mut current = status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    current.last_error = Some(error.to_owned());
}

fn run_session(status: &Arc<Mutex<VoiceStatus>>) -> std::io::Result<()> {
    let mut stream =
        std::net::TcpStream::connect_timeout(&broker_address()?, Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.set_nodelay(true)?;

    for (topic, publisher) in SUBSCRIPTIONS {
        // Clear first: a previous process can have died before its own
        // UNSUBSCRIBE, and duplicate routes would otherwise accumulate.
        stream.write_all(&tcp_frame(&subscription_message(
            FRAMEWORK_UNSUBSCRIBE,
            topic,
            publisher,
        )))?;
        stream.write_all(&tcp_frame(&subscription_message(
            FRAMEWORK_SUBSCRIBE,
            topic,
            publisher,
        )))?;
    }
    {
        let mut current = status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        current.state = "subscribed".to_owned();
        current.last_error = None;
    }

    let result = (|| {
        let mut buffer = Vec::with_capacity(4096);
        let mut chunk = [0_u8; 4096];
        let subscribed_at = Instant::now();
        let mut last_event: Option<Instant> = None;
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "broker closed",
                    ));
                }
                Ok(count) => {
                    buffer.extend_from_slice(&chunk[..count]);
                    for message in drain_tcp_frames(&mut buffer) {
                        match parse_voice(&message) {
                            Ok(Some(event)) => {
                                record_event(status, event);
                                last_event = Some(Instant::now());
                            }
                            Ok(None) => {}
                            Err(error) => record_error(status, error),
                        }
                    }
                }
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    // Voice events are sporadic by nature -- nobody speaks for
                    // minutes at a time -- so unlike RawSensor a quiet session
                    // is normal. Only a session that never produced anything at
                    // all is torn down and resubscribed.
                    if last_event.is_none()
                        && quiet_too_long(subscribed_at.elapsed(), None).is_some()
                    {
                        return Err(std::io::Error::new(
                            ErrorKind::TimedOut,
                            format!(
                                "no voice event for {:?}; resubscribing",
                                subscribed_at.elapsed()
                            ),
                        ));
                    }
                }
                Err(error) => return Err(error),
            }
        }
    })();

    for (topic, publisher) in SUBSCRIPTIONS {
        let _ = stream.write_all(&tcp_frame(&subscription_message(
            FRAMEWORK_UNSUBSCRIBE,
            topic,
            publisher,
        )));
    }
    result
}

pub(crate) fn worker(status: Arc<Mutex<VoiceStatus>>) {
    loop {
        if let Err(error) = run_session(&status) {
            let mut current = status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            current.state = "reconnecting".to_owned();
            current.reconnects = current.reconnects.saturating_add(1);
            current.last_error = Some(error.to_string());
        }
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a published frame exactly the way `AServiceMessage::PublishMessage`
    /// lays one out: 32-byte header, 12-byte body header, then the payload.
    fn publication(publisher: u16, topic: u16, message_id: u16, payload: &[u8]) -> Vec<u8> {
        let mut message = vec![0_u8; FRAMEWORK_HEADER_SIZE + BODY_HEADER_SIZE + payload.len()];
        message[0] = 1;
        put_u16(&mut message, 12, publisher);
        put_u16(&mut message, 14, SUBSCRIBER_SERVICE);
        put_u16(&mut message, 16, FRAMEWORK_PUBLISH);
        put_u16(&mut message, 18, topic);
        put_u16(&mut message, 32, message_id);
        put_u32(&mut message, 40, payload.len() as u32);
        message[FRAMEWORK_HEADER_SIZE + BODY_HEADER_SIZE..].copy_from_slice(payload);
        let sum = checksum(&message[FRAMEWORK_HEADER_SIZE..]);
        put_u32(&mut message, 24, sum);
        let len = message.len() as u32;
        put_u32(&mut message, 28, len);
        message
    }

    fn ssl_payload(handle: i32, bearing: i16, reserved: i16) -> Vec<u8> {
        let mut payload = vec![0_u8; SSL_PAYLOAD_SIZE];
        payload[0..4].copy_from_slice(&handle.to_le_bytes());
        payload[4..6].copy_from_slice(&bearing.to_le_bytes());
        payload[6..8].copy_from_slice(&reserved.to_le_bytes());
        payload
    }

    #[test]
    fn decodes_a_bearing_from_the_localiser() {
        let frame = publication(
            SSL_SERVICE,
            SSL_TOPIC,
            SSL_RESULT_ID,
            &ssl_payload(7, 127, 0),
        );
        assert_eq!(
            parse_voice(&frame),
            Ok(Some(VoiceEvent::SoundSource {
                bearing_degrees: 127,
                handle: 7,
                reserved: 0,
            }))
        );
    }

    #[test]
    fn accepts_the_whole_normalised_range() {
        for bearing in [0_i16, 1, 180, 359] {
            let frame = publication(
                SSL_SERVICE,
                SSL_TOPIC,
                SSL_RESULT_ID,
                &ssl_payload(0, bearing, 0),
            );
            match parse_voice(&frame) {
                Ok(Some(VoiceEvent::SoundSource {
                    bearing_degrees, ..
                })) => assert_eq!(i16::try_from(bearing_degrees).unwrap(), bearing),
                other => panic!("{bearing} rejected: {other:?}"),
            }
        }
    }

    #[test]
    fn rejects_a_bearing_the_firmware_could_not_have_sent() {
        // DoProcess normalises before publishing, so 360 or a negative value
        // means the field is not where we think it is.
        for bearing in [-1_i16, 360, 1000] {
            let frame = publication(
                SSL_SERVICE,
                SSL_TOPIC,
                SSL_RESULT_ID,
                &ssl_payload(0, bearing, 0),
            );
            assert_eq!(parse_voice(&frame), Err("SSLResult bearing out of range"));
        }
    }

    #[test]
    fn decodes_a_clap() {
        let frame = publication(KEYWORD_SERVICE, KEYWORD_TOPIC, CLAP_RESULT_ID, &{
            let mut payload = vec![0_u8; KEYWORD_PAYLOAD_SIZE];
            payload[0..4].copy_from_slice(&3_i32.to_le_bytes());
            payload
        });
        assert_eq!(parse_voice(&frame), Ok(Some(VoiceEvent::Clap { value: 3 })));
    }

    #[test]
    fn decodes_a_keyword_with_its_four_extra_fields() {
        let mut payload = vec![0_u8; KEYWORD_PAYLOAD_SIZE];
        for (slot, value) in [11_i32, 22, 33, 44, 55].iter().enumerate() {
            payload[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let frame = publication(KEYWORD_SERVICE, KEYWORD_TOPIC, KEYWORD_RESULT_ID, &payload);
        assert_eq!(
            parse_voice(&frame),
            Ok(Some(VoiceEvent::Keyword {
                index: 11,
                fields: [22, 33, 44, 55],
            }))
        );
    }

    #[test]
    fn decodes_recognised_text() {
        let text = b"CLEAN_START";
        let mut payload = vec![0_u8; VR_TEXT_OFFSET + text.len()];
        payload[0..4].copy_from_slice(&5_i32.to_le_bytes());
        payload[8..12].copy_from_slice(&(text.len() as u32).to_le_bytes());
        payload[VR_TEXT_OFFSET..].copy_from_slice(text);
        let frame = publication(VR_SERVICE, VR_TOPIC, VR_RESULT_ID, &payload);
        assert_eq!(
            parse_voice(&frame),
            Ok(Some(VoiceEvent::Recognition {
                command: 5,
                text: "CLEAN_START".to_owned(),
            }))
        );
    }

    #[test]
    fn refuses_a_text_length_that_runs_past_the_payload() {
        let mut payload = vec![0_u8; VR_TEXT_OFFSET + 4];
        payload[8..12].copy_from_slice(&9999_u32.to_le_bytes());
        let frame = publication(VR_SERVICE, VR_TOPIC, VR_RESULT_ID, &payload);
        assert_eq!(
            parse_voice(&frame),
            Err("VRResult text length out of range")
        );
    }

    #[test]
    fn ignores_topics_this_subscriber_did_not_ask_for() {
        // RawSensor shares the connection; it must pass through untouched.
        let frame = publication(110, 105, 0x0304, &vec![0_u8; 158]);
        assert_eq!(parse_voice(&frame), Ok(None));
    }

    #[test]
    fn catches_a_corrupted_frame() {
        let mut frame = publication(
            SSL_SERVICE,
            SSL_TOPIC,
            SSL_RESULT_ID,
            &ssl_payload(1, 90, 0),
        );
        let last = frame.len() - 1;
        frame[last] ^= 0xff;
        assert_eq!(parse_voice(&frame), Err("LGRP checksum mismatch"));
    }

    #[test]
    fn a_bearing_survives_a_later_clap() {
        let status = Arc::new(Mutex::new(VoiceStatus::new()));
        record_event(
            &status,
            VoiceEvent::SoundSource {
                bearing_degrees: 42,
                handle: 0,
                reserved: 0,
            },
        );
        record_event(&status, VoiceEvent::Clap { value: 1 });
        let json = status.lock().unwrap().json();
        assert!(json.contains("\"kind\":\"clap\""), "{json}");
        assert!(json.contains("\"degrees\":42"), "{json}");
    }

    #[test]
    fn disabled_status_says_so_without_inventing_an_event() {
        let json = VoiceStatus::disabled().json();
        assert!(json.contains("\"state\":\"disabled\""), "{json}");
        assert!(json.contains("\"event\":null"), "{json}");
        assert!(json.contains("\"live_confirmed\":false"), "{json}");
    }

    #[test]
    fn subscribes_to_each_topic_with_its_own_publisher() {
        for (topic, publisher) in SUBSCRIPTIONS {
            let message = subscription_message(FRAMEWORK_SUBSCRIBE, topic, publisher);
            assert_eq!(get_u16(&message, 14), publisher);
            assert_eq!(get_u16(&message, 32), topic);
            assert_eq!(get_u16(&message, 16), FRAMEWORK_SUBSCRIBE);
            assert_eq!(
                get_u32(&message, 24),
                checksum(&message[FRAMEWORK_HEADER_SIZE..])
            );
        }
    }
}
