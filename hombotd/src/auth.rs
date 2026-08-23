//! A minimal authentication gate for the daemon's one write-capable endpoint.
//!
//! `SECURITY.md` is explicit: "Do not add unauthenticated actuator, upload,
//! shell or reboot endpoints" and "require an unpredictable local token
//! stored with mode 0600." `/api/v1/audio/play` is the first endpoint the
//! daemon has ever had that writes anything -- it plays audio through the
//! robot's speaker on request -- so it is the first thing this gate guards.
//!
//! The token lives in a file on the device, not an environment variable:
//! `rc.local` and process environments are already readable by anything
//! running as root on the box, which is exactly the trusted interface this
//! control is meant to be bound to (an operator over ssh). Reading the token
//! back out is `cat /usr/data/frankenhomo/control.token` over that same ssh
//! connection -- not a new endpoint that would just move the problem.

use std::fs;
use std::io::Read;
use std::sync::OnceLock;

const TOKEN_PATH: &str = "/usr/data/frankenhomo/control.token";
const TOKEN_HEADER: &str = "x-hombot-token";
const TOKEN_BYTES: usize = 32;

static TOKEN: OnceLock<String> = OnceLock::new();

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 32 bytes of entropy. `/dev/urandom` is always present on the robot; the
/// weak fallback exists only so `cargo run` still works on a development
/// host that has neither the file nor anything exposed to a network that
/// matters -- it is never reached on the actual device.
fn entropy(len: usize) -> Vec<u8> {
    if let Ok(mut source) = fs::File::open("/dev/urandom") {
        let mut buffer = vec![0_u8; len];
        if source.read_exact(&mut buffer).is_ok() {
            return buffer;
        }
    }
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
        ^ u128::from(std::process::id());
    let mut buffer = Vec::with_capacity(len);
    while buffer.len() < len {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        buffer.extend_from_slice(&(seed as u64).to_le_bytes());
    }
    buffer.truncate(len);
    buffer
}

fn load_or_create() -> String {
    if let Ok(existing) = fs::read_to_string(TOKEN_PATH) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    let generated = hex(&entropy(TOKEN_BYTES));
    let _ = fs::write(TOKEN_PATH, &generated);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(TOKEN_PATH, fs::Permissions::from_mode(0o600));
    }
    generated
}

fn token() -> &'static str {
    TOKEN.get_or_init(load_or_create)
}

/// Exposes the same singleton token to other modules' tests, so an
/// integration test can build a request that actually authorizes without
/// this module leaking the token to anything else in production.
#[cfg(test)]
pub(crate) fn test_token() -> &'static str {
    token()
}

/// The value of one header in a raw HTTP request head, case-insensitive on
/// the name. `head` is everything `read_head()` captured, first line
/// included, so this deliberately skips line 0 before looking for `:`.
pub(crate) fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

/// Whether `head` carries a token equal to `expected`. Fails closed: an
/// empty expected token -- token generation having failed -- never
/// authorizes anything, even a request with no token header at all.
fn matches(expected: &str, head: &str) -> bool {
    !expected.is_empty()
        && header_value(head, TOKEN_HEADER)
            .map(|presented| presented == expected)
            .unwrap_or(false)
}

/// Whether a request head carries the correct control token.
pub(crate) fn authorized(head: &str) -> bool {
    matches(token(), head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_header_regardless_of_case() {
        let head = "POST /x HTTP/1.1\r\nHost: h\r\nX-Hombot-Token: abc123\r\n\r\n";
        assert_eq!(header_value(head, "x-hombot-token"), Some("abc123"));
        assert_eq!(header_value(head, "X-HOMBOT-TOKEN"), Some("abc123"));
    }

    #[test]
    fn missing_header_is_none() {
        let head = "POST /x HTTP/1.1\r\nHost: h\r\n\r\n";
        assert_eq!(header_value(head, "x-hombot-token"), None);
    }

    #[test]
    fn matching_token_authorizes() {
        let head = "POST /x HTTP/1.1\r\nX-Hombot-Token: secret\r\n\r\n";
        assert!(matches("secret", head));
    }

    #[test]
    fn wrong_token_is_refused() {
        let head = "POST /x HTTP/1.1\r\nX-Hombot-Token: wrong\r\n\r\n";
        assert!(!matches("secret", head));
    }

    #[test]
    fn no_token_header_is_refused() {
        let head = "POST /x HTTP/1.1\r\nHost: h\r\n\r\n";
        assert!(!matches("secret", head));
    }

    #[test]
    fn an_empty_expected_token_never_authorizes_even_an_empty_header() {
        let head = "POST /x HTTP/1.1\r\nX-Hombot-Token: \r\n\r\n";
        assert!(!matches("", head));
    }

    #[test]
    fn entropy_has_the_right_length_and_is_not_all_zero() {
        let bytes = entropy(32);
        assert_eq!(bytes.len(), 32);
        assert!(bytes.iter().any(|&b| b != 0));
    }

    #[test]
    fn two_entropy_draws_differ() {
        // Not a statistical test -- just catches "returns a fixed buffer".
        assert_ne!(entropy(32), entropy(32));
    }
}
