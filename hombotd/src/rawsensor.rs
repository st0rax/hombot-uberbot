use std::env;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const TCP_MAGIC: [u8; 2] = [0xaa, 0x55];
pub(crate) const FRAMEWORK_HEADER_SIZE: usize = 32;
const RAW_SENSOR_BODY_HEADER_SIZE: usize = 12;
const RAW_SENSOR_SIZE: usize = 158;
const RAW_SENSOR_FRAMEWORK_SIZE: usize =
    FRAMEWORK_HEADER_SIZE + RAW_SENSOR_BODY_HEADER_SIZE + RAW_SENSOR_SIZE;
pub(crate) const MAX_FRAME_SIZE: usize = 1_050_000;
pub(crate) const SUBSCRIBER_SERVICE: u16 = 13; // /collector_pc
const DAS_SERVICE: u16 = 110;
const RAW_SENSOR_TOPIC: u16 = 105;
const RAW_SENSOR_MESSAGE_ID: u16 = 0x0304;
pub(crate) const FRAMEWORK_SUBSCRIBE: u16 = 1;
pub(crate) const FRAMEWORK_UNSUBSCRIBE: u16 = 2;
pub(crate) const FRAMEWORK_PUBLISH: u16 = 3;
const SAMPLE_STALE_AFTER: Duration = Duration::from_secs(2);
// A subscribe can be accepted and then never deliver anything: the broker keeps
// the socket open, so reads only ever time out and the session would otherwise
// spin here forever without re-subscribing. Both deadlines are measured against
// decoded samples, not raw bytes, because unrelated topics also arrive on this
// socket. They are generous next to SAMPLE_STALE_AFTER so a healthy but briefly
// quiet route is not torn down.
pub(crate) const FIRST_SAMPLE_DEADLINE: Duration = Duration::from_secs(10);
pub(crate) const IDLE_DEADLINE: Duration = Duration::from_secs(5);

/// Decides whether a subscribed-but-quiet session has to be torn down.
/// `since_sample` is `None` until the first sample is decoded.
pub(crate) fn quiet_too_long(
    since_subscribe: Duration,
    since_sample: Option<Duration>,
) -> Option<(&'static str, Duration)> {
    let (waited, deadline, what) = match since_sample {
        Some(waited) => (waited, IDLE_DEADLINE, "no sample"),
        None => (since_subscribe, FIRST_SAMPLE_DEADLINE, "no first sample"),
    };
    (waited >= deadline).then_some((what, waited))
}

#[derive(Clone, Debug, PartialEq)]
struct RawSensorSample {
    legacy_level: i8,
    voltage_raw_centivolts: u16,
    charger_state_raw: u8,
    battery_aux_raw: u16,
    record_hex: String,
}

#[derive(Clone)]
pub(crate) struct RawSensorStatus {
    state: String,
    sample: Option<RawSensorSample>,
    last_update: Option<Instant>,
    last_update_unix_ms: Option<u128>,
    reconnects: u64,
    last_error: Option<String>,
}

impl RawSensorStatus {
    pub(crate) fn new() -> Self {
        Self {
            state: "starting".to_owned(),
            sample: None,
            last_update: None,
            last_update_unix_ms: None,
            reconnects: 0,
            last_error: None,
        }
    }

    /// Status for a build where the broker subscriber is not rolled out. The
    /// endpoint still answers, and reports plainly that nothing is sampling.
    pub(crate) fn disabled() -> Self {
        let mut status = Self::new();
        status.state = "disabled".to_owned();
        status
    }

    pub(crate) fn json(&self) -> String {
        self.json_at(Instant::now())
    }

