"""Check that release metadata points to the signed installer."""

import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from build_update_manifest import create_manifest  # noqa: E402


def main() -> None:
    with tempfile.TemporaryDirectory() as folder:
        bundle = Path(folder)
        installer = bundle / "CONTROLLER_0.11.0_x64-setup.exe"
        installer.write_bytes(b"installer")
        (bundle / (installer.name + ".sig")).write_text("signed-data\n", encoding="utf-8")
        manifest = create_manifest("0.11.0", bundle, "rorohecker/Controller")
        target = manifest["platforms"]["windows-x86_64"]
        assert target["signature"] == "signed-data"
        assert target["url"].endswith("/v0.11.0/CONTROLLER_0.11.0_x64-setup.exe")
        try:
            create_manifest("0.12.0", bundle, "rorohecker/Controller")
        except ValueError as error:
            assert "version" in str(error)
        else:
            raise AssertionError("mismatched installer version was accepted")
        (bundle / (installer.name + ".sig")).unlink()
        try:
            create_manifest("0.11.0", bundle, "rorohecker/Controller")
        except ValueError as error:
            assert "signature" in str(error)
        else:
            raise AssertionError("unsigned release was accepted")
    print("Update manifest smoke passed")


if __name__ == "__main__":
    main()
