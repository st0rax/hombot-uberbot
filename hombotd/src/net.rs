//! What the robot's network interfaces are actually doing.
//!
//! This exists because the dashboard had no way to tell whether the UBERPHONE
//! relay was attached, and a static "not paired" label stops being honest the
//! moment the phone is plugged in.
//!
//! Everything is read from /proc and /sys. The IPv4 address deliberately is
//! not: getting it needs an ioctl, and a link that is up with a driver bound
//! and bytes flowing is the part that answers "is the phone there".

use std::fs;
use std::path::{Path, PathBuf};

use crate::rawsensor::json_string;

/// Drivers that mean "an Android phone is sharing its connection".
const TETHER_DRIVERS: [&str; 2] = ["rndis_host", "cdc_ether"];

#[derive(Debug, PartialEq)]
pub(crate) struct Interface {
    pub(crate) name: String,
    pub(crate) operstate: Option<String>,
    pub(crate) carrier: Option<bool>,
    pub(crate) driver: Option<String>,
    pub(crate) rx_bytes: Option<u64>,
    pub(crate) tx_bytes: Option<u64>,
    pub(crate) default_route: bool,
}

impl Interface {
    fn is_phone_link(&self) -> bool {
        self.driver
            .as_deref()
            .map(|driver| TETHER_DRIVERS.contains(&driver))
            .unwrap_or(false)
    }

    fn json(&self) -> String {
        format!(
            concat!(
                "{{\"name\":{},\"operstate\":{},\"carrier\":{},\"driver\":{},",
                "\"rx_bytes\":{},\"tx_bytes\":{},\"default_route\":{},",
                "\"phone_link\":{}}}"
            ),
            json_string(Some(&self.name)),
            json_string(self.operstate.as_deref()),
            self.carrier
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            json_string(self.driver.as_deref()),
            self.rx_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            self.tx_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_owned()),
            self.default_route,
            self.is_phone_link(),
        )
    }
}

fn read_trimmed(path: PathBuf) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Interfaces that carry a default route, from /proc/net/route.
///
/// The file is whitespace separated with a header line; a default route is the
/// entry whose destination is all zeroes.
pub(crate) fn default_route_interfaces(routes: &str) -> Vec<String> {
    routes
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let destination = fields.next()?;
            (destination.trim_matches('0').is_empty() && !destination.is_empty())
                .then(|| name.to_owned())
        })
        .collect()
}

/// Byte counters keyed by interface, from /proc/net/dev.
///
/// Lines look like `  usb0: 2324 6 0 0 ... 4414 12 ...`, and the name can be
/// flush against the colon, so the split has to be on ':' rather than on
/// whitespace alone.
pub(crate) fn interface_counters(dev: &str) -> Vec<(String, u64, u64)> {
    dev.lines()
        .skip(2)
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let values: Vec<&str> = rest.split_whitespace().collect();
            let rx = values.first()?.parse().ok()?;
            // Receive block is 8 columns wide, so transmit bytes are column 9.
            let tx = values.get(8)?.parse().ok()?;
            Some((name.trim().to_owned(), rx, tx))
        })
        .collect()
}

fn collect(root: &Path) -> Vec<Interface> {
    let routes = fs::read_to_string(root.join("proc/net/route")).unwrap_or_default();
    let defaults = default_route_interfaces(&routes);
    let dev = fs::read_to_string(root.join("proc/net/dev")).unwrap_or_default();
    let counters = interface_counters(&dev);

    let mut interfaces = Vec::new();
    let Ok(entries) = fs::read_dir(root.join("sys/class/net")) else {
        return interfaces;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "lo" {
            continue;
        }
        let base = entry.path();
        let carrier = read_trimmed(base.join("carrier")).and_then(|value| match value.as_str() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        });
        let driver = fs::read_link(base.join("device/driver"))
            .ok()
            .and_then(|target| {
                target
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            });
        let counter = counters.iter().find(|(other, _, _)| *other == name);
        interfaces.push(Interface {
            operstate: read_trimmed(base.join("operstate")),
            carrier,
            driver,
            rx_bytes: counter.map(|(_, rx, _)| *rx),
            tx_bytes: counter.map(|(_, _, tx)| *tx),
            default_route: defaults.contains(&name),
            name,
        });
    }
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    interfaces
}

pub(crate) fn network_json() -> String {
    network_json_at(Path::new("/"))
}