    /// Snapshot for the Stage-2 interlock gate. Does not name bumper/cliff
    /// wire fields: those channels stay `Unknown` until firmware and capture
    /// XOR agree. Battery bytes are passed through as raw values only.
    pub(crate) fn interlock_input(&self, now: Instant) -> crate::interlock::Input {
        let age_ms = self
            .last_update
            .map(|updated| now.saturating_duration_since(updated).as_millis());
        let available = age_ms
            .map(|age| age <= SAMPLE_STALE_AFTER.as_millis())
            .unwrap_or(false);
        let (charger, volts) = match (available, self.sample.as_ref()) {
            (true, Some(sample)) => (
                Some(sample.charger_state_raw),
                Some(sample.voltage_raw_centivolts),
            ),
            _ => (None, None),
        };
        crate::interlock::Input::from_live_sample(available, charger, volts)
    }

    fn json_at(&self, now: Instant) -> String {
        let age_ms = self
            .last_update
            .map(|updated| now.saturating_duration_since(updated).as_millis());
        let available = age_ms
            .map(|age| age <= SAMPLE_STALE_AFTER.as_millis())
            .unwrap_or(false);
        let battery = if available {
            self.sample.as_ref().map(|sample| {
                format!(
                    concat!(
                        "{{\"voltage_v\":{:.2},\"voltage_raw_centivolts\":{},",
                        "\"resolution_v\":0.01,\"estimated_accuracy_v\":0.10,",
                        "\"calibration\":\"pending_multimeter_pair\",",
                        "\"legacy_level_raw\":{},\"charger_state_raw\":{},",
                        "\"aux_raw\":{},\"mapping_confidence\":0.98,",
                        "\"physical_accuracy_confidence\":0.75}}"
                    ),
                    f64::from(sample.voltage_raw_centivolts) / 100.0,
                    sample.voltage_raw_centivolts,
                    sample.legacy_level,
                    sample.charger_state_raw,
                    sample.battery_aux_raw,
                )
            })
        } else {
            None
        };
        let raw_record_hex = if available {
            self.sample
                .as_ref()
                .map(|sample| json_string(Some(&sample.record_hex)))
                .unwrap_or_else(|| "null".to_owned())
        } else {
            "null".to_owned()
        };
        format!(
            concat!(
                "{{\"available\":{},\"source\":\"broker_rawsensor\",",
                "\"state\":{},\"age_ms\":{},\"last_update_unix_ms\":{},",
                "\"raw_record_size\":158,\"raw_record_hex\":{},\"battery\":{},",
                "\"reconnects\":{},\"last_error\":{}}}"
            ),
            available,
            json_string(Some(&self.state)),
            age_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            self.last_update_unix_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            raw_record_hex,
            battery.unwrap_or_else(|| "null".to_owned()),
            self.reconnects,
            json_string(self.last_error.as_deref()),
        )
    }
}

pub(crate) fn json_string(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "null".to_owned();
    };
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            value if value.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", value as u32));
            }
            value => escaped.push(value),
        }
    }
    escaped.push('"');
    escaped
}

pub(crate) fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn checksum(payload: &[u8]) -> u32 {
    payload
        .iter()
        .fold(0_u32, |sum, byte| sum.wrapping_add(u32::from(*byte)))
}

pub(crate) fn put_u16(buffer: &mut [u8], offset: usize, value: u16) {
    buffer[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn get_u16(buffer: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buffer[offset], buffer[offset + 1]])
}

pub(crate) fn get_u32(buffer: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buffer[offset],
        buffer[offset + 1],
        buffer[offset + 2],
        buffer[offset + 3],
    ])
}

fn subscription_message(kind: u16) -> Vec<u8> {
    debug_assert!(matches!(kind, FRAMEWORK_SUBSCRIBE | FRAMEWORK_UNSUBSCRIBE));
    let mut message = vec![0_u8; 52];
    message[0] = 1;
    put_u16(&mut message, 12, SUBSCRIBER_SERVICE);
    // RawSensor lacks an XML publisher. Keeping receiver=/DAS makes the
    // broker's fallback route the subscription from broker 3 to broker 2.
    put_u16(&mut message, 14, DAS_SERVICE);
    put_u16(&mut message, 16, kind);
    put_u32(&mut message, 20, u32::MAX);
    put_u16(&mut message, 32, RAW_SENSOR_TOPIC);
    put_u16(&mut message, 34, SUBSCRIBER_SERVICE);
    let body_checksum = checksum(&message[FRAMEWORK_HEADER_SIZE..]);
    put_u32(&mut message, 24, body_checksum);
    let message_len = message.len() as u32;
    put_u32(&mut message, 28, message_len);
    message
}

