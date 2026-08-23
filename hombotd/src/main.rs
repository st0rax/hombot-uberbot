use std::env;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WIDTH: usize = 320;
const HEIGHT: usize = 240;
const LUMA_SIZE: usize = WIDTH * HEIGHT;
const FRAME_SIZE: usize = WIDTH * HEIGHT * 2;
const CAMERA_DEVICE: &str = "/dev/camclone";
static STREAM_GENERATION: AtomicUsize = AtomicUsize::new(0);
static SMARTCONTROL_PACKET_ID: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Default)]
struct RobotStatus {
    smartcontrol: String,
    robot_state: Option<String>,
    turbo: Option<String>,
    repeat: Option<String>,
    battery_level: Option<i32>,
    mode: Option<String>,
    nickname: Option<String>,
    firmware: Option<String>,
    last_payload: Option<String>,
    last_update_unix_ms: Option<u128>,
    reconnects: u64,
    last_error: Option<String>,
}

impl RobotStatus {
    fn new() -> Self {
        Self {
            smartcontrol: "starting".to_owned(),
            ..Self::default()
        }
    }

    fn json(&self) -> String {
        let percent = self
            .battery_level
            .filter(|value| (0..=5).contains(value))
            .map(|value| value * 20);
        format!(
            concat!(
                "{{\"service\":\"hombotd\",\"version\":\"0.1.3\",",
                "\"smartcontrol\":{},\"robot_state\":{},\"turbo\":{},",
                "\"repeat\":{},\"battery_level\":{},\"battery_percent\":{},",
                "\"mode\":{},\"nickname\":{},\"firmware\":{},",
                "\"last_update_unix_ms\":{},\"reconnects\":{},",
                "\"last_error\":{},\"last_payload\":{}}}"
            ),
            json_string(Some(&self.smartcontrol)),
            json_string(self.robot_state.as_deref()),
            json_string(self.turbo.as_deref()),
            json_string(self.repeat.as_deref()),
            json_i32(self.battery_level),
            json_i32(percent),
            json_string(self.mode.as_deref()),
            json_string(self.nickname.as_deref()),
            json_string(self.firmware.as_deref()),
            self.last_update_unix_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            self.reconnects,
            json_string(self.last_error.as_deref()),
            json_string(self.last_payload.as_deref()),
        )
    }
}

fn json_i32(value: Option<i32>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".to_owned())
}