fn network_json_at(root: &Path) -> String {
    let interfaces = collect(root);
    let phone = interfaces
        .iter()
        .find(|interface| interface.is_phone_link());
    let phone_json = match phone {
        Some(interface) => format!(
            "{{\"attached\":true,\"interface\":{},\"driver\":{},\"up\":{},\"routing\":{}}}",
            json_string(Some(&interface.name)),
            json_string(interface.driver.as_deref()),
            interface.operstate.as_deref() == Some("up") || interface.carrier == Some(true),
            json_string(Some(if interface.default_route {
                "uplink"
            } else {
                "link_only"
            })),
        ),
        None => {
            "{\"attached\":false,\"interface\":null,\"driver\":null,\"up\":false,\"routing\":null}"
                .to_owned()
        }
    };
    let list: Vec<String> = interfaces.iter().map(Interface::json).collect();
    format!(
        "{{\"uberphone\":{},\"interfaces\":[{}]}}",
        phone_json,
        list.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ROUTE: &str = concat!(
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n",
        "ra0\t00179B0A\t00000000\t0001\t0\t0\t0\tFFFFFF00\n",
        "usb0\t00A7CA0A\t00000000\t0001\t0\t0\t0\tFFFFFF00\n",
        "ra0\t00000000\tEB179B0A\t0003\t0\t0\t0\t00000000\n",
    );

    const DEV: &str = concat!(
        "Inter-|   Receive                        |  Transmit\n",
        " face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets\n",
        "    lo:       0       0    0    0    0     0          0         0       0       0\n",
        "  usb0:    2324       6    0    0    0     0          0         0    4414      12\n",
    );

    #[test]
    fn finds_only_the_default_route() {
        assert_eq!(default_route_interfaces(ROUTE), vec!["ra0".to_owned()]);
    }

    #[test]
    fn reads_both_byte_counters_past_the_receive_block() {
        let counters = interface_counters(DEV);
        assert_eq!(counters.len(), 2);
        assert_eq!(counters[1], ("usb0".to_owned(), 2324, 4414));
    }

    #[test]
    fn survives_a_kernel_that_offers_neither_file() {
        assert_eq!(default_route_interfaces(""), Vec::<String>::new());
        assert_eq!(interface_counters(""), Vec::new());
    }

    fn fixture(name: &str, driver: Option<&str>, operstate: &str, carrier: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("hombotd-net-{name}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("proc/net")).unwrap();
        fs::write(root.join("proc/net/route"), ROUTE).unwrap();
        fs::write(root.join("proc/net/dev"), DEV).unwrap();
        for iface in ["ra0", "usb0"] {
            let base = root.join("sys/class/net").join(iface);
            fs::create_dir_all(&base).unwrap();
            fs::write(
                base.join("operstate"),
                if iface == "usb0" { operstate } else { "up" },
            )
            .unwrap();
            fs::write(
                base.join("carrier"),
                if iface == "usb0" { carrier } else { "1" },
            )
            .unwrap();
        }
        if let Some(driver) = driver {
            // The real thing is a symlink into the driver's bus directory; the
            // code only ever looks at the last path component.
            let target = root.join("bus").join(driver);
            fs::create_dir_all(&target).unwrap();
            let link = root.join("sys/class/net/usb0/device");
            fs::create_dir_all(&link).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, link.join("driver")).unwrap();
            #[cfg(windows)]
            let _ = std::os::windows::fs::symlink_dir(&target, link.join("driver"));
        }
        root
    }

    #[test]
    fn reports_no_phone_when_nothing_is_plugged_in() {
        let root = fixture("none", None, "down", "0");
        let json = network_json_at(&root);
        assert!(json.contains("\"attached\":false"), "{json}");
        assert!(json.contains("\"name\":\"usb0\""), "{json}");
        assert!(
            !json.contains("\"name\":\"lo\""),
            "loopback should be skipped: {json}"
        );
    }

    #[test]
    fn a_tethered_phone_that_does_not_carry_traffic_reads_as_link_only() {
        let root = fixture("link", Some("rndis_host"), "up", "1");
        let json = network_json_at(&root);
        if !json.contains("\"driver\":\"rndis_host\"") {
            // Symlink creation needs privileges on Windows; skip rather than
            // fail on a machine that cannot represent the fixture.
            return;
        }
        assert!(json.contains("\"attached\":true"), "{json}");
        assert!(json.contains("\"routing\":\"link_only\""), "{json}");
        assert!(json.contains("\"rx_bytes\":2324"), "{json}");
        assert!(json.contains("\"tx_bytes\":4414"), "{json}");
    }
}