pub(crate) fn tcp_frame(message: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(message.len() + 6);
    frame.extend_from_slice(&TCP_MAGIC);
    frame.extend_from_slice(&(message.len() as u32).to_le_bytes());
    frame.extend_from_slice(message);
    frame
}

pub(crate) fn drain_tcp_frames(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut messages = Vec::new();
    loop {
        if buffer.len() < 6 {
            break;
        }
        let Some(start) = buffer.windows(2).position(|window| window == TCP_MAGIC) else {
            let keep = usize::from(buffer.last() == Some(&TCP_MAGIC[0]));
            buffer.drain(..buffer.len() - keep);
            break;
        };
        if start > 0 {
            buffer.drain(..start);
        }
        if buffer.len() < 6 {
            break;
        }
        let length = get_u32(buffer, 2) as usize;
        if !(FRAMEWORK_HEADER_SIZE..=MAX_FRAME_SIZE).contains(&length) {
            buffer.remove(0);
            continue;
        }
        if buffer.len() < 6 + length {
            break;
        }
        messages.push(buffer[6..6 + length].to_vec());
        buffer.drain(..6 + length);
    }
    messages
}

fn parse_raw_sensor(message: &[u8]) -> Result<Option<RawSensorSample>, &'static str> {
    if message.len() < FRAMEWORK_HEADER_SIZE {
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
    if get_u16(message, 16) != FRAMEWORK_PUBLISH || get_u16(message, 18) != RAW_SENSOR_TOPIC {
        return Ok(None);
    }
    if message.len() != RAW_SENSOR_FRAMEWORK_SIZE {
        return Err("RawSensor framework size mismatch");
    }
    if get_u16(message, 12) != DAS_SERVICE || get_u16(message, 14) != SUBSCRIBER_SERVICE {
        return Err("RawSensor route identity mismatch");
    }
    if get_u16(message, 32) != RAW_SENSOR_MESSAGE_ID {
        return Err("RawSensor message id mismatch");
    }
    if get_u32(message, 40) as usize != RAW_SENSOR_SIZE {
        return Err("RawSensor payload size mismatch");
    }
    let raw = &message[44..];
    Ok(Some(RawSensorSample {
        legacy_level: raw[4] as i8,
        voltage_raw_centivolts: get_u16(raw, 5),
        battery_aux_raw: get_u16(raw, 7),
        charger_state_raw: raw[9],
        record_hex: hex(raw),
    }))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[usize::from(byte >> 4)] as char);
        output.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    output
}

pub(crate) fn broker_address() -> std::io::Result<SocketAddr> {
    for variable in ["HOMBOTD_RAWSENSOR_HOST", "HOMBOTD_SMARTCONTROL_HOST"] {
        if let Ok(host) = env::var(variable) {
            let ip = host
                .parse::<IpAddr>()
                .map_err(|error| std::io::Error::new(ErrorKind::InvalidInput, error))?;
            return Ok(SocketAddr::new(ip, 9000));
        }
    }
    let probe = UdpSocket::bind(("0.0.0.0", 0))?;
    probe.connect(("10.255.255.255", 9))?;
    Ok(SocketAddr::new(probe.local_addr()?.ip(), 9000))
}

fn update_sample(status: &Arc<Mutex<RawSensorStatus>>, sample: RawSensorSample) {
    let mut current = status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    current.state = "connected".to_owned();
    current.sample = Some(sample);
    current.last_update = Some(Instant::now());
    current.last_update_unix_ms = Some(unix_ms());
    current.last_error = None;
}

