"""Offline WAV evidence, not inference or a perceptual quality score. Standard library only."""
from __future__ import annotations
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct


def read_wav(path: Path):
    if path.stat().st_size > 32 * 1024 * 1024:
        raise ValueError('WAV exceeds 32 MiB')
    blob = path.read_bytes()
    if len(blob) < 12 or blob[:4] != b'RIFF' or blob[8:12] != b'WAVE':
        raise ValueError('Expected RIFF/WAVE')
    pos, fmt, pcm = 12, None, None
    while pos + 8 <= len(blob):
        tag, size = struct.unpack_from('<4sI', blob, pos)
        pos += 8
        if size > len(blob) - pos:
            raise ValueError('Truncated WAV chunk')
        if tag == b'fmt ':
            if size < 16:
                raise ValueError('Truncated WAV format')
            fmt = struct.unpack_from('<HHIIHH', blob, pos)
        elif tag == b'data':
            pcm = blob[pos:pos + size]
        pos += size + (size & 1)
    if fmt is None or pcm is None:
        raise ValueError('Missing WAV format or data')
    kind, channels, rate, _, align, bits = fmt
    if not 1 <= channels <= 4 or not 1000 <= rate <= 192000 or (kind, bits) not in [(1, 16), (3, 32)]:
        raise ValueError('Use 16-bit PCM or 32-bit float WAV, 1-4 channels')
    width = bits // 8
    if align != channels * width or len(pcm) % align:
        raise ValueError('Invalid WAV block alignment')
    if len(pcm)//align>3_000_000 or len(pcm)/align/rate>30:
        raise ValueError('Use a clip of at most 30 seconds and three million frames')
    samples = [v[0] / 32768 if kind == 1 else v[0] for v in struct.iter_unpack('<h' if kind == 1 else '<f', pcm)]
    if not samples or len(samples) / channels / rate > 300 or not all(math.isfinite(v) for v in samples):
        raise ValueError('Empty, non-finite or longer than five minutes')
    mono = [sum(samples[i:i + channels]) / channels for i in range(0, len(samples), channels)]
    return mono, rate, {'sha256': hashlib.sha256(blob).hexdigest(), 'channels': channels, 'sampleRate': rate, 'frames': len(mono)}


def envelope(samples, rate):
    size = max(1, rate // 100)
    return [math.sqrt(sum(v * v for v in samples[i:i + size]) / len(samples[i:i + size])) for i in range(0, len(samples), size)]


def onset_alignment(reference, candidate):
    """Positive lag means candidate envelope follows reference; search only +/-200 ms."""
    best = (-2.0, 0)
    for lag in range(-20, 21):
        a, b = (reference[-lag:], candidate) if lag < 0 else (reference, candidate[lag:])
        n = min(len(a), len(b))
        if n < 10:
            continue
        a, b = a[:n], b[:n]
        am, bm = sum(a) / n, sum(b) / n
        va, vb = sum((v - am) ** 2 for v in a), sum((v - bm) ** 2 for v in b)
        if va * vb <= 1e-16:
            continue
        score = sum((x - am) * (y - bm) for x, y in zip(a, b)) / math.sqrt(va * vb)
        # On a tied periodic signal, prefer the smallest displacement.
        if score > best[0] + 1e-9 or abs(score - best[0]) <= 1e-9 and abs(lag) < abs(best[1]):
            best = score, lag
    return {'envelopeCorrelation': round(best[0], 5) if best[0] >= -1 else None, 'envelopeLagMs': best[1] * 10 if best[0] >= -1 else None}


def signal_stats(samples, rate):
    peak = max(abs(v) for v in samples)
    rms = math.sqrt(sum(v * v for v in samples) / len(samples))
    dc = sum(samples) / len(samples)
    # A narrow frequency scan describes the waveform; it is not a DualSense transfer-function measurement.
    powers = []
    for frequency in range(20, 301, 5):
        coefficient = 2 * math.cos(2 * math.pi * frequency / rate)
        first = second = 0.0
        for v in samples:
            value = v - dc + coefficient * first - second
            second, first = first, value
        powers.append((first * first + second * second - coefficient * first * second, frequency))
    return {'durationSeconds': round(len(samples) / rate, 6), 'peak': round(peak, 6), 'rms': round(rms, 6),
            'dc': round(dc, 6), 'crestFactor': round(peak / rms, 5) if rms else None,
            'clippedFraction': round(sum(abs(v) >= .999 for v in samples) / len(samples), 6),
            'dominantScanHz': max(powers)[1] if rms > 1e-8 else None,
            'comparisonGain': round(min(.08 / rms, .25 / peak), 6) if rms and peak else 0}


def compare(reference_path: Path, candidate_path: Path):
    reference, reference_rate, reference_meta = read_wav(reference_path)
    candidate, candidate_rate, candidate_meta = read_wav(candidate_path)
    return {'reference': reference_path.name, 'candidate': candidate_path.name, 'referenceMetadata': reference_meta,
            'candidateMetadata': candidate_meta, 'stats': signal_stats(candidate, candidate_rate),
            'durationErrorMs': round((len(candidate) / candidate_rate - len(reference) / reference_rate) * 1000, 3),
            **onset_alignment(envelope(reference, reference_rate), envelope(candidate, candidate_rate))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pair', nargs=2, action='append', required=True, metavar=('SOURCE_WAV', 'CANDIDATE_WAV'))
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    report = {'schemaVersion': 1, 'method': 'Precomputed-waveform inspection; no model inference, hardware playback, quality ranking or app-runtime DSP comparison',
              'comparison': 'Envelope correlation is level independent. Positive lag means candidate follows source. Suggested gains target RMS 0.08 with peak <=0.25.',
              'pairs': [compare(Path(a), Path(b)) for a, b in args.pair]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f'Offline haptic evaluation: {len(report["pairs"])} pairs; no playback or model execution')


if __name__ == '__main__':
    main()
