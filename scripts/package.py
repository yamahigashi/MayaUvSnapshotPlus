from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import tempfile
import urllib.request
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MODULE_DIR = REPO_ROOT / "build" / "MayaUvSnapshotPlus"
DEFAULT_DIST_DIR = REPO_ROOT / "dist"
DEFAULT_ZIP_BASENAME = "mayauvsnapshotplus"
MODULE_FILE = REPO_ROOT / "MayaUvSnapshotPlus.mod"
USER_SETUP_FILE = REPO_ROOT / "python" / "userSetup.py"
README_FILE = REPO_ROOT / "README.md"
LICENSE_FILE = REPO_ROOT / "LICENSE"
PYTHON_INSTALLER_CACHE_DIR = REPO_ROOT / "build" / "python-installers"
COMMON_BUILD_FEATURE = "maya-abi3-py39"
WINDOWS_PYTHON_PATCH_VERSIONS = {
    "3.7": "3.7.9",
    # Python 3.9.13 is the last 3.9 release with Windows binary installers.
    "3.9": "3.9.13",
}
VERSION_LINE_PATTERN = re.compile(r"^\+\s+MAYAVERSION:(\d+)\s+PLATFORM:win64\s+\S+\s+\S+\s+(.+)$")


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
        default=None,
        help="Output zip filename. Defaults to mayauvsnapshotplus_<tag>.zip when a git tag is available.",
    )
    parser.add_argument(
        "--skip-zip",
        action="store_true",
        help="Assemble the module directory without creating a zip archive.",
    )
    parser.add_argument(
        "--installer",
        choices=("auto", "pip", "uv"),
        default="auto",
        help="Installer frontend to use for packaging. 'auto' tries pip first, then uv.",
    )
    parser.add_argument(
        "--python-executable",
        type=Path,
        default=None,
        help=(
            "Python interpreter for the shared 2023+ build. "
            "Defaults to MAYA_UV_SNAPSHOT_PYTHON, --python-version resolution, or the current interpreter."
        ),
    )
    parser.add_argument(
        "--python-version",
        default="3.9",
        help="Python version to resolve for the shared 2023+ build. Defaults to 3.9.",
    )
    parser.add_argument(
        "--maya2022-python-executable",
        type=Path,
        default=None,
        help=(
            "Python interpreter for the Maya 2022 build. "
            "Defaults to MAYA_UV_SNAPSHOT_MAYA2022_PYTHON or --maya2022-python-version resolution."
        ),
    )
    parser.add_argument(
        "--maya2022-python-version",
        default="3.7",
        help="Python version to resolve for the Maya 2022 build. Defaults to 3.7.",
    )
    parser.add_argument(
        "--download-missing-python",
        action="store_true",
        help="Download and install requested Python versions when they are not found.",
    )
    return parser.parse_args()


def resolve_python_from_py_launcher(version: str) -> Path | None:
    if os.name != "nt":
        return None

    py_exe = shutil.which("py")
    if py_exe is None:
        return None

    try:
        result = subprocess.run(
            [py_exe, f"-{version}", "-c", "import sys; print(sys.executable)"],
            capture_output=True,
            text=True,
            check=True,
        )
    except subprocess.CalledProcessError:
        return None

    resolved = result.stdout.strip()
    if not resolved:
        return None
    return Path(resolved)


def resolve_python_from_path(version: str) -> Path | None:
    candidates = [
        f"python{version}",
        f"python{version.replace('.', '')}",
    ]
    if os.name == "nt":
        candidates.extend(
            [
                f"python{version}.exe",
                f"python{version.replace('.', '')}.exe",
            ]
        )

    for candidate in candidates:
        resolved = shutil.which(candidate)
        if resolved:
            return Path(resolved)
    return None


def install_windows_python(version: str) -> Path:
    if os.name != "nt":
        raise FileNotFoundError(
            f"Python {version} was not found and automatic installation is only supported on Windows."
        )

    patch_version = WINDOWS_PYTHON_PATCH_VERSIONS.get(version)
    if patch_version is None:
        raise FileNotFoundError(
            f"Automatic installation is not configured for Python {version}. "
            "Pass an explicit interpreter or install it first."
        )

    install_dir = Path.home() / f"python{version.replace('.', '')}"
    python_executable = install_dir / "python.exe"
    if python_executable.exists():
        return python_executable

    PYTHON_INSTALLER_CACHE_DIR.mkdir(parents=True, exist_ok=True)
    installer_path = PYTHON_INSTALLER_CACHE_DIR / f"python-{patch_version}-amd64.exe"
    installer_url = f"https://www.python.org/ftp/python/{patch_version}/python-{patch_version}-amd64.exe"

    if not installer_path.exists():
        print(f"Downloading Python {patch_version} from {installer_url}")
        urllib.request.urlretrieve(installer_url, installer_path)

    print(f"Installing Python {patch_version} to {install_dir}")
    subprocess.run(
        [
            str(installer_path),
            "/quiet",
            "InstallAllUsers=0",
            "PrependPath=0",
            "Include_test=0",
            f"TargetDir={install_dir}",
        ],
        check=True,
    )

    if not python_executable.exists():
        raise FileNotFoundError(f"Python installer completed but interpreter was not found: {python_executable}")

    return python_executable


