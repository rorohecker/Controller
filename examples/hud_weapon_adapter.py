"""Optional local HUD template adapter. Requires Pillow >=12.1 and mss >=10; no model or game images included.

Configuration: {"region":{"left":100,"top":100,"width":160,"height":80},
"templates":[{"weapon":"smg","file":"smg-icon.png"}],"threshold":0.92,"margin":0.1}
Capture the same HUD crop for every template. Match confidence is a similarity score,
not a probability. The app must explicitly allow HUD observations before using it.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import socket
import sys
import time


def features(values):
    if not values:
        return None
    mean = sum(values) / len(values)
    centered = [v - mean for v in values]
    magnitude = math.sqrt(sum(v * v for v in centered))
    if magnitude / math.sqrt(len(values)) < 5:
        return None  # Blank crops cannot identify weapons.
    return [v / magnitude for v in centered]


def match(values, templates, threshold=.92, margin=.1):
    vector = features(values)
    if vector is None:
        return None
    matches = sorted(((sum(a * b for a, b in zip(vector, t)), weapon) for weapon, t in templates), reverse=True)
    if not matches or len(vector) != len(templates[0][1]):
        return None
    score, weapon = matches[0]
    next_score = matches[1][0] if len(matches) > 1 else -1
    if score < threshold or score - next_score < margin:
        return None
    return weapon, min(1, max(0, score))


def validate(config):
    region = config['region']
    if set(region) != {'left', 'top', 'width', 'height'} or not all(type(v) is int for v in region.values()):
        raise ValueError('Use integer left/top/width/height coordinates')
    if not 8 <= region['width'] <= 1024 or not 8 <= region['height'] <= 1024 or region['width'] * region['height'] > 512 * 512:
        raise ValueError('Choose a HUD crop of at most 262144 pixels')
    templates = config['templates']
    if not isinstance(templates, list) or not 1 <= len(templates) <= 16:
        raise ValueError('Use 1-16 explicit weapon templates')
    names = set()
    for template in templates:
        name = template['weapon']
        if not isinstance(name, str) or not name.strip() or len(name.encode()) > 80 or any(ord(c) < 32 for c in name) or name in names:
            raise ValueError('Use distinct, nonempty weapon names')
        names.add(name)
    for name, default, low, high in [('threshold', .92, .5, 1), ('margin', .1, .01, 1)]:
        value = config.get(name, default)
        if type(value) not in (float, int) or not math.isfinite(value) or not low <= value <= high:
            raise ValueError('Invalid template threshold or margin')
    return region


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('config', type=Path)
    parser.add_argument('--port', type=int, default=6969)
    parser.add_argument('--fps', type=int, default=5)
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    if not 1024 <= args.port <= 65535 or not 1 <= args.fps <= 10 or args.config.stat().st_size > 65536:
        parser.error('Invalid port, frame rate or oversized configuration')
    config = json.loads(args.config.read_text(encoding='utf-8'))
    region = validate(config)
    from PIL import Image, ImageOps
    import mss
    Image.MAX_IMAGE_PIXELS = 4_000_000
    templates, fingerprints = [], set()
    for entry in config['templates']:
        path = args.config.parent / entry['file']
        if path.stat().st_size > 4 * 1024 * 1024:
            parser.error('Template exceeds 4 MiB')
        with Image.open(path) as image:
            if image.width*image.height>4_000_000:
                parser.error('Template exceeds four million pixels')
            image = ImageOps.grayscale(image).resize((32, 16))
            values = list(image.get_flattened_data())
            fingerprint = hashlib.sha256(bytes(values)).hexdigest()
            vector = features(values)
            if vector is None or fingerprint in fingerprints:
                parser.error('Templates must have detail and distinct crops')
            fingerprints.add(fingerprint)
            templates.append((entry['weapon'], vector))
    previous = None
    with mss.MSS() as capture, socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
        while True:
            started = time.monotonic()
            shot = capture.grab(region)
            image = ImageOps.grayscale(Image.frombytes('RGB', shot.size, bytes(shot.rgb))).resize((32, 16))
            result = match(list(image.get_flattened_data()), templates, config.get('threshold', .92), config.get('margin', .1))
            if result:
                weapon, confidence = result
                packet = {'detection': {'weapon': weapon, 'source': 'hud', 'confidence': confidence,
                                       'ageMs': int((time.monotonic() - started) * 1000), 'adapter': 'local-hud-template'}}
                if args.dry_run:
                    print(json.dumps(packet), flush=True)
                else:
                    sender.sendto(json.dumps(packet).encode(), ('127.0.0.1', args.port))
                if previous != weapon:
                    print(f'Matched {weapon}: {confidence:.3f}', file=sys.stderr)
                previous = weapon
            else:
                previous = None  # No guess; the app expires the previous observation.
            time.sleep(max(0, 1 / args.fps - (time.monotonic() - started)))


if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        pass
