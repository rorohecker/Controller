"""Create a Tauri update feed from signed desktop bundles."""

import argparse
import json
import re
from pathlib import Path
from urllib.parse import quote


def signed_asset(artifact: Path, version: str, repository: str) -> dict:
    if not artifact.is_file() or artifact.stat().st_size == 0:
        raise ValueError(f'updater bundle is empty or missing: {artifact.name}')
    signature_file = artifact.with_name(artifact.name + '.sig')
    if not signature_file.is_file():
        raise ValueError(f'missing updater signature for {artifact.name}')
    signature = signature_file.read_text(encoding='utf-8').strip()
    if not signature:
        raise ValueError(f'updater signature is empty for {artifact.name}')
    return {'url': f'https://github.com/{repository}/releases/download/v{version}/{quote(artifact.name)}', 'signature': signature}


def create_manifest(version: str, bundle_dir: Path, repository: str, require_all: bool = False) -> dict:
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("version must be a semantic version without a leading v")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("repository must be owner/name")
    installers = list(bundle_dir.rglob(f"*_{version}_x64-setup.exe"))
    if len(installers) != 1:
        raise ValueError(f"expected one NSIS installer for version {version}, found {len(installers)}")
    platforms = {'windows-x86_64': signed_asset(installers[0], version, repository)}
    linux = [p for p in bundle_dir.rglob('*.AppImage') if f'_{version}_' in p.name and p.name.endswith(('_amd64.AppImage', '_x86_64.AppImage'))]
    mac = list(bundle_dir.rglob('*.app.tar.gz'))
    for name, artifacts in [('Linux x86_64', linux), ('universal macOS', mac)]:
        if len(artifacts) > 1:
            raise ValueError(f'ambiguous {name} updater artifacts')
    if linux:
        platforms['linux-x86_64'] = signed_asset(linux[0], version, repository)
    if mac:
        asset = signed_asset(mac[0], version, repository)
        platforms['darwin-aarch64'] = asset
        platforms['darwin-x86_64'] = asset.copy()
    if require_all and len(platforms) != 4:
        raise ValueError('release requires signed Windows x64, Linux x64 and universal macOS updater bundles')
    return {'version': version, 'platforms': platforms}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("bundle_dir", type=Path)
    parser.add_argument("repository")
    parser.add_argument('--require-all', action='store_true')
    args = parser.parse_args()
    manifest = create_manifest(args.version, args.bundle_dir, args.repository, args.require_all)
    output = args.bundle_dir / "latest.json"
    output.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(output)


if __name__ == "__main__":
    main()
