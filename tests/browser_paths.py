"""Cross-platform helpers shared by the browser smoke tests: Chrome lookup and a scratch directory."""
import os
from pathlib import Path
import shutil
import sys
import tempfile

_MISSING = Path(os.devnull + '-chrome-not-found')


def chrome_path():
    """A system Chrome/Chromium when one exists (CHROME_PATH wins), else a path that does not exist.

    Callers use `str(chrome) if chrome.exists() else None`, so a missing system browser falls back to
    Playwright's bundled Chromium (`playwright install chromium`), which is what Linux and macOS CI use.
    """
    override = os.environ.get('CHROME_PATH')
    if override and Path(override).exists():
        return Path(override)
    if sys.platform.startswith('win'):
        roots = [os.environ.get(k) for k in ('PROGRAMFILES', 'PROGRAMFILES(X86)', 'LOCALAPPDATA')]
        candidates = [Path(r) / 'Google' / 'Chrome' / 'Application' / 'chrome.exe' for r in roots if r]
    elif sys.platform == 'darwin':
        candidates = [Path('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')]
    else:
        candidates = [Path(p) for name in ('google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser') if (p := shutil.which(name))]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return _MISSING


def scratch_dir():
    """Directory for screenshots and other throwaway artefacts; created on demand on every OS."""
    path = Path(tempfile.gettempdir())
    path.mkdir(parents=True, exist_ok=True)
    return path


def target_dir():
    """`<repo>/target`, created on demand (a UI-only CI job may run before any cargo build)."""
    path = Path(__file__).resolve().parents[1] / 'target'
    path.mkdir(parents=True, exist_ok=True)
    return path


def core_exe(root):
    """Default debug core binary for the current OS."""
    return root / 'target' / 'debug' / ('controller.exe' if sys.platform == 'win32' else 'controller')
