"""Live USB feature integration: silent PCM, local UDP telemetry, input-click arming."""
import json
import queue
import socket
import struct
import subprocess
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    exe = ROOT / "target" / "debug" / "aftertouch.exe"
    core = subprocess.Popen([str(exe), "--core"], cwd=ROOT, stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    events = queue.Queue()
    def reader():
        for line in core.stdout:
            events.put(json.loads(line))
    threading.Thread(target=reader, daemon=True).start()
    seen = []

    def send(action, **kwargs):
        core.stdin.write(json.dumps(dict(action=action, **kwargs)) + "\n")
        core.stdin.flush()

    def until(match, seconds=5):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            try:
                event = events.get(timeout=0.2)
            except queue.Empty:
                continue
            seen.append(event)
            if event.get("type") == "error":
                raise AssertionError(event)
            if match(event):
                return event
        raise AssertionError(f"Timed out; recent events: {seen[-8:]}")

    try:
        device = until(lambda e: e.get("type") == "device" and e.get("connected"), 7)
        assert device["transport"] == "USB", "This integration check needs USB audio."

        send("setAudioBridge", enabled=True, gain=0.0, preset="gentle")
        until(lambda e: e.get("type") == "audioStatus" and e.get("bridge") is True)
        audio = until(lambda e: e.get("type") == "audioStatus" and e.get("callbacks", 0) > 0, 3)
        send("setAudioBridge", enabled=False, gain=0.0, preset="gentle")
        until(lambda e: e.get("type") == "audioStatus" and e.get("bridge") is False)
        send("playHaptic", shape="sine", frequency=90, attack=4, release=80,
             strength=10, pan=0)
        until(lambda e: e.get("type") == "audioStatus" and e.get("route") == "voiceCoil")

        send("setInputEffects", enabled=True, threshold=0.95, strength=10)
        until(lambda e: e.get("type") == "inputEffects" and e.get("enabled") is True)
        send("setInputEffects", enabled=False, threshold=0.95, strength=10)
        until(lambda e: e.get("type") == "inputEffects" and e.get("enabled") is False)

        send("setLowBatteryAlert", enabled=True)
        until(lambda e: e.get("type") == "hapticAlertStatus" and e.get("lowBattery") is True)
        send("setHapticTimer", seconds=1)
        until(lambda e: e.get("type") == "hapticAlertStatus" and e.get("timerSeconds") == 1)
        until(lambda e: e.get("type") == "hapticAlert" and "timer" in e.get("message", "").lower(), 3)
        send("setLowBatteryAlert", enabled=False)
        until(lambda e: e.get("type") == "hapticAlertStatus" and e.get("lowBattery") is False)

        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        send("setTelemetry", enabled=True, port=port)
        until(lambda e: e.get("type") == "telemetryStatus" and e.get("enabled") is True)
        packet = bytearray(311)
        struct.pack_into("<I", packet, 0, 1)
        struct.pack_into("<f", packet, 8, 8000.0)
        struct.pack_into("<f", packet, 16, 6200.0)
        struct.pack_into("<f", packet, 244, 30.0)
        packet[232 + 75] = 3
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            for _ in range(3):
                sock.sendto(packet, ("127.0.0.1", port))
                time.sleep(0.05)
        sample = until(lambda e: e.get("type") == "telemetry" and e.get("gear") == 3, 3)
        send("setTelemetry", enabled=True, port=port, lan=True)
        until(lambda e: e.get("type") == "telemetryStatus" and e.get("lan") is True)
        struct.pack_into("<f", packet, 16, 6300.0)
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            for _ in range(3):
                sock.sendto(packet, ("127.0.0.1", port))
                time.sleep(0.05)
        until(lambda e: e.get("type") == "telemetry" and e.get("rpm") == 6300.0, 3)
        send("setTelemetry", enabled=True, port=port, lan=False, format="json")
        until(lambda e: e.get("type") == "telemetryStatus" and e.get("enabled") is True and e.get("format") == "json")
        adapter_packet = json.dumps({"rpm": 6400, "maxRpm": 8000, "speedKmh": 112,
                                     "gear": 4, "brake": 160, "slip": 1.4,
                                     "frontSlip": 1.3, "kerb": [True, False, False, False]}).encode()
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            for _ in range(3):
                sock.sendto(adapter_packet, ("127.0.0.1", port))
                time.sleep(0.05)
        until(lambda e: e.get("type") == "telemetry" and e.get("source") == "Local JSON" and e.get("rpm") == 6400.0, 3)
        send("setTelemetry", enabled=False, port=port)
        until(lambda e: e.get("type") == "telemetryStatus" and e.get("enabled") is False)
        send("reset")
        until(lambda e: e.get("type") == "notice" and e.get("message") == "Effects reset")
        print(f"Feature smoke passed: {audio['callbacks']} PCM callbacks; {sample['rpm']} RPM Forza and local JSON telemetry; input click armed and cleared")
    finally:
        core.stdin.close()
        try:
            core.wait(timeout=5)
        except subprocess.TimeoutExpired:
            core.terminate()


if __name__ == "__main__":
    main()
