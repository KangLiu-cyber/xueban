#!/bin/bash
# Usage: download.sh <bilibili_url> <workdir> [cookies_file]
# Downloads a single Bilibili episode (respects the p= parameter) as video.mp4
set -e
URL="$1"
WORKDIR="$2"
COOKIES="$3"
[ -z "$URL" ] && { echo "usage: download.sh <bilibili_url> <workdir> [cookies]"; exit 1; }
mkdir -p "$WORKDIR"

# Extract p param (default 1)
P=$(echo "$URL" | grep -oE '[?&]p=[0-9]+' | grep -oE '[0-9]+' | head -1)
P=${P:-1}
# Base URL without query string
BASE=$(echo "$URL" | sed -E 's/[?].*$//')

echo "[download] episode p=$P from $BASE"
COOKIE_ARG=""
[ -n "$COOKIES" ] && COOKIE_ARG="--cookies $COOKIES"

yt-dlp --playlist-items "$P" \
    -f "bv*[height<=720]+ba/b[height<=720]/best" \
    --merge-output-format mp4 \
    $COOKIE_ARG \
    -o "$WORKDIR/video.mp4" \
    "$BASE" || true

# Normalize output filename if yt-dlp appended extra suffixes
if [ ! -f "$WORKDIR/video.mp4" ]; then
    FOUND=$(ls "$WORKDIR"/video*.mp4 2>/dev/null | head -1)
    [ -n "$FOUND" ] && mv "$FOUND" "$WORKDIR/video.mp4"
fi
[ -f "$WORKDIR/video.mp4" ] && echo "[download] done: $WORKDIR/video.mp4" || { echo "[download] FAILED"; exit 1; }
