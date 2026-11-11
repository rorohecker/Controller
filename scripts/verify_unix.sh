#!/usr/bin/env bash
# Runs the same checks as the macOS / Linux CI jobs (.github/workflows/portable-build.yml) so a
# release can be verified on a real Mac or Linux box BEFORE pushing.
#
#   bash scripts/verify_unix.sh            # Rust checks + core smoke tests (about 10-20 min first time)
#   bash scripts/verify_unix.sh --ui       # also run the browser UI gate (needs python3 + Playwright)
#   bash scripts/verify_unix.sh --fast     # skip the slow x86_64-apple-darwin cross-check (macOS only)
#
# Every step runs even if an earlier one fails; a summary is printed at the end and the exit
# code is non-zero if anything failed. Copy the summary back so failures can be fixed.
set -u
cd "$(dirname "$0")/.."

WITH_UI=0; FAST=0
for arg in "$@"; do
  case "$arg" in
    --ui) WITH_UI=1 ;;
    --fast) FAST=1 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

OS="$(uname -s)"
PY="$(command -v python3 || command -v python || true)"
RESULTS=(); FAILED=0
export PYTHONIOENCODING=utf-8

step() {  # step "name" command...
  local name="$1"; shift
  echo; echo "=================== $name"; echo "\$ $*"
  local start=$SECONDS
  if "$@"; then RESULTS+=("PASS  ${name}  ($((SECONDS-start))s)")
  else RESULTS+=("FAIL  ${name}  ($((SECONDS-start))s)"); FAILED=1; fi
}
skip() { RESULTS+=("SKIP  $1  ($2)"); }

echo "Host: $OS $(uname -m)   rustc: $(rustc --version 2>/dev/null || echo 'NOT INSTALLED')"
command -v cargo >/dev/null || { echo "Install Rust first: https://rustup.rs (rust-toolchain.toml pins the version)"; exit 2; }

if [ "$OS" = "Linux" ]; then
  echo "Linux needs these packages (Debian/Ubuntu names; see portable-build.yml):"
  echo "  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libasound2-dev libudev-dev libxkbcommon-dev libpipewire-0.3-dev"
fi

[ -n "$PY" ] && step "version consistency" "$PY" scripts/check_versions.py || skip "version consistency" "no python"
step "cargo fmt --check"            cargo fmt --all -- --check
step "cargo check (workspace)"      cargo check --workspace --all-targets
step "cargo clippy -D warnings"     cargo clippy --workspace --all-targets -- -D warnings
step "cargo test"                   cargo test --workspace --all-targets

if [ "$OS" = "Darwin" ]; then
  if [ "$FAST" = 1 ]; then
    skip "x86_64-apple-darwin check+clippy" "--fast"
  else
    step "rustup add darwin targets" rustup target add aarch64-apple-darwin x86_64-apple-darwin
    step "check x86_64-apple-darwin" cargo check --workspace --target x86_64-apple-darwin
    step "clippy x86_64-apple-darwin" cargo clippy --workspace --all-targets --target x86_64-apple-darwin -- -D warnings
  fi
fi

if [ -n "$PY" ]; then
  step "build debug core"            cargo build --bin controller
  step "core smoke tests"            "$PY" tests/run_core_checks.py target/debug/controller
else
  skip "core smoke tests" "no python"
fi

if [ "$WITH_UI" = 1 ] && [ -n "$PY" ]; then
  step "pip install test deps"       "$PY" -m pip install -r tests/requirements.txt
  step "playwright chromium"         "$PY" -m playwright install chromium
  step "UI gate (browser smokes)"    "$PY" tests/run_ui_checks.py --jobs 1
else
  skip "UI gate" "pass --ui to run it"
fi

echo; echo "=================== SUMMARY ($OS $(uname -m))"
printf '%s\n' "${RESULTS[@]}"
[ "$FAILED" = 0 ] && echo "ALL CHECKS PASSED" || echo "SOME CHECKS FAILED - send the FAIL lines and their output back"
exit "$FAILED"