fn record_parse_error(status: &Arc<Mutex<RawSensorStatus>>, error: &str) {
    let mut current = status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    current.last_error = Some(error.to_owned());
}

fn run_session(status: &Arc<Mutex<RawSensorStatus>>) -> std::io::Result<()> {
    let mut stream = TcpStream::connect_timeout(&broker_address()?, Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.set_nodelay(true)?;
    // A prior process can have died before its best-effort UNSUBSCRIBE. Clear
    // the fixed service/topic tuple before adding it again so reconnects are
    // idempotent and cannot accumulate duplicate broker routes.
    stream.write_all(&tcp_frame(&subscription_message(FRAMEWORK_UNSUBSCRIBE)))?;
    stream.write_all(&tcp_frame(&subscription_message(FRAMEWORK_SUBSCRIBE)))?;
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
        let mut last_sample: Option<Instant> = None;
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
                        match parse_raw_sensor(&message) {
                            Ok(Some(sample)) => {
                                update_sample(status, sample);
                                last_sample = Some(Instant::now());
                            }
                            Ok(None) => {}
                            Err(error) => record_parse_error(status, error),
                        }
                    }
                }
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    if let Some((what, waited)) =
                        quiet_too_long(subscribed_at.elapsed(), last_sample.map(|at| at.elapsed()))
                    {
                        return Err(std::io::Error::new(
                            ErrorKind::TimedOut,
                            format!("{what} for {waited:?}; resubscribing"),
                        ));
                    }
                }
                Err(error) => return Err(error),
            }
        }
    })();

    let _ = stream.write_all(&tcp_frame(&subscription_message(FRAMEWORK_UNSUBSCRIBE)));
    result
}

