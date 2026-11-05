#!/bin/sh
# Captures the cl-rs GUI into a light/dark x comfortable/compact x window-size matrix.
# Requires macOS and a logged-in session (the app needs a window server).
set -eu

cd "$(dirname "$0")/.."

out=target/captures
delay=3
timeout=20
sizes=1280x800,1440x900,1920x1080
appearances=Light,Dark
densities=Comfortable,Compact
states=welcome,explorer
build=1
keep_config=0
list_only=0

usage() {
    cat >&2 <<'EOF'
Usage: sh scripts/gui_captures.sh [options]

Captures the cl-rs window for a matrix of states, appearances, densities and window
sizes into target/captures. Builds the bundle with the `ui-capture` feature first.

Options:
  --out DIR            Output directory (default: target/captures)
  --delay SECONDS      Wait before each capture (default: 3)
  --timeout SECONDS    Maximum wait for a capture file (default: 20)
  --sizes LIST         Comma-separated WxH (default: 1280x800,1440x900,1920x1080)
  --appearances LIST   Comma-separated System,Light,Dark (default: Light,Dark)
  --densities LIST     Comma-separated Comfortable,Compact (default: both)
  --states LIST        Comma-separated welcome,explorer (default: both)
  --skip-build         Use the existing bundle in target/debug
  --keep-config        Keep each cell's config, cache and logs
  --list               Print the planned cells and exit
  -h, --help           Show this help

Environment:
  CLRS_GUI_CAPTURE_DELAY is set per cell; other CLRS_* variables are set by the script.
EOF
}

die() {
    printf 'gui_captures: %s\n' "$*" >&2
    exit 1
}

note() {
    printf 'gui_captures: %s\n' "$*" >&2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) [ "$#" -ge 2 ] || die "--out needs a value"; out=$2; shift 2 ;;
        --delay) [ "$#" -ge 2 ] || die "--delay needs a value"; delay=$2; shift 2 ;;
        --timeout) [ "$#" -ge 2 ] || die "--timeout needs a value"; timeout=$2; shift 2 ;;
        --sizes) [ "$#" -ge 2 ] || die "--sizes needs a value"; sizes=$2; shift 2 ;;
        --appearances) [ "$#" -ge 2 ] || die "--appearances needs a value"; appearances=$2; shift 2 ;;
        --densities) [ "$#" -ge 2 ] || die "--densities needs a value"; densities=$2; shift 2 ;;
        --states) [ "$#" -ge 2 ] || die "--states needs a value"; states=$2; shift 2 ;;
        --skip-build) build=0; shift ;;
        --keep-config) keep_config=1; shift ;;
        --list) list_only=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) usage; die "unknown option: $1" ;;
    esac
done

case "$out" in
    ""|"/") die "refusing to use $out as the output directory" ;;
