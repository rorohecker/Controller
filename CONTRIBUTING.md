# Contributing

CONTROLLER is a Rust workspace (the app plus the pure `protocol/` crate) with a Tauri 2 shell and a plain HTML/CSS/JS interface in `ui/`. Windows is the tested platform; macOS and Linux are previews. Read [platform support](docs/platform-support.md) and the [hardware test checklist](docs/hardware-test-checklist.md) before changing anything that touches a controller: a passing build is not hardware verification, and hardware-visible features that have not been tried on a pad must be labelled unverified in code comments and docs.

## Prerequisites

- Rust: `rust-toolchain.toml` pins the exact stable toolchain (with `rustfmt` and `clippy`); rustup installs it on first use. `Cargo.toml` declares the minimum supported version in `rust-version`.
- Python 3.12 for the test runners and scripts.
- Tauri CLI 2.11.1 for bundles only (not needed for `cargo run`/`cargo test`): `cargo install tauri-cli --version 2.11.1 --locked`. On Windows `scripts/install_tauri_cli.ps1` installs a verified copy.

### Windows

Windows 10 or 11, the MSVC build tools and WebView2. Then:

```powershell
cargo run --release --bin controller
```

### Linux (Ubuntu 22.04 names)

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
  libasound2-dev libudev-dev libxkbcommon-dev libpipewire-0.3-dev libdbus-1-dev libclang-dev
cargo build --bin controller
```

For hardware access install `resources/70-controller-dualsense.rules` (hidraw) and, for the virtual pad, `resources/71-controller-uinput.rules` plus the `uinput` module. Bundles use `cargo tauri build --bundles deb,appimage --config tauri.linux.conf.json`.

### macOS

Xcode command line tools. Bundles use `cargo tauri build --bundles dmg --config tauri.macos.conf.json`; CI builds a universal binary and creates `icons/icon.icns` from `icons/icon.png` (that file and `icons/App.iconset/` are generated and ignored). Desktop control needs Accessibility permission; screen colour needs Screen Recording; system audio capture needs macOS 14.6 or newer.

## Tests

Rust:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Python gates (run from the repository root):

```
python -m pip install -r tests/requirements.txt
python -m playwright install chromium        # on Linux add --with-deps
python tests/run_ui_checks.py                # browser/offline gate, no controller needed
cargo build --bin controller
python tests/run_core_checks.py              # real stdio core with HID discovery disabled
python tests/update_manifest_smoke.py
```

`tests/requirements.txt` pins Playwright (1.64.0, the version CI uses). Several browser tests look for Chrome at `C:\Program Files\Google\Chrome\Application\chrome.exe` and otherwise use the Chromium that `playwright install` downloads, so on macOS and Linux the second command is required. The root `app.js`, `index.html` and `style.css` are old mockups that are ignored; the app is built from `ui/`.

`tests/core_smoke.py` and `tests/feature_smoke.py` need a connected USB DualSense and are not run in CI.

## Formatting and lint policy

CI runs `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` on every pull request, so both must be clean; do not add blanket `#[allow]`s to silence Clippy, fix the code or justify a narrow allow in a comment. Line endings are LF (`.gitattributes`). JavaScript and CSS are not auto-formatted; follow the surrounding style.

## Pull requests

CI (`.github/workflows/portable-build.yml`) runs on pull requests and on pushes to `main`. It checks formatting and version consistency, then Clippy and tests on Windows, tests/Clippy/core checks/bundles on Ubuntu 22.04 and macOS, and the browser gate on Ubuntu. Dependabot proposes weekly GitHub Actions and Cargo updates.

## Version bump

Bump both files together, then verify:

1. `version` in `Cargo.toml` (`[package]`).
2. `version` in `tauri.conf.json`.
3. Run `python scripts/check_versions.py` (add `--tag vX.Y.Z` to check a tag).
4. Update the sidebar fallback in `ui/index.html` (`#app-version`) and the version headers in [COMPATIBILITY.md](COMPATIBILITY.md) and [docs/implementation-coverage.md](docs/implementation-coverage.md).
5. Commit, then tag `vX.Y.Z`. The release workflow refuses a tag that does not match the version, refuses to run without `TAURI_SIGNING_PRIVATE_KEY`, and publishes signed installers and the update feed. Tags containing `-` are published as pre-releases.

`protocol/Cargo.toml` has its own crate version and is not part of the app release version.

## Hardware validation

Physical results are tracked in [docs/hardware-test-checklist.md](docs/hardware-test-checklist.md), observed outcomes go in [COMPATIBILITY.md](COMPATIBILITY.md), and the backlog is in [docs/development-backlog.md](docs/development-backlog.md). Do not record a feature as verified from CI or a unit test. Automated checks must never write to a controller (Edge writes, firmware or profile slots included).

## Legacy "Aftertouch" identifiers

The product was renamed from Aftertouch to CONTROLLER, but some identifiers deliberately keep the old name so existing installs keep working: the bundle identifier `studio.aftertouch.desktop` (changing it would make an installed app and its updater see a different application), the `aftertouch-protocol` crate and `aftertouch-probe` binary names, the migration that copies `%APPDATA%\Aftertouch\profiles.json`, and the browser storage keys for lightbar colours, scripts and battery history. Do not rename them without a migration plan.

## Large files in history

`dist/CONTROLLER.exe` and its checksum used to be committed and are now untracked and ignored; older commits still contain those binaries. Rewriting history to drop them (for example with `git filter-repo`) is optional and has not been done. Build artifacts belong in GitHub Releases, not in the repository.
