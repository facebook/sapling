#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import atexit
import functools
import glob
import hashlib
import os
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
from typing import List

rm_rf = functools.partial(shutil.rmtree, ignore_errors=True)
print_err = functools.partial(print, file=sys.stderr)
glob_r = functools.partial(glob.glob, recursive=True)

# used to detect if files are changed.
def hash_path_contents(paths: List[str]):
    h = hashlib.sha1()
    sorted_paths = sorted(paths)
    for path in sorted_paths:
        try:
            with open(path, "rb") as f:
                h.update(f.read())
                h.update(b"\0")
        except IsADirectoryError:
            pass
    return h.hexdigest()


WALK_EXCLUDE_DIRS = ["node_modules", "build", "dist", "vscode-build", "coverage"]
WALK_EXCLUDE_EXTS = ".xz"

# find source code files (to hash_path_contents), excluding build results and node_modules
def walk_src_files(top: str):
    for root, dirs, files in os.walk(top):
        for exclude in WALK_EXCLUDE_DIRS:
            if exclude in dirs:
                dirs.remove(exclude)
        for name in files:
            if any(name.endswith(ext) for ext in WALK_EXCLUDE_EXTS):
                continue
            yield os.path.join(root, name)


def run(command: List[str], cwd=None, env=None):
    print_err(f"{cwd if cwd else ' '} $ {shlex.join(command)}")

    if env is not None:
        env = {**os.environ, **env}

    # shell=True with a List `command` seems buggy on *nix.
    # It might run ['sh', '-c', 'a', 'b'] instead of ['sh', '-c', 'a b'].
    subprocess.run(command, shell=(os.name == "nt"), check=True, cwd=cwd, env=env)


def realpath_args(args: List[str]) -> List[str]:
    return [os.path.realpath(arg) if os.path.exists(arg) else arg for arg in args]


def copy_writable(src, dst, *, follow_symlinks=True):
    """shutil.copy, but ensure that yarn.lock is writable
    - RE might make src/ read-only with its "restrictive mode".
    - When copying the RE "restrictive" src/, yarn.lock is read-only.
    - yarn wants yarn.lock to be writable, even with --frozen-lockfile.
    """
    shutil.copy(src, dst, follow_symlinks=follow_symlinks)
    if dst.endswith("yarn.lock") and os.name != "nt":
        os.chmod(dst, 0o666)


def main():
    parser = argparse.ArgumentParser(
        description="Creates a tarball of built ISL source."
    )
    parser.add_argument(
        "-o",
        "--output",
        nargs="?",
        default="isl-dist.tar.xz",
        help="Path to the output '.tar.xz' file.",
    )
    parser.add_argument(
        "--yarn",
        default="",
        help="Path to yarn executable.",
    )
    parser.add_argument(
        "--yarn-offline-mirror",
        default=None,
        help="Path to the yarn offline mirror.",
    )
    parser.add_argument(
        "--src",
        default=None,
        help="Directory that contains the source code.",
    )
    parser.add_argument(
        "--tmp",
        default=None,
        help="Temporary directory to run build. Do not modify src in-place.",
    )

    args = parser.parse_args()

    # posix=False prevents shlex.split from treating \\ as escape character, breaking Windows.
    yarn = realpath_args(
        shlex.split(args.yarn or os.getenv("YARN") or "yarn", posix=False)
    )

    src = args.src or "."
    out = args.output

    if args.tmp:
        # copy source to a temporary directory
        # used by buck genrule, which does not guarantee src is writable
        tmp_src_path = tempfile.mkdtemp(prefix="isl-src", dir=args.tmp)
        atexit.register(lambda: rm_rf(tmp_src_path))
        print_err(f"copying source {src} to {tmp_src_path}")
        shutil.copytree(
            src, tmp_src_path, dirs_exist_ok=True, copy_function=copy_writable
        )
        src = tmp_src_path

    src_join = functools.partial(os.path.join, src)

    source_hash = hash_path_contents(walk_src_files(src))
    try:
        with tarfile.open(out, "r") as tar:
            old_source_hash = tar.pax_headers.get("source_hash")
        if old_source_hash == source_hash:
            print_err(f"source not changed, skip rebuilding {out}")
            return
        else:
            print_err(
                f"source changed {old_source_hash[:8]} -> {source_hash[:8]}, rebuilding {out}"
            )
            os.unlink(out)
    except FileNotFoundError:
        pass
    except tarfile.ReadError:
        os.unlink(out)

    if args.yarn_offline_mirror:
        env = {"YARN_YARN_OFFLINE_MIRROR": os.path.realpath(args.yarn_offline_mirror)}
        run(
            yarn
            + [
                "--cwd",
                src_join(),
                "install",
                "--offline",
                "--frozen-lockfile",
                "--ignore-scripts",
                "--check-files",
            ],
            env=env,
        )
    else:
        run(yarn + ["--cwd", src_join(), "install", "--prefer-offline"])

    rm_rf(src_join("server/dist"))
    run(yarn + ["--cwd", src_join("isl-server"), "run", "build"], env={"CI": "false"})

    rm_rf(src_join("isl/build"))
    run(yarn + ["--cwd", src_join("isl"), "run", "build"], env={"CI": "false"})

    print_err(f"writing {out}")

    headers = {
        "source_hash": source_hash,
        "entry_point": "isl-server/dist/run-proxy.js",
    }

    with tarfile.open(
        out, "w:xz", pax_headers=headers, format=tarfile.PAX_FORMAT
    ) as tar:

        def add(path):
            tar.add(src_join(path), path)

        add("isl-server/dist")
        add("isl-server/node_modules/ws")
        add("isl/build")


if __name__ == "__main__":
    main()