pub(crate) fn worker(status: Arc<Mutex<RawSensorStatus>>) {
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

    #[test]
    fn quiet_session_waits_longer_for_the_first_sample() {
        // Subscribed, nothing decoded yet: hold until FIRST_SAMPLE_DEADLINE.
        assert_eq!(quiet_too_long(Duration::from_secs(9), None), None);
        assert_eq!(
            quiet_too_long(Duration::from_secs(10), None),
            Some(("no first sample", Duration::from_secs(10)))
        );
    }

    #[test]
    fn quiet_session_gives_up_after_an_idle_gap() {
        // Once samples have been seen, a shorter gap is enough to resubscribe,
        // and the long first-sample budget no longer applies.
        let seen = Some(Duration::from_secs(4));
        assert_eq!(quiet_too_long(Duration::from_secs(600), seen), None);
        assert_eq!(
            quiet_too_long(Duration::from_secs(600), Some(Duration::from_secs(5))),
            Some(("no sample", Duration::from_secs(5)))
        );
    }

    #[test]
    fn a_live_session_is_never_torn_down() {
        assert_eq!(
            quiet_too_long(
                Duration::from_secs(86_400),
                Some(Duration::from_millis(400))
            ),
            None
        );
    }

    #[test]
    fn disabled_status_reports_itself_as_unavailable() {
        let json = RawSensorStatus::disabled().json();
        assert!(json.contains("\"state\":\"disabled\""), "{json}");
        assert!(json.contains("\"available\":false"), "{json}");
        assert!(json.contains("\"battery\":null"), "{json}");
    }

    fn raw_fixture() -> Vec<u8> {
        let mut raw = vec![0_u8; RAW_SENSOR_SIZE];
        raw[4] = 0xec;
        raw[5..7].copy_from_slice(&885_u16.to_le_bytes());
        raw[7..9].copy_from_slice(&0x1234_u16.to_le_bytes());
        raw[9] = 1;
        raw
    }

    fn publication(raw: &[u8]) -> Vec<u8> {
        let mut message = vec![0_u8; RAW_SENSOR_FRAMEWORK_SIZE];
        message[0] = 1;
        put_u16(&mut message, 12, DAS_SERVICE);
        put_u16(&mut message, 14, SUBSCRIBER_SERVICE);
        put_u16(&mut message, 16, FRAMEWORK_PUBLISH);
        put_u16(&mut message, 18, RAW_SENSOR_TOPIC);
        put_u16(&mut message, 32, RAW_SENSOR_MESSAGE_ID);
        put_u32(&mut message, 40, RAW_SENSOR_SIZE as u32);
        message[44..].copy_from_slice(raw);
        let sum = checksum(&message[FRAMEWORK_HEADER_SIZE..]);
        put_u32(&mut message, 24, sum);
        put_u32(&mut message, 28, RAW_SENSOR_FRAMEWORK_SIZE as u32);
        message
    }

    #[test]
    fn subscribe_frame_matches_static_firmware_proof() {
        let frame = tcp_frame(&subscription_message(FRAMEWORK_SUBSCRIBE));
        assert_eq!(
            hex(&frame),
            "aa55340000000100000000000000000000000d006e0001000000ffffffff760000003400000069000d0000000000000000000000000000000000"
        );
    }

    #[test]
    fn raw_publication_preserves_hundredths_of_a_volt() {
        let sample = parse_raw_sensor(&publication(&raw_fixture()))
            .unwrap()
            .unwrap();
        assert_eq!(sample.voltage_raw_centivolts, 885);
        assert_eq!(sample.legacy_level, -20);
        assert_eq!(sample.charger_state_raw, 1);
        assert_eq!(sample.battery_aux_raw, 0x1234);
        assert_eq!(sample.record_hex.len(), RAW_SENSOR_SIZE * 2);
    }

    #[test]
    fn raw_publication_rejects_corruption_and_wrong_route() {
        let mut corrupt = publication(&raw_fixture());
        corrupt[44] ^= 1;
        assert_eq!(parse_raw_sensor(&corrupt), Err("LGRP checksum mismatch"));

        let mut wrong_route = publication(&raw_fixture());
        put_u16(&mut wrong_route, 14, 30);
        assert_eq!(
            parse_raw_sensor(&wrong_route),
            Err("RawSensor route identity mismatch")
        );
    }

    #[test]
    fn tcp_parser_handles_fragmentation_junk_and_coalescing() {
        let first = tcp_frame(&publication(&raw_fixture()));
        let second = first.clone();
        let split = first.len() - 5;
        let mut buffer = b"junk".to_vec();
        buffer.extend_from_slice(&first[..split]);
        assert!(drain_tcp_frames(&mut buffer).is_empty());
        buffer.extend_from_slice(&first[split..]);
        buffer.extend_from_slice(&second);
        let messages = drain_tcp_frames(&mut buffer);
        assert_eq!(messages.len(), 2);
        assert!(buffer.is_empty());
    }

    #[test]
    fn api_hides_stale_sample_and_reports_confidence() {
        let now = Instant::now();
        let sample = parse_raw_sensor(&publication(&raw_fixture()))
            .unwrap()
            .unwrap();
        let mut status = RawSensorStatus::new();
        status.state = "connected".to_owned();
        status.sample = Some(sample);
        status.last_update = Some(now);
        status.last_update_unix_ms = Some(1234);

        let fresh = status.json_at(now + Duration::from_millis(40));
        assert!(fresh.contains("\"available\":true"));
        assert!(fresh.contains("\"voltage_v\":8.85"));
        assert!(fresh.contains("\"voltage_raw_centivolts\":885"));
        assert!(fresh.contains("\"mapping_confidence\":0.98"));
        assert!(fresh.contains("\"raw_record_hex\":\"00000000ec7503"));

        let stale = status.json_at(now + Duration::from_millis(2001));
        assert!(stale.contains("\"available\":false"));
        assert!(stale.contains("\"battery\":null"));
        assert!(stale.contains("\"raw_record_hex\":null"));
        assert!(stale.contains("\"age_ms\":2001"));
    }
}
