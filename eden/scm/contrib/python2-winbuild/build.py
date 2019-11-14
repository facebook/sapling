# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Build python2 using Visual Studio 2017 toolchain

Required dependency:
- Python 3 (for fetching build dependencies).
- Windows SDK 10.0.17763.0 (provided by VS 2017 Installer).

Build result:
- Bin: Binary runtime with minimized `sys.path`.
    - python27.dll: The main runtime.
    - python27.zip: Pure stdlib.
    - python.exe: The interpreter. A thin wrapper of python27.dll.
    - _msi.pyd, _sqlite3.pyd: Windows-specific modules.
- Include: Header files for building native extensions.
- libs: Library files for building native extensions.
- DebugSymbols: Debug symbol files.

To add new patches:
- After building, work in the git repo `/cpython27-build`. Fetch,
  rebase, or cherry-pick changes.
- Use `git format-patch <parent-commit-hash-of-1st-patch>` to generate patch
  files.

To test whether it works (fb-only):
- Run `fb/packaging/build_nupkg.py build package --python <this-directory>`.
- Then check whether `build/embed/hg.exe` works or not.
- Run this before `fb/upload.py`.
"""

import glob
import os
import shutil
import subprocess
import sys
import zipfile


def git(dest, *args, **kwargs):
    fullargs = ["git"]
    if dest:
        fullargs += ["--git-dir=%s/.git" % dest, "--work-tree=%s" % dest]
    subprocess.check_call(fullargs + list(args), **kwargs)


def checkout(url, tag, oid, dest):
    if not os.path.isdir(dest):
        print("Creating %s" % dest)
        git(None, "init", dest)
    print("Fetching %s from %s" % (tag, url))
    git(dest, "fetch", "--depth=1", url, tag)
    print("Checking out %s" % tag)
    git(dest, "update-ref", "refs/heads/master", oid)
    git(dest, "reset", "--hard", "master")


def patch(patches, dest):
    for patch in sorted(glob.glob("*.patch")):
        print("Applying %s" % patch)
        git(dest, "am", stdin=open(patch))


def build(dest):
    print("Building (Requires Python 3 to fetch dependencies, and VS2017 Win SDK)")
    # See PCbuild/readme.txt for details.
    subprocess.check_call(
        " ".join(
            [
                "%s\\PCbuild\\build.bat" % dest,
                "-e",  # get external dependencies (ex. ssl)
                "-p x64",
                "-c Release",
                "--no-tkinter",
                "--no-bsddb",
                # 141: VS 2017
                '"/p:PlatformToolset=v141"',
                # Ideally, we don't specify an exact version like
                # 10.0.17763.0, and msbuild just uses the latest stable SDK.
                # However, at the time of writing, msbuild errors out if this
                # is not explicitly specified. Feel free to bump the SDK
                # version, or remove this flag (and the comment) if that works.
                #
                # See https://developercommunity.visualstudio.com/content/problem/140294/windowstargetplatformversion-makes-it-impossible-t.html
                '"/p:WindowsTargetPlatformVersion=10.0.17763.0"',
            ]
        ),
        shell=True,
    )


def mkdirp(path):
    if not os.path.isdir(path):
        os.makedirs(path)


def pack(src, dest):
    bindest = os.path.join(dest, "Bin")
    includedest = os.path.join(dest, "Include")
    libdest = os.path.join(dest, "libs")  # rust-cpython depends on this "libs" name.
    debugdest = os.path.join(dest, "DebugSymbols")
    for path in [bindest, includedest, libdest, debugdest]:
        mkdirp(path)

    amd64dir = os.path.join(src, "PCbuild", "amd64")
    print("Copying runtime binary files from %s to %s" % (amd64dir, bindest))
    for name in os.listdir(amd64dir):
        if name.endswith(".pyd") or name in {"python27.dll", "python.exe"}:
            shutil.copy(os.path.join(amd64dir, name), bindest)

    includedir = os.path.join(src, "Include")
    print("Copying development files to %s" % includedest)
    for name in os.listdir(includedir):
        shutil.copy(os.path.join(includedir, name), includedest)
    shutil.copy(os.path.join(src, "PC", "pyconfig.h"), includedest)

    print("Copying library files to %s" % libdest)
    for name in os.listdir(amd64dir):
        if name.endswith(".exp") or name.endswith(".lib"):
            shutil.copy(os.path.join(amd64dir, name), libdest)

    print("Copying debug files to %s" % debugdest)
    for name in os.listdir(amd64dir):
        if name.endswith(".pdb"):
            shutil.copy(os.path.join(amd64dir, name), debugdest)

    puredir = os.path.join(src, "Lib")
    zippath = os.path.join(bindest, "python27.zip")
    print("Packing pure modules from %s to %s" % (puredir, zippath))
    with zipfile.PyZipFile(zippath, "w") as z:
        z.writepy(puredir)
        for name in os.listdir(puredir):
            if name != "test" and os.path.exists(
                os.path.join(puredir, name, "__init__.py")
            ):
                z.writepy(os.path.join(puredir, name))
    print("%s\\python.exe is ready." % bindest)


def main():
    assert os.name == "nt", "This script only runs under Windows"
    # Python 2 is required so py_compile (used by PyZipFile) can understand Python 2 syntax
    assert sys.version_info.major == 2, "This script only runs under Python 2"
    # Avoids long paths that might be an issue.
    builddir = "C:\\cpython27-build"
    checkout(
        "https://github.com/python/cpython",
        # latest stable 2.7 (at time of writing)
        "v2.7.16",
        "413a49145e35c7a2d3a7de27c5a1828449c9b2e5",
        builddir,
    )
    patch(sorted(glob.glob("*.patch")), builddir)
    build(builddir)
    pack(builddir, "")


if __name__ == "__main__":
    main()