fn json_string(value: Option<&str>) -> String {
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

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn extract_json_string(payload: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\":\"");
    let start = payload.find(&marker)? + marker.len();
    let mut escaped = false;
    for (offset, character) in payload[start..].char_indices() {
        if character == '"' && !escaped {
            return Some(payload[start..start + offset].to_owned());
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    None
}

fn update_status(status: &Arc<Mutex<RobotStatus>>, payload: &str) {
    let mut current = status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(value) = extract_json_string(payload, "ROBOT_STATE") {
        current.robot_state = Some(value);
    }
    if let Some(value) = extract_json_string(payload, "TURBO") {
        current.turbo = Some(value);
    }
    if let Some(value) = extract_json_string(payload, "REPEAT") {
        current.repeat = Some(value);
    }
    if let Some(value) = extract_json_string(payload, "BATT") {
        current.battery_level = value.parse().ok();
    }
    if let Some(value) = extract_json_string(payload, "MODE") {
        current.mode = Some(value);
    }
    if let Some(value) = extract_json_string(payload, "NICKNAME") {
        current.nickname = Some(value);
    }
    if let Some(value) = extract_json_string(payload, "VERSION") {
        current.firmware = Some(value);
    }
    current.smartcontrol = "connected".to_owned();
    current.last_payload = Some(payload.to_owned());
    current.last_update_unix_ms = Some(unix_ms());
    current.last_error = None;
}

fn smartcontrol_frame(payload: &str) -> Vec<u8> {
    let payload = payload.as_bytes();
    let id = SMARTCONTROL_PACKET_ID.fetch_add(1, Ordering::Relaxed);
    let length = payload.len().min(u16::MAX as usize) as u16;
    let mut frame = Vec::with_capacity(12 + usize::from(length));
    frame.extend_from_slice(&[13, 4, id, 0]);
    frame.extend_from_slice(&0_u16.to_le_bytes());
    frame.extend_from_slice(&1_u16.to_le_bytes());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&payload[..usize::from(length)]);
    frame
}

fn drain_frames(buffer: &mut Vec<u8>) -> Vec<String> {
    let mut frames = Vec::new();
    loop {
        if buffer.len() < 12 {
            break;
        }
        if buffer[0] != 13 {
            if let Some(position) = buffer.iter().position(|byte| *byte == 13) {
                buffer.drain(..position);
            } else {
                buffer.clear();
            }
            continue;
        }
        let length = u16::from_le_bytes([buffer[8], buffer[9]]) as usize;
        if length > 16_384 {
            buffer.remove(0);
            continue;
        }
        if buffer.len() < 12 + length {
            break;
        }
        let payload = String::from_utf8_lossy(&buffer[12..12 + length]).into_owned();
        buffer.drain(..12 + length);
        frames.push(payload);
    }
    frames
}

fn receive_into(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> std::io::Result<Vec<String>> {
    let mut chunk = [0_u8; 4096];
    match stream.read(&mut chunk) {
        Ok(0) => Err(std::io::Error::new(ErrorKind::UnexpectedEof, "peer closed")),
        Ok(count) => {
            buffer.extend_from_slice(&chunk[..count]);
            Ok(drain_frames(buffer))
        }
        Err(error) => Err(error),
    }
}

fn smartcontrol_address(port: u16) -> std::io::Result<SocketAddr> {
    if let Ok(host) = env::var("HOMBOTD_SMARTCONTROL_HOST") {
        let ip = host
            .parse::<IpAddr>()
            .map_err(|error| std::io::Error::new(ErrorKind::InvalidInput, error))?;
        return Ok(SocketAddr::new(ip, port));
    }

    // This firmware exposes the internal services on all interfaces, while
    // its nominal 127/8 route can be unusable. A connected UDP socket reveals
    // the active WLAN address without transmitting a datagram.
    let probe = UdpSocket::bind(("0.0.0.0", 0))?;
    probe.connect(("10.255.255.255", 9))?;
    Ok(SocketAddr::new(probe.local_addr()?.ip(), port))
}

fn connect_smartcontrol(status: &Arc<Mutex<RobotStatus>>) -> std::io::Result<()> {
    let host = smartcontrol_address(4002)?.ip();
    let mut session =
        TcpStream::connect_timeout(&SocketAddr::new(host, 4002), Duration::from_secs(3))?;
    session.set_read_timeout(Some(Duration::from_millis(500)))?;
    session.set_write_timeout(Some(Duration::from_secs(2)))?;
    session.write_all(&smartcontrol_frame("{\"CONNECT\":\"REQUEST\"}"))?;

    let handshake_deadline = Instant::now() + Duration::from_secs(5);
    let mut session_buffer = Vec::with_capacity(1024);
    let mut enabled = false;
    while Instant::now() < handshake_deadline {
        match receive_into(&mut session, &mut session_buffer) {
            Ok(frames) => {
                if frames
                    .iter()
                    .any(|payload| payload.contains("\"CONNECT\":\"ENABLE\""))
                {
                    enabled = true;
                    break;
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => return Err(error),
        }
    }
    if !enabled {
        return Err(std::io::Error::new(
            ErrorKind::TimedOut,
            "SmartControl did not enable session",
        ));
    }
    // Port 4002 is only the one-shot admission channel. The service closes
    // its side after ENABLE, so release our descriptor before entering the
    // long-lived command loop on port 4000.
    drop(session);

    let mut command =
        TcpStream::connect_timeout(&SocketAddr::new(host, 4000), Duration::from_secs(3))?;
    command.set_read_timeout(Some(Duration::from_millis(500)))?;
    command.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut command_buffer = Vec::with_capacity(4096);
    let mut next_alive = Instant::now();
    {
        let mut current = status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        current.smartcontrol = "connected".to_owned();
        current.last_error = None;
    }

    loop {
        if Instant::now() >= next_alive {
            command.write_all(&smartcontrol_frame("{\"SESSION\":\"ALIVE\"}"))?;
            next_alive = Instant::now() + Duration::from_secs(5);
        }
        match receive_into(&mut command, &mut command_buffer) {
            Ok(frames) => {
                for payload in frames {
                    update_status(status, &payload);
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => return Err(error),
        }
    }
}

fn smartcontrol_worker(status: Arc<Mutex<RobotStatus>>) {
    loop {
        if let Err(error) = connect_smartcontrol(&status) {
            let mut current = status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            current.smartcontrol = "reconnecting".to_owned();
            current.reconnects = current.reconnects.saturating_add(1);
            current.last_error = Some(error.to_string());
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn requested_fps(path: &str) -> u32 {
    path.split('?')
        .nth(1)
        .into_iter()
        .flat_map(|query| query.split('&'))
        .find_map(|item| item.strip_prefix("fps="))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(15)
        .clamp(1, 30)
}

fn stream_camera(stream: &mut TcpStream, path: &str) -> std::io::Result<()> {
    // A newer stream supersedes an older one. This makes mode switches
    // immediate and prevents stale clients from retaining the camera.
    let generation = STREAM_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;

    let fps = requested_fps(path);
    let grayscale = path.starts_with("/stream.y8");
    let transmitted_size = if grayscale { LUMA_SIZE } else { FRAME_SIZE };
    let content_type = if grayscale {
        "application/x-hombot-y8"
    } else {
        "application/x-hombot-yuv422p"
    };
    let frame_period = Duration::from_micros(1_000_000 / u64::from(fps));
    let mut camera = File::open(CAMERA_DEVICE)?;
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nCache-Control: no-store, no-cache, must-revalidate\r\nConnection: close\r\nX-Frame-Width: {WIDTH}\r\nX-Frame-Height: {HEIGHT}\r\nX-Frame-Bytes: {transmitted_size}\r\nX-Target-Fps: {fps}\r\n\r\n"
    );
    stream.write_all(header.as_bytes())?;

    let mut frame = vec![0_u8; FRAME_SIZE];
    loop {
        if STREAM_GENERATION.load(Ordering::Acquire) != generation {
            return Ok(());
        }
        let started = Instant::now();
        camera.read_exact(&mut frame)?;
        stream.write_all(&frame[..transmitted_size])?;
        let elapsed = started.elapsed();
        if elapsed < frame_period {
            thread::sleep(frame_period - elapsed);
        }
    }
}

fn single_frame(stream: &mut TcpStream) -> std::io::Result<()> {
    STREAM_GENERATION.fetch_add(1, Ordering::AcqRel);
    let mut camera = File::open(CAMERA_DEVICE)?;
    let mut frame = vec![0_u8; FRAME_SIZE];
    camera.read_exact(&mut frame)?;
    response(stream, "200 OK", "application/x-hombot-yuv422p", &frame);
    Ok(())
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 512];
    while request.len() < 4096 {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|value| value == b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

fn handle_client(mut stream: TcpStream, status: Arc<Mutex<RobotStatus>>) {
    let _ = stream.set_nodelay(true);
    let request = match read_request(&mut stream) {
        Ok(value) => value,
        Err(_) => return,
    };
    let mut first_line = request.lines().next().unwrap_or("").split_whitespace();
    let method = first_line.next().unwrap_or("");
    let path = first_line.next().unwrap_or("/");

    if method != "GET" {
        response(
            &mut stream,
            "405 Method Not Allowed",
            "application/json",
            br#"{"error":"read-only prototype"}"#,
        );
        return;
    }

    let result = if path == "/" || path.starts_with("/index.html") {
        response(&mut stream, "200 OK", "text/html; charset=utf-8", UI);
        Ok(())
    } else if path.starts_with("/healthz") {
        let state = status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .smartcontrol
            .clone();
        let body = format!(
            "{{\"status\":\"ok\",\"service\":\"hombotd\",\"version\":\"0.1.3\",\"camera\":\"/dev/camclone\",\"smartcontrol\":{}}}",
            json_string(Some(&state))
        );
        response(&mut stream, "200 OK", "application/json", body.as_bytes());
        Ok(())
    } else if path.starts_with("/api/v1/status") {
        let body = status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .json();
        response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
        );
        Ok(())
    } else if path.starts_with("/frame.yuv") {
        single_frame(&mut stream)
    } else if path.starts_with("/stream.yuv") || path.starts_with("/stream.y8") {
        stream_camera(&mut stream, path)
    } else {
        response(
            &mut stream,
            "404 Not Found",
            "application/json",
            br#"{"error":"not found"}"#,
        );
        Ok(())
    };

    if let Err(error) = result {
        eprintln!("client error: {error}");
    }
}

fn main() -> std::io::Result<()> {
    let port = env::var("HOMBOTD_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(6261);
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    let status = Arc::new(Mutex::new(RobotStatus::new()));
    let worker_status = Arc::clone(&status);
    thread::spawn(move || smartcontrol_worker(worker_status));
    eprintln!("hombotd 0.1.3 listening on 0.0.0.0:{port}");
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let client_status = Arc::clone(&status);
                thread::spawn(move || handle_client(stream, client_status));
            }
            Err(error) => eprintln!("accept error: {error}"),
        }
    }
    Ok(())
}

const UI: &[u8] = br###"<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Frankenhomo FPV</title>
<style>
html,body{margin:0;background:#090b0e;color:#eef2f6;font:15px system-ui,sans-serif}
main{max-width:960px;margin:auto;padding:16px}.bar{display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-bottom:10px}
button{background:#273242;color:#fff;border:1px solid #5c6c82;border-radius:5px;padding:7px 12px;cursor:pointer}
button:hover{background:#34445a}#state{font-weight:700}.metric{color:#9fc4e8}
.robot{display:grid;grid-template-columns:repeat(auto-fit,minmax(135px,1fr));gap:7px;margin:0 0 10px}.tile{background:#141a22;border:1px solid #2e3b4e;border-radius:6px;padding:8px}.tile b{display:block;color:#84d1a7;font-size:17px}.tile small{color:#8d9bad}
#view{display:block;width:100%;max-width:960px;aspect-ratio:4/3;background:#000;image-rendering:auto}
#view:fullscreen{width:100vw;height:100vh;object-fit:contain;background:#000}
</style>
</head>
<body><main>
<div class="bar"><strong>Frankenhomo FPV</strong><span id="state">bereit</span><span id="fps" class="metric">0 FPS</span><span id="interval" class="metric">0 ms</span><span id="rate" class="metric">0 Mbit/s</span></div>
<div class="robot"><div class="tile"><small>Roboter</small><b id="robotState">-</b></div><div class="tile"><small>Akku (LG-Stufe)</small><b id="battery">-</b></div><div class="tile"><small>Modus</small><b id="mode">-</b></div><div class="tile"><small>SmartControl</small><b id="smartcontrol">startet</b></div><div class="tile"><small>Firmware</small><b id="firmware">-</b></div></div>
<div class="bar"><button onclick="start(10,false)">Farbe 10</button><button onclick="start(15,false)">Farbe 15</button><button onclick="start(15,true)">Grau 15</button><button onclick="start(20,true)">Grau 20</button><button onclick="start(30,true)">Grau 30</button><button onclick="stopStream()">Stopp</button><button onclick="fullscreen()">Vollbild</button></div>
<canvas id="view" width="320" height="240"></canvas>
</main><script>
const W=320,H=240,Y=W*H,COLOR_SIZE=Y*2,canvas=document.getElementById('view'),ctx=canvas.getContext('2d'),image=ctx.createImageData(W,H),pixels=image.data;
let controller=null,frames=0,bytes=0,lastPaint=0,metricStart=0;
function text(id,value){document.getElementById(id).textContent=value}
function metrics(){let now=performance.now();if(lastPaint)text('interval',Math.round(now-lastPaint)+' ms');lastPaint=now;frames++;if(now-metricStart>=1000){let seconds=(now-metricStart)/1000;text('fps',(frames/seconds).toFixed(1)+' FPS');text('rate',(bytes*8/seconds/1e6).toFixed(1)+' Mbit/s');frames=0;bytes=0;metricStart=now}}
function paintColor(a){let u=Y,v=Y+(Y>>1),o=0;for(let p=0;p<Y;p++,o+=4){let c=p>>1,y=a[p],du=a[u+c]-128,dv=a[v+c]-128;pixels[o]=y+((351*dv)>>8);pixels[o+1]=y-((86*du+179*dv)>>8);pixels[o+2]=y+((443*du)>>8);pixels[o+3]=255}ctx.putImageData(image,0,0);metrics()}
function paintGray(a){for(let p=0,o=0;p<Y;p++,o+=4){let y=a[p];pixels[o]=y;pixels[o+1]=y;pixels[o+2]=y;pixels[o+3]=255}ctx.putImageData(image,0,0);metrics()}
async function start(fps,gray){stopStream();controller=new AbortController();metricStart=performance.now();lastPaint=0;let size=gray?Y:COLOR_SIZE,endpoint=gray?'/stream.y8':'/stream.yuv';text('state','verbinde '+fps+' FPS '+(gray?'grau':'Farbe'));try{let r=await fetch(endpoint+'?fps='+fps,{signal:controller.signal,cache:'no-store'});if(!r.ok)throw Error('HTTP '+r.status);let reader=r.body.getReader(),frame=new Uint8Array(size),used=0;text('state','live '+(gray?'grau':'Farbe'));while(true){let x=await reader.read();if(x.done)break;bytes+=x.value.length;let at=0;while(at<x.value.length){let take=Math.min(size-used,x.value.length-at);frame.set(x.value.subarray(at,at+take),used);used+=take;at+=take;if(used===size){if(gray)paintGray(frame);else paintColor(frame);used=0}}}}catch(e){if(e.name!=='AbortError')text('state','Fehler: '+e.message)}}
function stopStream(){if(controller){controller.abort();controller=null}text('state','gestoppt')}
function fullscreen(){if(canvas.requestFullscreen)canvas.requestFullscreen();else if(canvas.webkitRequestFullscreen)canvas.webkitRequestFullscreen()}
async function refreshStatus(){try{let r=await fetch('/api/v1/status',{cache:'no-store'}),s=await r.json();text('robotState',s.robot_state||'-');text('battery',s.battery_percent==null?'unbekannt':s.battery_percent+' % (raw '+s.battery_level+')');text('mode',s.mode||'-');text('smartcontrol',s.smartcontrol);text('firmware',s.firmware||'-')}catch(e){text('smartcontrol','API offline')}}
canvas.addEventListener('dblclick',fullscreen);setInterval(refreshStatus,1000);refreshStatus();start(15,false);
</script></body></html>"###;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smartcontrol_frame_matches_lgsrv_wire_format() {
        let frame = smartcontrol_frame("{\"SESSION\":\"ALIVE\"}");
        assert_eq!(frame[0], 13);
        assert_eq!(frame[1], 4);
        assert_eq!(&frame[4..8], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes([frame[8], frame[9]]), 19);
        assert_eq!(&frame[10..12], &[0, 0]);
        assert_eq!(&frame[12..], b"{\"SESSION\":\"ALIVE\"}");
    }

    #[test]
    fn parser_handles_fragmented_and_coalesced_frames() {
        let first = smartcontrol_frame("{\"CONNECT\":\"ENABLE\"}");
        let second = smartcontrol_frame("{\"BATT\":\"4\"}");
        let split = first.len() - 3;
        let mut buffer = first[..split].to_vec();
        assert!(drain_frames(&mut buffer).is_empty());
        buffer.extend_from_slice(&first[split..]);
        buffer.extend_from_slice(&second);
        assert_eq!(
            drain_frames(&mut buffer),
            vec!["{\"CONNECT\":\"ENABLE\"}", "{\"BATT\":\"4\"}"]
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn status_extracts_connect_init_values() {
        let status = Arc::new(Mutex::new(RobotStatus::new()));
        update_status(
            &status,
            "{\"CONNECT_INIT\":[{\"ROBOT_STATE\":\"CHARGING\"},{\"BATT\":\"4\"},{\"MODE\":\"ZZ\"},{\"VERSION\":\"11128\"}]}",
        );
        let json = status.lock().unwrap().json();
        assert!(json.contains("\"robot_state\":\"CHARGING\""));
        assert!(json.contains("\"battery_level\":4"));
        assert!(json.contains("\"battery_percent\":80"));
        assert!(json.contains("\"firmware\":\"11128\""));
    }
}