esac
case "$out" in
    /*) ;;
    *) out="$(pwd -P)/$out" ;;
esac

[ "$(uname -s)" = Darwin ] || die "the GUI only runs on macOS"

comma_list() {
    printf '%s' "$1" | tr ',' ' '
}

contains() {
    case " $1 " in
        *" $2 "*) return 0 ;;
        *) return 1 ;;
    esac
}

display_size() {
    osascript -l JavaScript -e \
        'ObjC.import("AppKit"); const f = $.NSScreen.mainScreen.frame; Math.round(f.size.width) + "x" + Math.round(f.size.height)' \
        2>/dev/null || true
}

fits() {
    width=${1%x*}
    height=${1#*x}
    [ "$width" -le "$((display_w - 20))" ] && [ "$height" -le "$((display_h - 40))" ]
}

display=$(display_size)
case "$display" in
    [0-9]*x[0-9]*) ;;
    *) display="" ;;
esac
if [ -n "$display" ]; then
    display_w=${display%x*}
    display_h=${display#*x}
else
    display_w=0
    display_h=0
fi

binary=target/debug/cl-rs.app/Contents/MacOS/cl-rs-gui

if [ "$list_only" -eq 1 ]; then
    for state in $(comma_list "$states"); do
        for appearance in $(comma_list "$appearances"); do
            for density in $(comma_list "$densities"); do
                for window in $(comma_list "$sizes"); do
                    if [ -n "$display" ] && ! fits "$window"; then
                        printf '%s\t%s\t%s\t%s\tskipped (display %s)\n' "$state" "$appearance" "$density" "$window" "$display"
                    else
                        printf '%s\t%s\t%s\t%s\n' "$state" "$appearance" "$density" "$window"
                    fi
                done
            done
        done
    done
    exit 0
fi

if [ "$build" -eq 1 ]; then
    note "building the bundle with the ui-capture feature"
    sh scripts/bundle_gui.sh debug --features ui-capture >/dev/null
fi
[ -x "$binary" ] || die "missing $binary; run without --skip-build"

fixture="$out/fixture"
case "$fixture" in
    "$out"/*) ;;
    *) die "fixture path escaped the output directory" ;;
esac
mkdir -p "$out"
chmod -R u+rwX "$fixture" 2>/dev/null || true
rm -rf "$fixture"
mkdir -p \
    "$fixture/Documents/Photos" \
    "$fixture/Projects/web/node_modules/pkg" \
    "$fixture/Downloads" \
    "$fixture/Library/Caches/Homebrew" \
    "$fixture/Library/Caches/com.example.tool" \
    "$fixture/.npm/_cacache" \
    "$fixture/.git/objects/pack" \
    "$fixture/.android/avd/Pixel.avd" \
    "$fixture/blocked" \
    "$fixture/very/deep/path/for/layout/checks"

fill() {
    dd if=/dev/zero of="$1" bs=1024 count="$2" 2>/dev/null
}

printf 'Quarterly notes. Nothing sensitive.\n' > "$fixture/Documents/notes.txt"
fill "$fixture/Documents/Photos/holiday.raw" 2048
fill "$fixture/Downloads/archive.zip" 3072
fill "$fixture/Projects/web/node_modules/pkg/index.js" 1024
printf '{"name":"web","private":true}\n' > "$fixture/Projects/web/package.json"
fill "$fixture/Library/Caches/Homebrew/brew-cache.tar" 4096
fill "$fixture/Library/Caches/com.example.tool/blob.bin" 2048
fill "$fixture/.npm/_cacache/blob" 5120
fill "$fixture/.git/objects/pack/pack-0000000000000000.pack" 1024
fill "$fixture/.android/avd/Pixel.avd/userdata-qemu.img" 1024
printf 'hidden\n' > "$fixture/.hidden-notes"
fill "$fixture/blocked/secret.bin" 256
fill "$fixture/very/deep/path/for/layout/checks/a-rather-long-file-name-that-should-truncate-in-narrow-columns.bin" 512
ln -s ../Documents/notes.txt "$fixture/Projects/link-to-notes.txt"
chmod 000 "$fixture/blocked"

mkdir -p "$out/logs"
manifest="$out/manifest.tsv"
printf 'state\tappearance\tdensity\trequested\tactual\tfile\n' > "$manifest"

captured=0
skipped=0
failed=0
pid=

cleanup() {
    if [ -n "${pid:-}" ]; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
        pid=
    fi
}
trap cleanup EXIT INT TERM

for state in $(comma_list "$states"); do
    case "$state" in
        welcome|explorer) ;;
        *) die "unknown state: $state (expected welcome or explorer)" ;;
    esac
    for appearance in $(comma_list "$appearances"); do
        case "$appearance" in
            System|Light|Dark) ;;
            *) die "unknown appearance: $appearance" ;;
        esac
        for density in $(comma_list "$densities"); do
            case "$density" in
                Comfortable|Compact) ;;
                *) die "unknown density: $density" ;;
            esac
            for window in $(comma_list "$sizes"); do
                case "$window" in
                    [0-9]*x[0-9]*) ;;
                    *) die "bad window size: $window (expected WxH)" ;;
                esac
                if [ -n "$display" ] && ! fits "$window"; then
                    note "skipping $state $appearance $density $window (display is $display)"
                    skipped=$((skipped + 1))
                    continue
                fi

                width=${window%x*}
                height=${window#*x}
                name="$state-$(printf '%s' "$appearance" | tr 'A-Z' 'a-z')-$(printf '%s' "$density" | tr 'A-Z' 'a-z')-$window"
                png="$out/$name.png"
                cell="$out/cells/$name"
                rm -rf "$cell"
                mkdir -p "$cell/config" "$cell/cache"
                rm -f "$png"

                cat > "$cell/config/settings.json" <<EOF
{
  "appearance": "$appearance",
  "density": "$density",
  "cache_enabled": true,
  "cross_devices": false,
  "sidebar_visible": true,
  "map_visible": true,
  "window": { "x": 20.0, "y": 40.0, "width": $width.0, "height": $height.0 },
  "recent": []
}
EOF

                note "capturing $name"
                if [ "$state" = explorer ]; then
                    env HOME="$fixture" \
                        CLRS_GUI_CONFIG_DIR="$cell/config" \
                        CLRS_CACHE_DIR="$cell/cache" \
                        CLRS_GUI_CAPTURE="$png" \
                        CLRS_GUI_CAPTURE_DELAY="$delay" \
                        "$binary" "$fixture" \
                        >"$cell/stdout.log" 2>"$cell/stderr.log" &
                else
                    env HOME="$fixture" \
                        CLRS_GUI_CONFIG_DIR="$cell/config" \
                        CLRS_CACHE_DIR="$cell/cache" \
                        CLRS_GUI_CAPTURE="$png" \
                        CLRS_GUI_CAPTURE_DELAY="$delay" \
                        "$binary" \
                        >"$cell/stdout.log" 2>"$cell/stderr.log" &
                fi
                pid=$!

                ticks=0
                limit=$((timeout * 10))
                while [ ! -s "$png" ] && [ "$ticks" -lt "$limit" ]; do
                    if ! kill -0 "$pid" 2>/dev/null; then
                        break
                    fi
                    sleep 0.1
                    ticks=$((ticks + 1))
                done
                cleanup

                if [ ! -s "$png" ]; then
                    failed=$((failed + 1))
                    note "FAILED $name (no capture after ${timeout}s)"
                    sed -n '1,20p' "$cell/stderr.log" >&2 || true
                    printf '%s\t%s\t%s\t%s\t-\tFAILED\n' "$state" "$appearance" "$density" "$window" >> "$manifest"
                    continue
                fi

                captured=$((captured + 1))
                actual=$(sips -g pixelWidth -g pixelHeight "$png" 2>/dev/null |
                    awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {if (w && h) print w "x" h; else print "?"}')
                printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
                    "$state" "$appearance" "$density" "$window" "$actual" "$name.png" >> "$manifest"
                if [ "$keep_config" -eq 0 ]; then
                    rm -rf "$cell"
                else
                    mv "$cell/stdout.log" "$out/logs/$name.stdout.log" 2>/dev/null || true
                    mv "$cell/stderr.log" "$out/logs/$name.stderr.log" 2>/dev/null || true
                fi
            done
        done
    done
done

trap - EXIT INT TERM
note "captured $captured, skipped $skipped, failed $failed"
note "matrix: $manifest"

[ "$failed" -eq 0 ] || exit 1
