#!/bin/sh
# Transient Android USB tether manager for the LG HomBot.
# Nothing is installed at boot by this script.

set -u

# ifconfig, route and udhcpc live in /sbin, which is not on PATH for a
# non-interactive ssh command. Set it here so the script behaves the same
# whether it is run from rc.local, a login shell, or over ssh.
PATH=/sbin:/usr/sbin:/bin:/usr/bin:$PATH
export PATH

RUNTIME_DIR=${UBERPHONE_RUNTIME_DIR:-/tmp/uberphone}
MODULE_DIR=${UBERPHONE_MODULE_DIR:-/usr/data/frankenhomo/modules/usb-tether}
DHCP_SCRIPT=${UBERPHONE_DHCP_SCRIPT:-/usr/data/frankenhomo/bin/udhcpc-uberphone.sh}
LOG_FILE="$RUNTIME_DIR/tether.log"
STATE_FILE="$RUNTIME_DIR/lease.state"
PID_FILE="$RUNTIME_DIR/udhcpc.pid"
ROUTE_BEFORE="$RUNTIME_DIR/route.before"

mkdir -p "$RUNTIME_DIR" || exit 1

log() {
    message=$1
    timestamp=$(date '+%Y-%m-%dT%H:%M:%S%z' 2>/dev/null || date)
    printf '%s %s\n' "$timestamp" "$message" >> "$LOG_FILE"
    printf '%s\n' "$message"
    logger -t uberphone "$message" 2>/dev/null || true
}

die() {
    log "ERROR: $1"
    exit 1
}

loaded() {
    grep -q "^$1 " /proc/modules 2>/dev/null
}

verify_module() {
    module=$1
    kernel=$(uname -r)
    [ -r "$module" ] || die "missing module: $module"
    command -v strings >/dev/null 2>&1 || die "strings is required to verify module vermagic"
    strings "$module" | grep -F "vermagic=$kernel" >/dev/null 2>&1 ||
        die "refusing module with non-matching vermagic: $module"
}

load_module() {
    name=$1
    module=$2
    loaded "$name" && return 0
    verify_module "$module"
    log "loading $name"
    insmod "$module" || die "insmod failed for $name; see dmesg"
}

driver_name() {
    iface=$1
    driver_link="/sys/class/net/$iface/device/driver"
    [ -L "$driver_link" ] || return 1
    basename "$(readlink "$driver_link")"
}

find_tether_interface() {
    for path in /sys/class/net/*; do
        [ -e "$path" ] || continue
        iface=$(basename "$path")
        driver=$(driver_name "$iface" 2>/dev/null || true)
        case "$driver" in
            rndis_host|cdc_ether) printf '%s\n' "$iface"; return 0 ;;
        esac
    done
    return 1
}

wait_for_interface() {
    attempts=0
    while [ "$attempts" -lt 15 ]; do
        iface=$(find_tether_interface 2>/dev/null || true)
        [ -n "$iface" ] && { printf '%s\n' "$iface"; return 0; }
        attempts=$((attempts + 1))
        sleep 1
    done
    return 1
}

start_tether() {
    route_mode=${1:-link}
    case "$route_mode" in
        link|uplink) ;;
        *) die "route mode must be 'link' or 'uplink'" ;;
    esac

    [ ! -s "$STATE_FILE" ] || die "USB tether already has an active lease; use status or restart"
    rm -f "$PID_FILE"
    route -n > "$ROUTE_BEFORE" 2>&1 || true
    load_module usbnet /lib/modules/$(uname -r)/kernel/drivers/usb/usbnet.ko
    load_module cdc_ether "$MODULE_DIR/cdc_ether.ko"
    load_module rndis_host "$MODULE_DIR/rndis_host.ko"

    iface=$(wait_for_interface) ||
        die "no RNDIS/CDC interface appeared; enable Android USB tethering and inspect dmesg"
    [ -x "$DHCP_SCRIPT" ] || die "missing DHCP hook: $DHCP_SCRIPT"

    log "phone interface detected: $iface"
    ifconfig "$iface" up || die "could not bring up $iface"
    export UBERPHONE_ROUTE_MODE=$route_mode
    export UBERPHONE_STATE_FILE=$STATE_FILE
    export UBERPHONE_LOG_FILE=$LOG_FILE
    # Android brings its tethering DHCP server up a moment after the RNDIS
    # interface appears, so five quick discovers routinely miss it -- measured
    # on a OnePlus 9 Pro, where -t 5 found nothing and -t 20 got a lease.
    udhcpc -i "$iface" -p "$PID_FILE" -s "$DHCP_SCRIPT" -t 20 -T 3 -b ||
        die "udhcpc failed on $iface"

    attempts=0
    while [ "$attempts" -lt 15 ]; do
        [ -s "$STATE_FILE" ] && break
        attempts=$((attempts + 1))
        sleep 1
    done
    [ -s "$STATE_FILE" ] || die "no DHCP lease received on $iface"
    log "USB tether ready in $route_mode mode"
    status_tether
}

stop_tether() {
    if [ -s "$PID_FILE" ]; then
        pid=$(sed -n '1p' "$PID_FILE")
        case "$pid" in *[!0-9]*|'') ;; *) kill "$pid" 2>/dev/null || true ;; esac
    fi

    if [ -s "$STATE_FILE" ]; then
        IFS='|' read iface ip router route_mode < "$STATE_FILE"
        if [ "${route_mode:-link}" = uplink ]; then
            for gateway in ${router:-}; do
                route del default gw "$gateway" dev "$iface" metric 50 2>/dev/null ||
                    route del default gw "$gateway" dev "$iface" 2>/dev/null || true
            done
        fi
        ifconfig "$iface" 0.0.0.0 down 2>/dev/null || true
        log "USB tether stopped on $iface"
    else
        log "USB tether is not active"
    fi
    rm -f "$PID_FILE" "$STATE_FILE"
}

status_tether() {
    iface=$(find_tether_interface 2>/dev/null || true)
    if [ -z "$iface" ]; then
        printf '%s\n' "phone_interface=absent"
        return 1
    fi
    printf 'phone_interface=%s\n' "$iface"
    printf 'driver=%s\n' "$(driver_name "$iface" 2>/dev/null || printf unknown)"
    ifconfig "$iface" 2>/dev/null || true
    route -n 2>/dev/null || true
}

case "${1:-status}" in
    start) start_tether "${2:-link}" ;;
    stop) stop_tether ;;
    restart) stop_tether; start_tether "${2:-link}" ;;
    status) status_tether ;;
    log) tail -n "${2:-80}" "$LOG_FILE" 2>/dev/null || true ;;
    *) die "usage: $0 {start [link|uplink]|stop|restart [link|uplink]|status|log [lines]}" ;;
esac
