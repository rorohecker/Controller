"""Check that release metadata points to the signed installer."""

import json
import sys
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from build_update_manifest import create_manifest  # noqa: E402
from clean_bundle_outputs import clean  # noqa: E402


def main() -> None:
    version = json.loads((ROOT / "tauri.conf.json").read_text(encoding="utf-8"))["version"]
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT, text=True))
    package = next(p for p in metadata["packages"] if p["name"] == "controller")
    assert package["version"] == version, "installer and Cargo versions differ"
    assert [t["name"] for t in package["targets"] if "bin" in t["kind"]] == ["controller"], "developer probes must not be bundled with the app"
    major, minor, patch = map(int, version.split('-')[0].split("."))
    wrong_version = f"{major}.{minor}.{patch + 1}"
    with tempfile.TemporaryDirectory() as folder:
        bundle = Path(folder)
        output=bundle/'target/release/bundle'; output.mkdir(parents=True)
        (output/'stale.sig').write_text('old');(bundle/'source.txt').write_text('keep')
        clean(bundle)
        assert not output.exists() and (bundle/'source.txt').read_text()=='keep'
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
        (bundle / f"CONTROLLER_{wrong_version}_x64-setup.exe").write_bytes(b"stale")
        assert create_manifest(version, bundle, "rorohecker/Controller") == manifest
        # A package for another architecture must never become the x64 update.
        (bundle / f"CONTROLLER_{version}_arm64-setup.exe").write_bytes(b"other architecture")
        assert create_manifest(version, bundle, "rorohecker/Controller") == manifest
        (bundle / (installer.name + ".sig")).unlink()
        try:
            create_manifest(version, bundle, "rorohecker/Controller")
        except ValueError as error:
            assert "signature" in str(error)
        else:
            raise AssertionError("unsigned release was accepted")
        (bundle / (installer.name + '.sig')).write_text('windows-signature', encoding='utf-8')
        try:
            create_manifest(version, bundle, 'rorohecker/Controller', require_all=True)
        except ValueError as error:
            assert 'requires signed' in str(error)
        else:
            raise AssertionError('incomplete portable feed accepted')
        for name in [f'CONTROLLER_{version}_amd64.AppImage', 'CONTROLLER.app.tar.gz']:
            (bundle/name).write_bytes(b'bundle')
            (bundle/(name+'.sig')).write_text('signature-'+name, encoding='utf-8')
        portable=create_manifest(version,bundle,'rorohecker/Controller',require_all=True)
        assert set(portable['platforms'])=={'windows-x86_64','linux-x86_64','darwin-aarch64','darwin-x86_64'}
        assert portable['platforms']['darwin-aarch64']==portable['platforms']['darwin-x86_64']
        assert portable['platforms']['linux-x86_64']['url'].endswith('.AppImage')
        (bundle/'CONTROLLER.app.tar.gz.sig').unlink()
        try:
            create_manifest(version,bundle,'rorohecker/Controller',require_all=True)
        except ValueError as error:
            assert 'signature' in str(error)
        else:
            raise AssertionError('unsigned Mac bundle accepted')
    print("Update manifest smoke passed")


if __name__ == "__main__":
    main()
