#!/bin/sh
# Keep the executable immutable; the private body is interpreter input.
exec /bin/sh "${0%/*}/node-body.sh" "$@"
