#!/bin/sh
set -eu

RC_LOCAL=${HOMBOTD_RC_LOCAL:-/usr/etc/rc.local}
BACKUP_DIR=${HOMBOTD_BACKUP_DIR:-/usr/data/frankenhomo-backup}
ORIGINAL=${HOMBOTD_RC_BACKUP:-$BACKUP_DIR/rc.local.before-hombotd}
STAMP=`date +%Y%m%d-%H%M%S 2>/dev/null || echo unknown`
CURRENT_BACKUP="$BACKUP_DIR/rc.local.pre-rollback-$STAMP"
STAGED="$RC_LOCAL.rollback.new"

if [ ! -f "$ORIGINAL" ]
then
    echo "rollback source missing: $ORIGINAL" >&2
    exit 1
fi

mkdir -p "$BACKUP_DIR"
cp -p "$RC_LOCAL" "$CURRENT_BACKUP"
cp -p "$ORIGINAL" "$STAGED"
chmod 0755 "$STAGED"
if ! sh -n "$STAGED"
then
    rm -f "$STAGED"
    echo "rollback source failed syntax check; current startup unchanged" >&2
    exit 1
fi

mv -f "$STAGED" "$RC_LOCAL"

if [ -f /tmp/hombotd.pid ]
then
    PID=`cat /tmp/hombotd.pid 2>/dev/null || true`
    if [ -n "$PID" ]
    then
        kill "$PID" 2>/dev/null || true
    fi
fi

if [ -x /usr/bin/lg.srv ]
then
    /usr/bin/lg.srv
fi

echo "rollback_complete=yes"
echo "restored=$ORIGINAL"
echo "replaced_file_saved=$CURRENT_BACKUP"
