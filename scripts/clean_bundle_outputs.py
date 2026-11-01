"""Remove generated bundle outputs before CI packaging to exclude stale signatures."""
from pathlib import Path
import shutil


def clean(root: Path) -> None:
    root = root.resolve()
    target = (root / 'target').resolve()
    if not target.is_relative_to(root):
        raise ValueError('target directory resolves outside the checkout')
    paths = [root/'target/release/bundle', root/'target/universal-apple-darwin/release/bundle']
    resolved = [path.resolve() for path in paths]
    if any(not path.is_relative_to(target) or path == target for path in resolved):
        raise ValueError('bundle output resolves outside the target directory')
    for path in resolved:
        if path.exists():
            if not path.is_dir():
                raise ValueError('bundle output must be a directory')
            shutil.rmtree(path)


if __name__ == '__main__':
    clean(Path(__file__).resolve().parents[1])
