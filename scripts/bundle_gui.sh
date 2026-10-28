#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile=${1:-debug}
if [ "$#" -gt 0 ]; then shift; fi
case "$profile" in
    debug) cargo build --locked -p cl-rs-gui "$@" ;;
    release) cargo build --locked --release -p cl-rs-gui "$@" ;;
    *) printf 'Usage: sh scripts/bundle_gui.sh [debug|release] [cargo build options]\n' >&2; exit 2 ;;
esac
bundle="target/$profile/cl-rs.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/cl-rs-gui" "$bundle/Contents/MacOS/cl-rs-gui"
cp crates/gui/Info.plist "$bundle/Contents/Info.plist"
/usr/bin/plutil -lint "$bundle/Contents/Info.plist"
printf 'Built %s (unsigned local development bundle)\n' "$bundle"
