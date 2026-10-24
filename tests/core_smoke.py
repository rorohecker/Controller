"""Live USB smoke check after `cargo build --bin controller`.

Requires a connected DualSense. Briefly sends a 10% trigger preview, then resets.
"""
import json
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def main():
    exe = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target" / "debug" / "controller.exe"
    process = subprocess.Popen([str(exe), "--core"], cwd=ROOT, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    messages = queue.Queue()

    def reader():
        for line in process.stdout:
            try:
                messages.put(json.loads(line))
            except json.JSONDecodeError:
                messages.put({"type": "malformed", "line": line})

    threading.Thread(target=reader, daemon=True).start()
    seen = []
    try:
        until = time.monotonic() + 7
        while time.monotonic() < until:
            try:
                message = messages.get(timeout=0.5)
            except queue.Empty:
                continue
            seen.append(message)
            if message.get("type") == "device" and message.get("connected"):
                break
        assert any(m.get("type") == "device" and m.get("connected") for m in seen), "No live controller found"
        process.stdin.write(json.dumps({"action": "setGyroAim", "enabled": True,
                                        "sensitivity": 1, "yawScale": 1, "pitchScale": 1,
                                        "invertY": False, "activation": "l2"}) + "\n")
        process.stdin.flush()
        until = time.monotonic() + 4
        while time.monotonic() < until:
            try:
                message = messages.get(timeout=0.5)
            except queue.Empty:
                continue
            seen.append(message)
            if any(m.get("type") == "rate" and m.get("reportsPerSecond", 0) > 0 for m in seen) and any(m.get("type") == "gyroAim" and m.get("enabled") is True for m in seen):
                break
        process.stdin.write(json.dumps({"action": "setGyroAim", "enabled": False,
                                        "sensitivity": 1, "yawScale": 1, "pitchScale": 1,
                                        "invertY": False, "activation": "l2"}) + "\n")
        process.stdin.flush()
        until = time.monotonic() + 2
        while time.monotonic() < until and not any(m.get("type") == "gyroAim" and m.get("enabled") is False for m in seen):
            try:
                seen.append(messages.get(timeout=0.2))
            except queue.Empty:
                pass
        curve = [i / 10 for i in range(11)]
        process.stdin.write(json.dumps({"action": "setRemap", "enabled": False,
                                        "bindings": [{"button": 0, "vk": 65, "modifier": 4,
                                                      "behavior": "macro", "sequence": [66]}]}) + "\n")
        process.stdin.write(json.dumps({"action": "applyTriggers",
                                        "left": {"mode": "resistance", "strength": 10, "curve": curve},
                                        "right": {"mode": "bow", "strength": 10, "curve": curve}}) + "\n")
        process.stdin.write('{"action":"renewTriggers"}\n')
        process.stdin.write('{"action":"reset"}\n')
        process.stdin.flush()
        until = time.monotonic() + 3
        while time.monotonic() < until and not (any(m.get("type") == "safetyReset" for m in seen)
                                           and any(m.get("type") == "notice" and m.get("message") == "Effects reset" for m in seen)):
            try:
                seen.append(messages.get(timeout=0.2))
            except queue.Empty:
                pass
        rates = [m for m in seen if m.get("type") == "rate" and m.get("reportsPerSecond", 0) > 0]
        assert rates, "No valid input reports"
        assert all("sequenceGaps" in m and "invalidReports" in m for m in rates)
        assert any(m.get("type") == "gyroAim" and m.get("enabled") is True for m in seen)
        assert any(m.get("type") == "gyroAim" and m.get("enabled") is False for m in seen)
        assert any(m.get("type") == "remap" and m.get("enabled") is False for m in seen)
        assert any(m.get("type") == "triggerLease" and m.get("active") is True for m in seen)
        assert any(m.get("type") == "safetyReset" for m in seen)
        assert any(m.get("type") == "triggerLease" and m.get("active") is False for m in seen)
        assert not any(m.get("type") == "error" for m in seen), [m for m in seen if m.get("type") == "error"]
        print("Core smoke passed:", rates[-1]["reportsPerSecond"], "reports/s;",
              rates[-1]["sequenceGaps"], "gaps;", rates[-1]["invalidReports"], "rejected")
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.terminate()


if __name__ == "__main__":
    main()
