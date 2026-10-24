"""Live USB feature integration: audio, light modes, alerts, and telemetry."""
import json
import queue
import socket
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    exe = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target" / "debug" / "controller.exe"
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
        live_input = until(lambda e: e.get("type") == "input" and e.get("input", {}).get("fullReport"))
        assert len(live_input["input"]["buttons"]) == 23
        assert len(live_input["input"]["axes"]) == 4
        assert len(live_input["input"]["gyro"]) == 3
        assert 0 <= live_input["input"]["batteryInfo"]["sensorByte"] <= 255
        send("listDevices")
        listed = until(lambda e: e.get("type") == "devices" and e.get("selected") == device["path"])
        assert any(item["path"] == device["path"] for item in listed["items"])

        send("setLightbar", rgb=[24, 80, 128])
        until(lambda e: e.get("type") == "notice" and e.get("message") == "Lightbar set")
        send("batteryLightbar", enabled=True)
        until(lambda e: e.get("type") == "lightMode" and e.get("battery") is True)
        send("setScreenLightbar", enabled=True)
        screen_mode = until(lambda e: e.get("type") == "lightMode" and e.get("screen") is True)
        assert screen_mode["battery"] is False
        screen_color = until(lambda e: e.get("type") == "screenColor", 4)
        assert len(screen_color["rgb"]) == 3
        send("setTemperatureLightbar", enabled=True)
        sensor_mode = until(lambda e: e.get("type") == "lightMode" and e.get("sensor") is True)
        assert sensor_mode["battery"] is False
        sensor_color = until(lambda e: e.get("type") == "sensorColor", 4)
        assert 0 <= sensor_color["sensorByte"] <= 255
        before_release = len(seen)
        send("releaseLightbar")
        until(lambda e: e.get("type") == "notice" and e.get("message") == "Lightbar released")
        assert any(e.get("type") == "lightMode" and e.get("battery") is False
                   and e.get("screen") is not True and e.get("sensor") is not True
                   for e in seen[before_release:])

        send("setTouchMouse", enabled=True)
        until(lambda e: e.get("type") == "mouseMode" and e.get("enabled") is True)
        send("setTouchGestures", enabled=True)
        until(lambda e: e.get("type") == "touchGestures" and e.get("enabled") is True)
        send("setTouchGestures", enabled=False)
        until(lambda e: e.get("type") == "touchGestures" and e.get("enabled") is False)
        send("setTouchMouse", enabled=False)
        until(lambda e: e.get("type") == "mouseMode" and e.get("enabled") is False)

        send("setAudioBridge", enabled=True, gain=0.0, preset="gentle")
        until(lambda e: e.get("type") == "audioStatus" and e.get("bridge") is True)
        audio = until(lambda e: e.get("type") == "audioStatus" and e.get("callbacks", 0) > 0, 3)
        assert isinstance(audio.get("glitches"), int)
        send("playSpeakerChirp")
        until(lambda e: e.get("type") == "speakerStatus" and "Chirp sent" in e.get("message", ""))
        send("setAudioBridge", enabled=False, gain=0.0, preset="gentle")
        until(lambda e: e.get("type") == "audioStatus" and e.get("bridge") is False)
        send("playHaptic", shape="sine", frequency=90, attack=4, release=80,
             strength=10, pan=0)
        until(lambda e: e.get("type") == "audioStatus" and e.get("route") == "voiceCoil")
        send("rumble", left=10, right=10, durationMs=50)
        until(lambda e: e.get("type") == "notice" and e.get("message") == "Rumble playing")

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
        send("setScript", enabled=True, source='if rpm / max_rpm > 0.7 { #{ light: [200, 30, 30], cue: "pulse" } } else { #{} }')
        until(lambda e: e.get("type") == "scriptStatus" and e.get("enabled") is True)
        adapter_packet = json.dumps({"rpm": 6400, "maxRpm": 8000, "speedKmh": 112,
                                     "gear": 4, "brake": 160, "slip": 1.4,
                                     "frontSlip": 1.3, "kerb": [True, False, False, False]}).encode()
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            for _ in range(3):
                sock.sendto(adapter_packet, ("127.0.0.1", port))
                time.sleep(0.05)
        until(lambda e: e.get("type") == "scriptOutput" and e.get("light") == [200, 30, 30] and e.get("cue") == "pulse", 3)
        until(lambda e: e.get("type") == "telemetry" and e.get("source") == "Local JSON" and e.get("rpm") == 6400.0 and e.get("revColor") == [200, 30, 30], 3)
        send("setScript", enabled=False)
        until(lambda e: e.get("type") == "scriptStatus" and e.get("enabled") is False)
        send("setScreenLightbar", enabled=True)
        until(lambda e: e.get("type") == "telemetryStatus" and e.get("enabled") is False)
        until(lambda e: e.get("type") == "lightMode" and e.get("screen") is True)
        send("setHapticTimer", seconds=30)
        until(lambda e: e.get("type") == "hapticAlertStatus" and e.get("timerSeconds") == 30)
        send("setInputEffects", enabled=True, threshold=0.95, strength=10)
        until(lambda e: e.get("type") == "inputEffects" and e.get("enabled") is True)
        before_reset = len(seen)
        send("reset")
        until(lambda e: e.get("type") == "notice" and e.get("message") == "Effects reset")
        assert any(e.get("type") == "safetyReset" for e in seen[before_reset:])
        assert any(e.get("type") == "lightMode" and e.get("battery") is False for e in seen[before_reset:])
        assert any(e.get("type") == "triggerLease" and e.get("active") is False for e in seen[before_reset:])
        print(f"Feature smoke passed: USB light modes and raw sensor byte {sensor_color['sensorByte']}; "
              f"{audio['callbacks']} PCM callbacks; {sample['rpm']} RPM Forza and local JSON telemetry; "
              "input click armed and cleared")
    finally:
        core.stdin.close()
        try:
            core.wait(timeout=5)
        except subprocess.TimeoutExpired:
            core.terminate()


if __name__ == "__main__":
    main()
