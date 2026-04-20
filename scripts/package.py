from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MODULE_DIR = REPO_ROOT / "build" / "MayaUvSnapshotPlus"
DEFAULT_DIST_DIR = REPO_ROOT / "dist"
DEFAULT_ZIP_NAME = "mayauvsnapshotplus.zip"
MODULE_FILE = REPO_ROOT / "MayaUvSnapshotEdgeDrawer.mod"
USER_SETUP_FILE = REPO_ROOT / "python" / "userSetup.py"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build a deployable Maya module directory and zip archive."
    )
    parser.add_argument(
        "--module-dir",
        type=Path,
        default=DEFAULT_MODULE_DIR,
        help="Directory where the Maya module contents will be assembled.",
    )
    parser.add_argument(
        "--dist-dir",
        type=Path,
        default=DEFAULT_DIST_DIR,
        help="Directory where the distributable zip file will be written.",
    )
    parser.add_argument(
        "--zip-name",
        default=DEFAULT_ZIP_NAME,
        help="Output zip filename.",
    )
    parser.add_argument(
        "--skip-zip",
        action="store_true",
        help="Assemble the module directory without creating a zip archive.",
    )
    return parser.parse_args()


def install_package(module_dir: Path) -> None:
    scripts_dir = module_dir / "scripts"
    scripts_dir.mkdir(parents=True, exist_ok=True)

    pip_cmd = [
        sys.executable,
        "-m",
        "pip",
        "install",
        "--target",
        str(scripts_dir),
        ".",
    ]

    try:
        subprocess.run(pip_cmd, cwd=REPO_ROOT, check=True)
    except subprocess.CalledProcessError:
        uv_exe = shutil.which("uv")
        if uv_exe is None:
            raise

        subprocess.run(
            [
                uv_exe,
                "pip",
                "install",
                "--python",
                sys.executable,
                "--target",
                str(scripts_dir),
                ".",
            ],
            cwd=REPO_ROOT,
            check=True,
        )
    copy_user_setup(scripts_dir)
    prune_scripts_dir(scripts_dir)


def copy_user_setup(scripts_dir: Path) -> Path:
    target = scripts_dir / USER_SETUP_FILE.name
    shutil.copy2(USER_SETUP_FILE, target)
    return target


def prune_scripts_dir(scripts_dir: Path) -> None:
    for path in scripts_dir.rglob("__pycache__"):
        if path.is_dir():
            shutil.rmtree(path)

    for pattern in ("*.dist-info", "*.egg-info"):
        for path in scripts_dir.glob(pattern):
            if path.is_dir():
                shutil.rmtree(path)


def copy_module_file(target_dir: Path) -> Path:
    target_dir.mkdir(parents=True, exist_ok=True)
    target = target_dir / MODULE_FILE.name
    shutil.copy2(MODULE_FILE, target)
    return target


def build_zip(module_dir: Path, dist_dir: Path, zip_name: str) -> Path:
    dist_dir.mkdir(parents=True, exist_ok=True)
    zip_path = dist_dir / zip_name
    if zip_path.exists():
        zip_path.unlink()

    with ZipFile(zip_path, "w", compression=ZIP_DEFLATED) as archive:
        for path in sorted(module_dir.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(module_dir.parent))

        archive.write(MODULE_FILE, MODULE_FILE.name)

    return zip_path


def main() -> int:
    args = parse_args()
    module_dir = args.module_dir.resolve()
    dist_dir = args.dist_dir.resolve()

    if module_dir.exists():
        shutil.rmtree(module_dir)

    install_package(module_dir)
    module_file = copy_module_file(module_dir.parent)

    print(f"Module directory: {module_dir}")
    print(f"Module file: {module_file}")

    if args.skip_zip:
        return 0

    zip_path = build_zip(module_dir, dist_dir, args.zip_name)
    print(f"Zip archive: {zip_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
