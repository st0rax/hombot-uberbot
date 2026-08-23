#!/bin/sh
set -eu

RC_LOCAL=/usr/etc/rc.local
RELEASE=/usr/data/frankenhomo/releases/0.1.3/hombotd
BACKUP_DIR=/usr/data/frankenhomo-backup
STAMP=`date +%Y%m%d-%H%M%S 2>/dev/null || echo unknown`
BACKUP="$BACKUP_DIR/rc.local.pre-hombotd-0.1.3-$STAMP"
STAGED=/usr/etc/rc.local.hombotd-0.1.3.new

if [ ! -x "$RELEASE" ]
then
    echo "missing executable: $RELEASE" >&2
    exit 1
fi

if [ "`grep -c '^# FRANKENHOMO_SERVER_START$' "$RC_LOCAL"`" -ne 1 ] || \
   [ "`grep -c '^# FRANKENHOMO_SERVER_END$' "$RC_LOCAL"`" -ne 1 ]
then
    echo "expected exactly one managed server block" >&2
    exit 1
fi

mkdir -p "$BACKUP_DIR"
cp -p "$RC_LOCAL" "$BACKUP"

awk '
BEGIN { replacing = 0; replaced = 0 }
$0 == "# FRANKENHOMO_SERVER_START" {
    replacing = 1
    replaced++
    print "# FRANKENHOMO_SERVER_START"
    print "# lg.srv is retained on disk for rollback but is no longer started."
    print "if [ -x /usr/data/frankenhomo/releases/0.1.3/hombotd ]"
    print "then"
    print "  HOMBOTD_PORT=6260 /usr/data/frankenhomo/releases/0.1.3/hombotd >/tmp/hombotd.log 2>&1 &"
    print "  echo $! > /tmp/hombotd.pid"
    print "fi"
    print "# FRANKENHOMO_SERVER_END"
    next
}
replacing == 1 {
    if ($0 == "# FRANKENHOMO_SERVER_END") replacing = 0
    next
}
{ print }
END { if (replaced != 1 || replacing != 0) exit 43 }
' "$RC_LOCAL" > "$STAGED"

chmod 0755 "$STAGED"
sh -n "$STAGED"
mv -f "$STAGED" "$RC_LOCAL"

OLD_PID=""
if [ -f /tmp/hombotd.pid ]
then
    OLD_PID=`cat /tmp/hombotd.pid 2>/dev/null || true`
fi
if [ -n "$OLD_PID" ]
then
    kill "$OLD_PID" 2>/dev/null || true
    sleep 1
fi

HOMBOTD_PORT=6260 "$RELEASE" >/tmp/hombotd.log 2>&1 &
NEW_PID=$!
echo "$NEW_PID" > /tmp/hombotd.pid
sleep 1
if ! kill -0 "$NEW_PID" 2>/dev/null
then
    cp -p "$BACKUP" "$RC_LOCAL"
    echo "new server failed; rc.local restored from $BACKUP" >&2
    exit 1
fi

echo "upgraded=0.1.3"
echo "pid=$NEW_PID"
echo "backup=$BACKUP"
