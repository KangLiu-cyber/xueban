#!/bin/bash
# Usage: frames.sh <video> <outdir> [scene_threshold]
# Extracts keyframes at scene changes (PPT slide transitions) + timestamps.txt
set -e
VIDEO="$1"; OUTDIR="$2"; THRESH="${3:-0.08}"
[ -z "$VIDEO" ] && { echo "usage: frames.sh <video> <outdir> [threshold]"; exit 1; }
mkdir -p "$OUTDIR"
rm -f "$OUTDIR"/frame_*.jpg "$OUTDIR/timestamps.txt"

# First frame (scene filter can miss the opening slide)
ffmpeg -y -loglevel error -ss 0 -i "$VIDEO" -frames:v 1 -q:v 2 "$OUTDIR/frame_0000.jpg"
echo "0.0" > "$OUTDIR/timestamps.txt"

# Scene-change frames, numbered from 0001 so frame_0000 stays the opening
ffmpeg -loglevel info -i "$VIDEO" \
    -vf "select='gt(scene,$THRESH)',showinfo" \
    -vsync vfr -start_number 1 -q:v 2 "$OUTDIR/frame_%04d.jpg" 2>&1 \
    | grep -oE 'pts_time:[0-9.]+' | sed 's/pts_time://' >> "$OUTDIR/timestamps.txt" || true

COUNT=$(ls "$OUTDIR"/frame_*.jpg 2>/dev/null | wc -l | tr -d ' ')
echo "[frames] extracted $COUNT frames (threshold=$THRESH) -> $OUTDIR"
echo "[frames] timestamps.txt line N (1-based) <-> frame files in sorted order"
