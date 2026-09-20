#!/usr/bin/env bash
set -euo pipefail

BINARY="${1:-target/debug/jame-prompt}"
EXPECTED='JamePrompt native smoke ñ 123'
TARGET_TITLE='JamePrompt Native Hotkey Smoke Target'
TEMP_DIR="$(mktemp -d)"
APP_LOG="$TEMP_DIR/jame-prompt.log"
TARGET_OUTPUT="$TEMP_DIR/target.txt"
APP_PID=""
TARGET_PID=""

cleanup() {
    if [[ -n "$TARGET_PID" ]] && kill -0 "$TARGET_PID" 2>/dev/null; then
        kill "$TARGET_PID" 2>/dev/null || true
    fi
    if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
        kill "$APP_PID" 2>/dev/null || true
    fi
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

fail() {
    echo "native hotkey X11 smoke failed: $1" >&2
    if [[ -f "$APP_LOG" ]]; then
        echo "----- JamePrompt log -----" >&2
        cat "$APP_LOG" >&2 || true
    fi
    exit 1
}

if [[ -z "${DISPLAY:-}" ]]; then
    fail "DISPLAY is not set"
fi

if [[ ! -x "$BINARY" ]]; then
    fail "binary is not executable: $BINARY"
fi

export XDG_SESSION_TYPE=x11
export GDK_BACKEND=x11
export GTK_A11Y=none
export JAME_PROMPT_UI_SMOKE_DURATION_MS=30000

"$BINARY" --native-hotkey-smoke >"$APP_LOG" 2>&1 &
APP_PID=$!

APP_WINDOW=""
for _ in $(seq 1 100); do
    if ! kill -0 "$APP_PID" 2>/dev/null; then
        fail "JamePrompt exited before registering the smoke hotkey"
    fi

    APP_WINDOW="$(xdotool search --name '^JamePrompt$' 2>/dev/null | head -n 1 || true)"
    if [[ -n "$APP_WINDOW" ]]; then
        break
    fi
    sleep 0.1
done

if [[ -z "$APP_WINDOW" ]]; then
    fail "JamePrompt window did not appear"
fi

zenity --entry --title="$TARGET_TITLE" --text="Waiting for JamePrompt auto-paste..." >"$TARGET_OUTPUT" &
TARGET_PID=$!

TARGET_WINDOW=""
for _ in $(seq 1 100); do
    if ! kill -0 "$TARGET_PID" 2>/dev/null; then
        fail "GTK target exited before receiving paste"
    fi

    TARGET_WINDOW="$(xdotool search --name "^$TARGET_TITLE$" 2>/dev/null | head -n 1 || true)"
    if [[ -n "$TARGET_WINDOW" ]]; then
        break
    fi
    sleep 0.1
done

if [[ -z "$TARGET_WINDOW" ]]; then
    fail "GTK target window did not appear"
fi

xdotool windowmap --sync "$TARGET_WINDOW"
xdotool windowfocus --sync "$TARGET_WINDOW"
FOCUSED_WINDOW="$(xdotool getwindowfocus)"
if [[ "$FOCUSED_WINDOW" != "$TARGET_WINDOW" ]]; then
    fail "GTK target is not the focused X11 window"
fi

xdotool key --clearmodifiers ctrl+shift+p
sleep 1

if ! kill -0 "$APP_PID" 2>/dev/null; then
    fail "JamePrompt exited while processing the global hotkey"
fi

xdotool key Return
wait "$TARGET_PID" || fail "GTK target did not complete normally"
TARGET_PID=""

ACTUAL="$(cat "$TARGET_OUTPUT")"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
    fail "expected exact pasted content '$EXPECTED' but received '$ACTUAL'"
fi

echo "X11 native hotkey auto-paste smoke passed"
