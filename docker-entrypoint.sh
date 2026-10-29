#!/bin/sh
# Start as root just long enough to hand the writable volumes to PUID:PGID,
# then drop privileges for good -- the same PUID/PGID convention the
# LinuxServer.io Sonarr/Radarr images use.
#
# Volumes created by older (root-running) versions of this image are
# root-owned; the chown below is what lets an upgraded container keep writing
# its existing database instead of failing on first start.
set -e

PUID="${PUID:-1000}"
PGID="${PGID:-1000}"

# Already started non-root (e.g. `docker run --user`), or PUID=0 to opt out
# and keep the old run-as-root behavior: nothing to fix up, just run.
if [ "$(id -u)" != "0" ] || [ "$PUID" = "0" ]; then
    exec /app/meme-arr "$@"
fi

for dir in "${DATA_DIR:-/data}" "${OUTPUT_DIR:-/output}"; do
    mkdir -p "$dir"
    # Only touch what isn't owned correctly yet -- cheap after the first start,
    # and doesn't abort on mounts (e.g. CIFS) that refuse chown.
    if ! find "$dir" \( ! -user "$PUID" -o ! -group "$PGID" \) \
            -exec chown "$PUID:$PGID" {} + 2>/dev/null; then
        echo "warning: couldn't chown everything under $dir to $PUID:$PGID." \
             "If writes fail, fix that mount's permissions or set PUID=0." >&2
    fi
done

# A numeric PUID has no passwd entry or home dir; give fontconfig (used by
# the subtitle-burning filter) somewhere writable for its cache.
export HOME=/tmp

exec setpriv --reuid="$PUID" --regid="$PGID" --clear-groups /app/meme-arr "$@"