def resolve_requested_python(
    explicit_executable: Path | None,
    env_var: str,
    version: str,
    download_missing_python: bool,
) -> Path:
    if explicit_executable is not None:
        return explicit_executable.resolve()

    configured = os.environ.get(env_var)
    if configured:
        return Path(configured).resolve()

    resolved = resolve_python_from_py_launcher(version)
    if resolved is None:
        resolved = resolve_python_from_path(version)
    if resolved is None:
        if download_missing_python:
            resolved = install_windows_python(version)
        else:
            raise FileNotFoundError(
                f"Python interpreter for version {version} was not found. "
                "Pass an explicit interpreter, install it first, or use --download-missing-python."
            )
    return resolved.resolve()


def build_env(python_executable: Path) -> dict[str, str]:
    env = os.environ.copy()
    env["PYO3_PYTHON"] = str(python_executable)
    return env


def resolve_release_tag() -> str | None:
    commands = (
        ["git", "describe", "--tags", "--exact-match"],
        ["git", "describe", "--tags", "--abbrev=0"],
    )
    for command in commands:
        try:
            result = subprocess.run(
                command,
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=True,
            )
        except (subprocess.CalledProcessError, FileNotFoundError):
            continue

        tag = result.stdout.strip()
        if tag:
            return tag
    return None


def default_zip_name() -> str:
    tag = resolve_release_tag()
    if not tag:
        return f"{DEFAULT_ZIP_BASENAME}.zip"
    return f"{DEFAULT_ZIP_BASENAME}_{tag}.zip"


def module_root_from_platform_path(platform_path: str) -> str:
    normalized = platform_path.strip().replace("\\", "/")
    if normalized.startswith("./"):
        normalized = normalized[2:]
    suffix = "/platforms/"
    if suffix not in normalized:
        raise ValueError(f"Module path does not contain '/platforms/': {platform_path}")
    return normalized.split(suffix, 1)[0]


def parse_platform_paths() -> dict[int, str]:
    platform_paths: dict[int, str] = {}
    for line in MODULE_FILE.read_text(encoding="utf-8").splitlines():
        match = VERSION_LINE_PATTERN.match(line.strip())
        if not match:
            continue
        platform_paths[int(match.group(1))] = match.group(2).strip()

    if not platform_paths:
        raise RuntimeError(f"No MAYAVERSION platform entries found in {MODULE_FILE}")
    return platform_paths


def platform_scripts_dir(module_dir: Path, relative_platform_path: str) -> Path:
    return module_dir.parent.joinpath(relative_platform_path).resolve() / "scripts"


def pip_install_command(
    python_executable: Path,
    target_dir: Path,
    config_settings: list[str] | None = None,
) -> list[str]:
    command = [
        str(python_executable),
        "-m",
        "pip",
        "install",
        "--target",
        str(target_dir),
    ]
    for setting in config_settings or []:
        command.extend(["--config-settings", setting])
    command.append(".")
    return command


def uv_install_command(
    python_executable: Path,
    target_dir: Path,
    config_settings: list[str] | None = None,
) -> list[str]:
    uv_exe = shutil.which("uv")
    if uv_exe is None:
        raise FileNotFoundError("uv executable was not found on PATH")

    command = [
        uv_exe,
        "pip",
        "install",
        "--python",
        str(python_executable),
        "--target",
        str(target_dir),
    ]
    for setting in config_settings or []:
        command.extend(["-C", setting])
    command.append(".")
    return command


def run_install(
    target_dir: Path,
    python_executable: Path,
    installer: str,
    config_settings: list[str] | None = None,
) -> None:
    commands = []
    if installer == "pip":
        commands.append(pip_install_command(python_executable, target_dir, config_settings=config_settings))
    elif installer == "uv":
        commands.append(uv_install_command(python_executable, target_dir, config_settings=config_settings))
    else:
        commands.append(pip_install_command(python_executable, target_dir, config_settings=config_settings))
        commands.append(uv_install_command(python_executable, target_dir, config_settings=config_settings))

    last_error: Exception | None = None
    for command in commands:
        try:
            subprocess.run(
                command,
                cwd=REPO_ROOT,
                check=True,
                env=build_env(python_executable),
            )
            return
        except (subprocess.CalledProcessError, FileNotFoundError) as exc:
            last_error = exc

    if last_error is None:
        raise RuntimeError("No installer command was attempted")
    raise last_error


