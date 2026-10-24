"""Check that release metadata points to the signed installer."""

import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from build_update_manifest import create_manifest  # noqa: E402


def main() -> None:
    version = json.loads((ROOT / "tauri.conf.json").read_text(encoding="utf-8"))["version"]
    major, minor, patch = map(int, version.split("."))
    wrong_version = f"{major}.{minor}.{patch + 1}"
    with tempfile.TemporaryDirectory() as folder:
        bundle = Path(folder)
        installer = bundle / f"CONTROLLER_{version}_x64-setup.exe"
        installer.write_bytes(b"installer")
        (bundle / (installer.name + ".sig")).write_text("signed-data\n", encoding="utf-8")
        manifest = create_manifest(version, bundle, "rorohecker/Controller")
        target = manifest["platforms"]["windows-x86_64"]
        assert target["signature"] == "signed-data"
        assert target["url"].endswith(f"/v{version}/{installer.name}")
        try:
            create_manifest(wrong_version, bundle, "rorohecker/Controller")
        except ValueError as error:
            assert "version" in str(error)
        else:
            raise AssertionError("mismatched installer version was accepted")
        (bundle / (installer.name + ".sig")).unlink()
        try:
            create_manifest(version, bundle, "rorohecker/Controller")
        except ValueError as error:
            assert "signature" in str(error)
        else:
            raise AssertionError("unsigned release was accepted")
    print("Update manifest smoke passed")


if __name__ == "__main__":
    main()
