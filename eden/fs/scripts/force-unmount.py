#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import shlex
import subprocess
import tempfile


def read_eden_mounts():
    with open("/proc/mounts") as f:
        for line in f:
            line = line.strip()
            mount_type, rest = line.split(maxsplit=1)
            mount_path, proto, flags, x, y = rest.rsplit(maxsplit=4)
            if mount_type.startswith("edenfs:"):
                yield mount_type, mount_path


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--all", action="store_true")
    group.add_argument("--tmp", action="store_true")
    args = parser.parse_args()

    tmp = tempfile.gettempdir()

    for _mount_type, mount_path in read_eden_mounts():
        if args.tmp and not mount_path.startswith(tmp):
            continue
        cmd = ["sudo", "umount", "-lf", mount_path]
        print(" ".join(map(shlex.quote, cmd)))
        if subprocess.call(cmd):
            print("umount failed")


if __name__ == "__main__":
    main()
