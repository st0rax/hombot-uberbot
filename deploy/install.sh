#!/bin/sh
set -eu

VERSION=${HOMBOTD_VERSION:-0.1.3}
RC_LOCAL=${HOMBOTD_RC_LOCAL:-/usr/etc/rc.local}
RELEASE_ROOT=${HOMBOTD_RELEASE_ROOT:-/usr/data/frankenhomo/releases}
RELEASE="$RELEASE_ROOT/$VERSION/hombotd"
BACKUP_DIR=${HOMBOTD_BACKUP_DIR:-/usr/data/frankenhomo-backup}
ORIGINAL="$BACKUP_DIR/rc.local.before-hombotd"
STAMP=`date +%Y%m%d-%H%M%S 2>/dev/null || echo unknown`
AUDIT_BACKUP="$BACKUP_DIR/rc.local.pre-hombotd-$STAMP"
STAGED="$RC_LOCAL.hombotd.new"

if [ ! -x "$RELEASE" ]
then
    echo "missing executable: $RELEASE" >&2
    exit 1
fi

if grep -q '^# FRANKENHOMO_SERVER_START$' "$RC_LOCAL"
then
    echo "managed block already installed; use the versioned upgrade script" >&2
    exit 1
fi

if ! grep -q '^if \[ -x /usr/bin/lg\.srv \]$' "$RC_LOCAL"
then
    echo "expected lg.srv startup block not found; original unchanged" >&2
    exit 1
fi

mkdir -p "$BACKUP_DIR"
cp -p "$RC_LOCAL" "$AUDIT_BACKUP"
if [ ! -f "$ORIGINAL" ]
then
    cp -p "$RC_LOCAL" "$ORIGINAL"
fi

awk -v release="$RELEASE" '
BEGIN { replacing = 0; replaced = 0 }
$0 == "if [ -x /usr/bin/lg.srv ]" {
    if (replaced != 0) exit 42
    replacing = 1
    replaced = 1
    print "# FRANKENHOMO_SERVER_START"
    print "# lg.srv is retained on disk for rollback but is no longer started."
    print "if [ -x " release " ]"
    print "then"
    print "  HOMBOTD_PORT=6260 " release " >/tmp/hombotd.log 2>&1 &"
    print "  echo $! > /tmp/hombotd.pid"
    print "fi"
    print "# FRANKENHOMO_SERVER_END"
    next
}
replacing == 1 {
    if ($0 == "fi") replacing = 0
    next
}
{ print }
END {
    if (replaced != 1 || replacing != 0) exit 43
}
' "$RC_LOCAL" > "$STAGED"

chmod 0755 "$STAGED"
if ! sh -n "$STAGED"
then
    rm -f "$STAGED"
    echo "syntax check failed; original unchanged" >&2
    exit 1
fi

if [ "`grep -c '^# FRANKENHOMO_SERVER_START$' "$STAGED"`" -ne 1 ] || \
   [ "`grep -c '^# FRANKENHOMO_SERVER_END$' "$STAGED"`" -ne 1 ]
then
    rm -f "$STAGED"
    echo "marker verification failed; original unchanged" >&2
    exit 1
fi

mv -f "$STAGED" "$RC_LOCAL"
echo "installed=$RC_LOCAL"
echo "version=$VERSION"
echo "rollback_source=$ORIGINAL"
echo "audit_backup=$AUDIT_BACKUP"
md5sum "$RC_LOCAL" "$ORIGINAL" "$AUDIT_BACKUP"
