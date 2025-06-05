#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Produce sl-manylinux.tar.xz that contains the sl binary and dependencies"""

import argparse
import os
import shutil
import subprocess
import tarfile
import tempfile

# Layout:
#
#   ./sl (main binary)
#   ./isl-dist.tar.xz (ISL javascript)
#   ./lib/python3.12/lib-dynload/ (Python native stdlib modules)
#     (see https://github.com/python/cpython/blob/v3.12.0/Modules/getpath.py#L599)


def parse_args():
    parser = argparse.ArgumentParser(
        description="Build sl-manylinux.tar.xz that contains the sl binary and dependencies"
    )
    parser.add_argument(
        "--python-prefix",
        help="Python prefix (ex. /opt/python/cp312-cp312 or /). Only useful if sl is statically linked to python.",
        default="/opt/python/cp312-cp312",
    )
    parser.add_argument(
        "--output-path",
        "-o",
        default="sl-manylinux.tar.xz",
        help="Path to tar.xz (or tar.gz) output",
    )
    parser.add_argument("--sl-path", help="Path to the sl binary (skip build)")
    parser.add_argument("--isl-path", help="Path to isl-dist.tar.xz (skip build)")
    return parser.parse_args()


def is_dynamic_linked_to_python(sl_path):
    ldd_out = subprocess.check_output(["ldd", sl_path]).decode("utf-8")
    return "libpython" in ldd_out


def get_python_version(python_exe_path):
    out = subprocess.check_output(
        [
            python_exe_path,
            "-c",
            "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')",
        ]
    )
    return out.decode("utf-8").strip()


def add_python_native_stdlib(python_prefix, tar):
    # ex. "3.12"
    version = get_python_version(os.path.join(python_prefix, "bin/python"))
    rel_python_native_lib_dir = f"lib/python{version}/lib-dynload"
    src_python_native_lib_dir = os.path.join(python_prefix, rel_python_native_lib_dir)
    if not os.path.isdir(src_python_native_lib_dir):
        raise RuntimeError(
            f"Missing native python library at {src_python_native_lib_dir}"
        )
    tar.add(src_python_native_lib_dir, arcname=rel_python_native_lib_dir)


def build_sl_and_isl(python_prefix):
    project_root = os.path.dirname(
        os.path.dirname(os.path.dirname(os.path.realpath(__file__)))
    )
    env = os.environ.copy()
    env["PYTHON_SYS_EXECUTABLE"] = os.path.join(python_prefix, "bin/python")
    subprocess.check_call(["make", "oss"], cwd=project_root, env=env)
    return os.path.join(project_root, "sl"), os.path.join(
        project_root, "isl-dist.tar.xz"
    )


def add_stripped_sl_binary(sl_path, tar):
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_sl_path = os.path.join(temp_dir, "sl")
        shutil.copy2(sl_path, temp_sl_path)
        subprocess.check_call(["strip", temp_sl_path])
        tar.add(temp_sl_path, arcname="sl")


def infer_compression_format(path):
    for ext in [".gz", ".xz", ".bz2"]:
        if path.endswith(ext):
            return ext[1:]
    return ""


def main():
    args = parse_args()
    python_prefix = args.python_prefix
    sl_path = args.sl_path
    isl_path = args.isl_path
    output_path = args.output_path
    if not sl_path or not isl_path:
        new_sl_path, new_isl_path = build_sl_and_isl(python_prefix)
        sl_path = sl_path or new_sl_path
        isl_path = isl_path or new_isl_path

    tmp_output_path = output_path + ".partial"
    with tarfile.open(
        tmp_output_path, f"w:{infer_compression_format(output_path)}"
    ) as tar:
        add_stripped_sl_binary(sl_path, tar)
        if not is_dynamic_linked_to_python(sl_path):
            # Pure Python modules are part of the binary (lib/python-modules).
            # Only add native modules. This is to make the main binary run on
            # other systems where the original python prefix doesn't exist.
            add_python_native_stdlib(python_prefix, tar)
        tar.add(isl_path, arcname="isl-dist.tar.xz")

    os.rename(tmp_output_path, output_path)


if __name__ == "__main__":
    main()
