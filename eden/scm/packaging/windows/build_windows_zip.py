#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This script assumes rust and openssl have been installed.

# To install openssl via vcpkg locally:
#
# 1. git clone https://github.com/Microsoft/vcpkg.git
# 2. .\vcpkg\bootstrap-vcpkg.bat
# 3  .\vcpkg integrate install (or alternatively set VCPKG_ROOT)
# 4. .\vcpkg.exe install openssl:x64-windows-static-md

# For rust: `choco install rust`


import io
import subprocess
import sys
import time
import zipfile
from contextlib import contextmanager
from pathlib import Path
from urllib import request

PY_VERSION = "3.10.11"


def main():
    # This is typically .../eden/scm
    project_root = (Path(__file__).parent / ".." / "..").resolve()

    out_dir = (project_root / "out").resolve()

    # eden/scm/out/python - where we extract the embedded Python distribution and the Python NuPkg
    python_dir = out_dir / "python"

    fetch_python(python_dir)

    build_sapling(project_root, python_dir)

    zip_sapling(project_root, python_dir, out_dir)


def build_sapling(project_root: Path, python_dir: Path):
    pythonexe = str(python_dir / "python.exe")

    with step("Building Sapling"):
        subprocess.check_call(
            [
                pythonexe,
                "build.py",
                "--oss",
                "--with-python",
                pythonexe,
            ],
            cwd=project_root,
        )


def python_runtime_files(python_dir: Path):
    version_parts = PY_VERSION.split(".")
    python_lib = f"python{version_parts[0]}{version_parts[1]}"

    for path in python_dir.glob(f"{python_lib}.*"):
        if path.suffix.lower() in (".dll", ".zip"):
            yield path, path.name

    for path in python_dir.glob("*.pyd"):
        yield path, f"DLLs/{path.name}"

    for path in python_dir.glob("*.dll"):
        arcname = (
            path.name
            if path.name.startswith(("python", "vcruntime"))
            else f"DLLs/{path.name}"
        )
        yield path, arcname


def build_output_files(project_root: Path, out_dir: Path):
    yield out_dir / "sl.exe", "sl.exe"
    yield out_dir / "isl-dist.tar.xz", "isl-dist.tar.xz"
    yield project_root / "contrib" / "editmergeps.ps1", "contrib/editmergeps.ps1"
    yield project_root / "contrib" / "editmergeps.bat", "contrib/editmergeps.bat"
    pdb = out_dir / "sl.pdb"
    if pdb.exists():
        yield pdb, "sl.pdb"


def zip_sapling(project_root: Path, python_dir: Path, out_dir: Path):
    zipfile_path = out_dir / f"sapling_windows_amd64.zip"

    with step(f"Zipping into {zipfile_path}"):
        with zipfile.ZipFile(zipfile_path, "w", zipfile.ZIP_DEFLATED) as z:
            seen = set()
            for path, arcname in python_runtime_files(python_dir):
                if arcname in seen:
                    continue
                seen.add(arcname)
                z.write(path, f"Sapling/{arcname}")
            for path, arcname in build_output_files(project_root, out_dir):
                if arcname in seen:
                    continue
                seen.add(arcname)
                z.write(path, f"Sapling/{arcname}")


def fetch_python(python_dir: Path):
    python_dir.mkdir(parents=True, exist_ok=True)

    if sys.platform == "cygwin":
        print("WARNING: CYGWIN BUILD NO LONGER OFFICIALLY SUPPORTED")
    # The embedded Python distribution contains the runtime (we ship this with our code).
    with step("Fetching Embeddable Python"):
        # Python zip is ~6MB.
        embedded_python_url = f"https://www.python.org/ftp/python/{PY_VERSION}/python-{PY_VERSION}-embed-amd64.zip"
        with zipfile.ZipFile(
            io.BytesIO(request.urlopen(embedded_python_url).read()), "r"
        ) as py_zip:
            py_zip.extractall(path=python_dir)

    # The NuPkg contains a full Python install (including build dependencies such as headers and .lib files).
    # We need this for building, but don't need to ship it.
    with step("Fetching NuPkg Python"):
        # NuPkg zip is ~15MB.
        nupkg_url = f"https://globalcdn.nuget.org/packages/python.{PY_VERSION}.nupkg"
        with zipfile.ZipFile(
            io.BytesIO(request.urlopen(nupkg_url).read()), "r"
        ) as py_zip:
            for info in py_zip.infolist():
                if info.filename.startswith(
                    "tools/include/"
                ) or info.filename.startswith("tools/libs/"):
                    info.filename = info.filename[len("tools/") :]
                    py_zip.extract(info, python_dir)


@contextmanager
def step(name):
    print(f"{name}... ", end="")
    start = time.time()
    yield
    elapsed = round(time.time() - start, 1)
    print(f"Done! ({elapsed}s)")


main()
