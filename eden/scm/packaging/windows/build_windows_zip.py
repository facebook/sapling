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


import glob
import io
import os
import subprocess
import time
import zipfile
from contextlib import contextmanager
from pathlib import Path
from urllib import request

PY_VERSION = "3.9.13"


def main():
    # This is typically .../eden/scm
    project_root = (Path(__file__).parent / ".." / "..").resolve()

    build_dir = (project_root / "build").resolve()

    # eden/scm/build/python - where we extract the embedded Python distribution and the Python NuPkg
    python_dir = build_dir / "python"

    fetch_python(python_dir)

    build_sapling(project_root, python_dir)

    zip_sapling(project_root, build_dir)


def build_sapling(project_root: Path, python_dir: Path):
    env = os.environ.copy()

    env["SAPLING_OSS_BUILD"] = "1"
    env["HGNAME"] = "sl"

    # TODO(T132168309): generate an appropriate version string

    # By default setup.py packages up the Python runtime it is invoked
    # with, so use our embedded python.exe.
    pythonexe = str(python_dir / "python.exe")

    with step("Building Sapling"):
        subprocess.check_call(
            [
                vcvarsbat(),
                "amd64",
                "&&",
                pythonexe,
                "setup.py",
                "build_interactive_smartlog",
                "build_py",
                "-c",
                "build_clib",
                "build_ext",
                "build_rust_ext",
                "--long-paths-support",
                "build_embedded",
            ],
            env=env,
            cwd=project_root,
        )


def zip_sapling(project_root: Path, build_dir: Path):
    # This is where "setup.py build_embedded" puts stuff.
    embedded_dir = build_dir / "embedded"

    artifacts_dir = project_root / "artifacts"
    artifacts_dir.mkdir(exist_ok=True)
    zipfile_path = artifacts_dir / f"sapling_windows_amd64.zip"

    with step(f"Zipping into {zipfile_path}"):
        with zipfile.ZipFile(zipfile_path, "w", zipfile.ZIP_DEFLATED) as z:
            for root, _dirs, files in os.walk(embedded_dir):
                for f in files:
                    source_file = os.path.join(root, f)
                    z.write(
                        source_file,
                        os.path.join(
                            "Sapling", os.path.relpath(source_file, embedded_dir)
                        ),
                    )


def fetch_python(python_dir: Path):
    python_dir.mkdir(parents=True, exist_ok=True)

    # The embedded Python distribution contains the runtime (we ship this with our code).
    with step("Fetching Embeddable Python"):
        # Python zip is ~6MB.
        embedded_python_url = f"https://www.python.org/ftp/python/{PY_VERSION}/python-{PY_VERSION}-embed-amd64.zip"
        with zipfile.ZipFile(
            io.BytesIO(request.urlopen(embedded_python_url).read()), "r"
        ) as py_zip:
            py_zip.extractall(path=python_dir)

    with step("Fetching Curses"):
        # For example, short_python_version = 39
        short_py_version = "".join(PY_VERSION.split(".")[:2])
        curses_distro_url = f"https://files.pythonhosted.org/packages/63/57/5ed9bfbbcbb9c34cdc5f578a57a087200fd09c70b30d78236e4deacf48b0/windows_curses-2.3.1-cp{short_py_version}-cp{short_py_version}-win_amd64.whl"
        with zipfile.ZipFile(
            io.BytesIO(request.urlopen(curses_distro_url).read()), "r"
        ) as py_zip:
            for info in py_zip.infolist():
                if info.filename.endswith(".pyd"):
                    py_zip.extract(info, python_dir)

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


def vcvarsbat() -> str:
    vcvarsall_paths = glob.glob(
        os.path.join(
            os.environ["ProgramFiles(x86)"],
            "Microsoft Visual Studio",
            "201[79]",
            "*",
            "VC",
            "Auxiliary",
            "Build",
            "vcvarsall.bat",
        )
    ) + glob.glob(
        os.path.join(
            os.environ["ProgramFiles"],
            "Microsoft Visual Studio",
            "2022",
            "*",
            "VC",
            "Auxiliary",
            "Build",
            "vcvarsall.bat",
        )
    )

    if not vcvarsall_paths:
        raise RuntimeError("couldn't find vcvarsall.bat")

    return vcvarsall_paths[0]


@contextmanager
def step(name):
    print(f"{name}... ", end="")
    start = time.time()
    yield
    elapsed = round(time.time() - start, 1)
    print(f"Done! ({elapsed}s)")


main()
