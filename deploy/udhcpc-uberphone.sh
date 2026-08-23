#!/bin/sh
# udhcpc hook used only by uberphone-tether.sh.

set -u

STATE_FILE=${UBERPHONE_STATE_FILE:-/tmp/uberphone/lease.state}
LOG_FILE=${UBERPHONE_LOG_FILE:-/tmp/uberphone/tether.log}
ROUTE_MODE=${UBERPHONE_ROUTE_MODE:-link}

log() {
    timestamp=$(date '+%Y-%m-%dT%H:%M:%S%z' 2>/dev/null || date)
    printf '%s DHCP %s\n' "$timestamp" "$1" >> "$LOG_FILE"
}

case "${1:-}" in
    bound|renew)
        if [ -n "${subnet:-}" ]; then
            ifconfig "$interface" "$ip" netmask "$subnet" up
        else
            ifconfig "$interface" "$ip" up
        fi
        if [ -n "${broadcast:-}" ]; then
            ifconfig "$interface" broadcast "$broadcast"
        fi
        if [ "$ROUTE_MODE" = uplink ]; then
            for gateway in ${router:-}; do
                route add default gw "$gateway" dev "$interface" metric 50 2>/dev/null ||
                    log "could not add phone default route through $gateway; WLAN route left untouched"
            done
        fi
        printf '%s|%s|%s|%s\n' "$interface" "$ip" "${router:-}" "$ROUTE_MODE" > "$STATE_FILE"
        log "$1 interface=$interface ip=$ip router=${router:-none} mode=$ROUTE_MODE dns=${dns:-none}"
        ;;
    deconfig)
        ifconfig "$interface" 0.0.0.0 down 2>/dev/null || true
        rm -f "$STATE_FILE"
        log "deconfig interface=$interface"
        ;;
    leasefail|nak)
        log "$1 interface=${interface:-unknown}"
        ;;
esac
