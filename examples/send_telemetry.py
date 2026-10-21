"""Send a short local telemetry preview to Aftertouch's Local JSON adapter."""

import argparse
import json
import math
import socket
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=5300)
    parser.add_argument("--seconds", type=float, default=12)
    args = parser.parse_args()
    if not 1024 <= args.port <= 65535 or not 0 < args.seconds <= 60:
        parser.error("Use a port from 1024 to 65535 and a duration up to 60 seconds")
    print("In Aftertouch, choose Local JSON adapter and start listening on this port.")
    start = time.monotonic()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        while time.monotonic() - start < args.seconds:
            elapsed = time.monotonic() - start
            rpm = 1800 + 5300 * (0.5 + 0.5 * math.sin(elapsed * 1.5))
            packet = {
                "rpm": round(rpm),
                "maxRpm": 8000,
                "speedKmh": round(50 + rpm / 75, 1),
                "gear": 3 if rpm < 5000 else 4,
                "brake": 180 if int(elapsed) % 5 == 4 else 0,
                "slip": 1.4 if int(elapsed) % 5 == 4 else 0,
                "frontSlip": 1.3 if int(elapsed) % 5 == 4 else 0,
                "kerb": [int(elapsed * 2) % 8 == 0, False, False, False],
            }
            sock.sendto(json.dumps(packet, separators=(",", ":")).encode(), ("127.0.0.1", args.port))
            time.sleep(0.1)


if __name__ == "__main__":
    main()
