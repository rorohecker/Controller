"""Map documented game/mod JSON lines to CONTROLLER's local recognition receiver.

Example: python examples/weapon_adapter.py --field weapon.name --map weapons.json
Map file: {"weapon_ids": {"game_rifle_id": "smg", "game_bow_id": "bow"}}
No game memory access, file polling, game hooks or automatic gun database.
"""
import argparse
import json
from pathlib import Path
import socket
import sys
import time


def packet(event, field, mapping, adapter):
    value = event
    for key in field.split('.'):
        if not isinstance(value, dict) or key not in value:
            return None
        value = value[key]
    weapon = mapping.get(str(value))
    if not isinstance(weapon, str) or not weapon.strip() or len(weapon.encode()) > 80 or any(ord(c) < 32 for c in weapon):
        return None
    return {'detection': {'weapon': weapon, 'source': 'telemetry', 'confidence': 1, 'ageMs': 0, 'adapter': adapter}}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--map', type=Path, required=True)
    p.add_argument('--field', default='weapon')
    p.add_argument('--port', type=int, default=6969)
    p.add_argument('--adapter', default='documented-json')
    args = p.parse_args()
    if not 1024 <= args.port <= 65535 or not 1 <= len(args.adapter) <= 80 or args.map.stat().st_size > 65536:
        p.error('Invalid port, adapter name or oversized mapping')
    mapping = json.loads(args.map.read_text(encoding='utf-8'))['weapon_ids']
    if not isinstance(mapping, dict) or len(mapping) > 100:
        p.error('Use up to 100 explicit weapon ID mappings')
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
        last = 0.0
        for line in sys.stdin:
            if len(line) > 8192:
                continue
            try:
                payload = packet(json.loads(line), args.field, mapping, args.adapter)
            except (ValueError, TypeError):
                continue
            if payload:
                # Bounded sender; confidence does not attest that the input is authentic telemetry.
                time.sleep(max(0, .05 - (time.monotonic() - last)))
                sender.sendto(json.dumps(payload).encode(), ('127.0.0.1', args.port))
                last = time.monotonic()


if __name__ == '__main__':
    main()
