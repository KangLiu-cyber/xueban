#!/bin/bash
# Usage: prepare_audio.sh <video> <out_wav>
# Extracts audio and converts to 16kHz mono 16-bit PCM WAV (Vosk requirement)
set -e
[ -z "$1" ] && { echo "usage: prepare_audio.sh <video> <out_wav>"; exit 1; }
ffmpeg -y -loglevel error -i "$1" -vn -ac 1 -ar 16000 -acodec pcm_s16le "$2"
echo "[audio] ready: $2 (16kHz mono)"
