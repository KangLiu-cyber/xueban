#!/usr/bin/env python3
"""Vosk Chinese speech-to-text with timestamps.
Usage: transcribe.py <audio.wav> <out.txt>
Output format: one line per utterance -> "[MM:SS] text"
"""
import sys, os, json, zipfile, urllib.request

MODEL_NAME = "vosk-model-small-cn-0.22"
MODEL_URL = "https://alphacephei.com/vosk/models/vosk-model-small-cn-0.22.zip"


def ensure_model():
    model_dir = os.path.expanduser("~/.trae-cn/skills-models/" + MODEL_NAME)
    if os.path.exists(model_dir):
        return model_dir
    os.makedirs(os.path.dirname(model_dir), exist_ok=True)
    zip_path = "/tmp/vosk-cn.zip"
    print(f"[transcribe] downloading {MODEL_NAME} (~40MB)...", file=sys.stderr)
    urllib.request.urlretrieve(MODEL_URL, zip_path)
    with zipfile.ZipFile(zip_path) as z:
        z.extractall(os.path.dirname(model_dir))
    print("[transcribe] model ready", file=sys.stderr)
    return model_dir


def main():
    if len(sys.argv) < 3:
        print("usage: transcribe.py <audio.wav> <out.txt>", file=sys.stderr)
        sys.exit(1)
    audio, out = sys.argv[1], sys.argv[2]
    model_dir = ensure_model()

    from vosk import Model, KaldiRecognizer
    import wave

    model = Model(model_dir)
    wf = wave.open(audio, "rb")
    if wf.getnchannels() != 1 or wf.getsampwidth() != 2:
        print("[transcribe] ERROR: audio must be 16-bit mono WAV (run prepare_audio.sh)", file=sys.stderr)
        sys.exit(1)
    rec = KaldiRecognizer(model, wf.getframerate())
    rec.SetWords(True)

    lines = []

    def flush(result):
        words = result.get("result") or []
        text = (result.get("text") or "").strip()
        if not text:
            return
        start = words[0]["start"] if words else 0
        m, s = divmod(int(start), 60)
        lines.append(f"[{m:02d}:{s:02d}] {text}")

    while True:
        data = wf.readframes(4000)
        if not data:
            break
        if rec.AcceptWaveform(data):
            flush(json.loads(rec.Result()))
    flush(json.loads(rec.FinalResult()))

    with open(out, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"[transcribe] wrote {len(lines)} segments -> {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