def copy_user_setup(scripts_dir: Path) -> None:
    scripts_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(USER_SETUP_FILE, scripts_dir / USER_SETUP_FILE.name)


def prune_scripts_dir(scripts_dir: Path) -> None:
    for path in scripts_dir.rglob("__pycache__"):
        if path.is_dir():
            shutil.rmtree(path)

    for pattern in ("*.dist-info", "*.egg-info"):
        for path in scripts_dir.glob(pattern):
            if path.is_dir():
                shutil.rmtree(path)


def install_scripts_tree(
    target_dir: Path,
    python_executable: Path,
    installer: str,
    config_settings: list[str] | None = None,
) -> None:
    if target_dir.exists():
        shutil.rmtree(target_dir)
    target_dir.mkdir(parents=True, exist_ok=True)

    run_install(target_dir, python_executable, installer, config_settings=config_settings)
    copy_user_setup(target_dir)
    prune_scripts_dir(target_dir)


def copy_tree_contents(source_dir: Path, target_dir: Path) -> None:
    if target_dir.exists():
        shutil.rmtree(target_dir)
    shutil.copytree(source_dir, target_dir)


def build_platform_scripts(
    module_dir: Path,
    platform_paths: dict[int, str],
    maya2022_python: Path,
    common_python: Path,
    installer: str,
) -> tuple[Path, list[Path]]:
    scripts_2022 = platform_scripts_dir(module_dir, platform_paths[2022])
    install_scripts_tree(scripts_2022, maya2022_python, installer)

    common_versions = sorted(version for version in platform_paths if version >= 2023)
    if not common_versions:
        return scripts_2022, []

    with tempfile.TemporaryDirectory(prefix="maya-uv-shared-", dir=module_dir.parent) as temp_dir:
        staging_scripts = Path(temp_dir) / "scripts"
        install_scripts_tree(
            staging_scripts,
            common_python,
            installer,
            config_settings=[f"build-args=--features {COMMON_BUILD_FEATURE}"],
        )

        shared_targets: list[Path] = []
        for version in common_versions:
            target_scripts = platform_scripts_dir(module_dir, platform_paths[version])
            copy_tree_contents(staging_scripts, target_scripts)
            shared_targets.append(target_scripts)

    return scripts_2022, shared_targets


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
        archive.write(README_FILE, README_FILE.name)
        archive.write(LICENSE_FILE, "LICENSE.md")

    return zip_path


def validate_module_layout(module_dir: Path, platform_paths: dict[int, str]) -> None:
    module_root = module_root_from_platform_path(platform_paths[2022])
    expected_root = module_dir.relative_to(module_dir.parent).as_posix()
    if module_root != expected_root:
        raise RuntimeError(
            f"Module file root '{module_root}' does not match package root '{expected_root}'."
        )

    for version, relative_path in sorted(platform_paths.items()):
        scripts_dir = platform_scripts_dir(module_dir, relative_path)
        if not scripts_dir.exists():
            raise RuntimeError(f"Missing scripts directory for Maya {version}: {scripts_dir}")


def main() -> int:
    args = parse_args()
    module_dir = args.module_dir.resolve()
    dist_dir = args.dist_dir.resolve()
    platform_paths = parse_platform_paths()

    maya2022_python = resolve_requested_python(
        args.maya2022_python_executable,
        "MAYA_UV_SNAPSHOT_MAYA2022_PYTHON",
        args.maya2022_python_version,
        args.download_missing_python,
    )
    common_python = resolve_requested_python(
        args.python_executable,
        "MAYA_UV_SNAPSHOT_PYTHON",
        args.python_version,
        args.download_missing_python,
    )

    if module_dir.exists():
        shutil.rmtree(module_dir)
    module_dir.mkdir(parents=True, exist_ok=True)

    scripts_2022, shared_targets = build_platform_scripts(
        module_dir,
        platform_paths,
        maya2022_python,
        common_python,
        args.installer,
    )
    validate_module_layout(module_dir, platform_paths)
    module_file = copy_module_file(module_dir.parent)

    print(f"Module directory: {module_dir}")
    print(f"Module file: {module_file}")
    print(f"Maya 2022 Python: {maya2022_python}")
    print(f"Shared Python: {common_python}")
    print(f"Installer: {args.installer}")
    print(f"Maya 2022 scripts: {scripts_2022}")
    for target in shared_targets:
        print(f"Shared scripts: {target}")

    if args.skip_zip:
        return 0

    zip_name = args.zip_name or default_zip_name()
    zip_path = build_zip(module_dir, dist_dir, zip_name)
    print(f"Zip archive: {zip_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
