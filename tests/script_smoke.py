"""Core integration for bounded telemetry scripts; no controller needed."""

import json
import queue
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def main():
    exe = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target" / "debug" / "controller.exe"
    core = subprocess.Popen(
        [str(exe), "--core", "--no-hid"], cwd=ROOT, stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1,
    )
    events = queue.Queue()

    def reader():
        for line in core.stdout:
            events.put(json.loads(line))

    threading.Thread(target=reader, daemon=True).start()

    def send(action, **kwargs):
        core.stdin.write(json.dumps(dict(action=action, **kwargs)) + "\n")
        core.stdin.flush()

    def until(match, seconds=4):
        deadline = time.monotonic() + seconds
        seen = []
        while time.monotonic() < deadline:
            try:
                event = events.get(timeout=0.2)
            except queue.Empty:
                continue
            seen.append(event)
            if match(event):
                return event
        raise AssertionError(f"Timed out waiting for core event; recent: {seen[-8:]}")

    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        send("setTelemetry", enabled=True, port=port, format="json")
        until(lambda event: event.get("type") == "telemetryStatus" and event.get("enabled") is True)
        send("setScript", enabled=True, source='if rpm > 6000.0 { #{ light: [200, 30, 30], cue: "pulse" } } else { #{} }')
        until(lambda event: event.get("type") == "scriptStatus" and event.get("enabled") is True)
        packet = json.dumps({"rpm": 6800, "maxRpm": 8000, "gear": 4}).encode()
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            for _ in range(3):
                sock.sendto(packet, ("127.0.0.1", port))
                time.sleep(0.12)
        until(lambda event: event.get("type") == "scriptOutput" and event.get("light") == [200, 30, 30] and event.get("cue") == "pulse")
        until(lambda event: event.get("type") == "telemetry" and event.get("revColor") == [200, 30, 30])

        send("setScript", enabled=True, source="loop {} #{}")
        until(lambda event: event.get("type") == "scriptStatus" and event.get("enabled") is True)
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            for _ in range(3):
                sock.sendto(packet, ("127.0.0.1", port))
                time.sleep(0.12)
        stopped = until(lambda event: event.get("type") == "scriptStatus" and event.get("enabled") is False)
        assert "Script stopped" in stopped["message"]
        assert core.poll() is None, "Core exited after script failure"
        send("setTelemetry", enabled=False, port=port)
        until(lambda event: event.get("type") == "telemetryStatus" and event.get("enabled") is False)
        send("setGameLink", settings={"enabled": True, "format": "json", "port": port})
        until(lambda event: event.get("type") == "gameLinkStatus" and event.get("enabled") is True)
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.sendto(b'{"weapon":"smg","events":["dash"]}', ("127.0.0.1", port))
        received = until(lambda event: event.get("type") == "gameEvent")
        assert received["weapon"] == "smg" and received["events"] == ["dash"]
        # Switching format on the same port must reuse the listener.
        send("setGameLink", settings={"enabled": True, "format": "dsx", "port": port})
        until(lambda event: event.get("type") == "gameLinkStatus" and event.get("enabled") is True)
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(2)
            sock.sendto(b'{"instructions":[{"type":0,"parameters":[0]}]}', ("127.0.0.1", port))
            assert json.loads(sock.recv(4096))["Status"] == "OK"
        send("setGameLink", settings={"enabled": False, "format": "dsx", "port": port})
        until(lambda event: event.get("type") == "gameLinkStatus" and event.get("enabled") is False)
        print("Script smoke passed: racing telemetry, bounded scripts, weapon/events UDP, DSX status and port reuse")
    finally:
        core.stdin.close()
        try:
            core.wait(timeout=5)
        except subprocess.TimeoutExpired:
            core.terminate()


if __name__ == "__main__":
    main()
