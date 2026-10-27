"""Create Tauri's static update manifest from a signed Windows NSIS bundle."""

import argparse
import json
import re
from pathlib import Path
from urllib.parse import quote


def create_manifest(version: str, bundle_dir: Path, repository: str) -> dict:
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("version must be a semantic version without a leading v")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("repository must be owner/name")
    installers = list(bundle_dir.glob(f"*_{version}_x64-setup.exe"))
    if len(installers) != 1:
        raise ValueError(f"expected one NSIS installer for version {version}, found {len(installers)}")
    installer = installers[0]
    signature_file = installer.with_name(installer.name + ".sig")
    if not signature_file.is_file():
        raise ValueError(f"missing updater signature for {installer.name}")
    signature = signature_file.read_text(encoding="utf-8").strip()
    if not signature:
        raise ValueError("updater signature is empty")
    url = f"https://github.com/{repository}/releases/download/v{version}/{quote(installer.name)}"
    return {
        "version": version,
        "platforms": {
            "windows-x86_64": {"url": url, "signature": signature}
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("bundle_dir", type=Path)
    parser.add_argument("repository")
    args = parser.parse_args()
    manifest = create_manifest(args.version, args.bundle_dir, args.repository)
    output = args.bundle_dir / "latest.json"
    output.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(output)


if __name__ == "__main__":
    main()
