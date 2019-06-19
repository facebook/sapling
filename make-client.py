#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This script generates the `eden` CLI executable.
# We use zipapp to bundle this as a single executable file.
# This script looks a bit complicated because the layout of
# the python modules in the source tree doesn't match their
# runtime names; we therefore massage them into the installation
# image, and pull in a couple of third party dependencies from pypi.
import argparse
import os
import shutil
import subprocess
import tempfile
import zipapp
from pipes import quote as shellquote


# Where to find the eden OSS directory; it contains this script.
DEFAULT_OSS_DIR = os.path.abspath(os.path.dirname(__file__))

# third party deps to include in the executable
DEPS = ["future", "six", "toml"]

# Source path to destination python module name.
# The lhs of each tuple is the path in the eden tree where the
# python sources are found, and the rhs is the destination path
MODULES = [
    # The eden dirstate library
    ("eden/py", "eden"),
    # The cli
    ("eden/cli", "eden/cli"),
    # A helper for the eden thrift client
    ("eden/fs/service", "eden/thrift"),
]


def run_cmd(cmd, env=None, cwd=None):
    cmd_str = " ".join(shellquote(arg) for arg in cmd)
    env_extra = env or {}
    env = os.environ.copy()
    print(
        "+ "
        + " ".join(["%s=%s" % (k, shellquote(v)) for k, v in env_extra.items()])
        + " "
        + cmd_str
    )
    env.update(env_extra)
    subprocess.check_call(cmd, env=env, cwd=cwd)


def generate_thrift_code(thrift_compiler, oss_dir, gen_dir):
    """ Generate python thrift clients for a couple of things """
    thrift_files = [
        "eden/fs/config/eden_config.thrift",
        "eden/fs/service/eden.thrift",
        "common/fb303/if/fb303.thrift",
        "eden/fs/inodes/overlay/overlay.thrift",
    ]
    for t in thrift_files:
        run_cmd(
            [
                thrift_compiler,
                "-I",
                oss_dir,
                "-gen",
                "py:new_style",
                "-out",
                gen_dir,
                os.path.join(oss_dir, t),
            ]
        )


def copy_py(src_dir, instdir, dest_prefix):
    """ Workhorse for processing the mapping from source tree to
    installation image.  This function copies only python files
    from the source and places them under an alternative directory
    structure in the destination """
    for root, _dirs, files in os.walk(src_dir):
        rel_root = os.path.relpath(root, src_dir)
        for f in files:
            if f.endswith(".py"):
                dest_dir = os.path.join(instdir, dest_prefix)
                if rel_root != ".":
                    dest_dir = os.path.join(dest_dir, rel_root)
                src_file_name = os.path.join(root, f)
                dest_file_name = os.path.join(dest_dir, os.path.basename(f))
                os.makedirs(dest_dir, exist_ok=True)
                shutil.copyfile(src_file_name, dest_file_name)


def find_site_packages(instdir):
    """ locate any and all site-packages directories in the install image """
    sp = []
    for root, dirs, _files in os.walk(instdir):
        for d in dirs:
            if d == "site-packages":
                sp.append(os.path.join(root, d))
    return sp


def move_site_packages_to_root(instdir):
    """ To reduce pythonpath headaches, after install packages from pip we
    sweep them out of site-packages dirs and move them up to the root so
    that they are reachable by the entrypoint in the zipapp """
    for sp in find_site_packages(instdir):
        for child in os.listdir(sp):
            os.rename(os.path.join(sp, child), os.path.join(instdir, child))


parser = argparse.ArgumentParser()
parser.add_argument("python")
parser.add_argument("--oss_dir", default=DEFAULT_OSS_DIR)
parser.add_argument(
    "--thrift-compiler",
    default=os.path.join(DEFAULT_OSS_DIR, "external/install/bin/thrift1"),
)
parser.add_argument(
    "--thrift-py",
    default=os.path.join(DEFAULT_OSS_DIR, "external/fbthrift/thrift/lib/py"),
)
args = parser.parse_args()

with tempfile.TemporaryDirectory() as instdir:
    generate_thrift_code(args.thrift_compiler, args.oss_dir, instdir)

    for src_dir, dest_prefix in MODULES:
        copy_py(os.path.join(args.oss_dir, src_dir), instdir, dest_prefix)

    # And the python thrift runtime
    copy_py(args.thrift_py, instdir, "thrift")

    for dep in DEPS:
        # There's no supported way to call `pip` in process, so we just
        # have to shell out and install it where we want it.
        run_cmd([args.python, "-m", "pip", "install", dep, "--prefix", instdir])

    move_site_packages_to_root(instdir)
    # run_cmd(["find", instdir])

    # Generate the `eden` executable zipfile.
    # We keep the shebang a little more flexible than just sys.executable to
    # minimize headaches if the system python is upgraded; there are no guarantees
    # that we'll keep running, but it seems more likely that we will than if we
    # hard coded it.
    zipapp.create_archive(
        instdir, target="eden.zip", interpreter=args.python, main="eden.cli.main:main"
    )
