#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import argparse
import datetime
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(
        description="Generate version/release information from latest hg changeset date"
    )
    parser.add_argument("--commit", help="Print commit hash")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--version", action="store_true", help="Print YYYYMMDD")
    group.add_argument("--release", action="store_true", help="Print HHMMSS")
    group.add_argument(
        "--dash-combined", action="store_true", help="Print YYYYMMDD-HHMMSS"
    )
    group.add_argument(
        "--dot-combined", action="store_true", help="Print YYYYMMDD.HHMMSS"
    )
    args = parser.parse_args()

    # Obtain changeset epoch seconds from hg
    rev_epoch = int(
        subprocess.check_output(
            [
                "hg",
                "log",
                "-l1",
                f"-r {args.commit}" if args.commit else "",
                "--template",
                "{date}",
            ],
            text=True,
        ).split(".", 1)[0]
    )

    dt = datetime.datetime.fromtimestamp(rev_epoch)
    version = dt.strftime("%Y%m%d")
    release = dt.strftime("%H%M%S")

    if args.version:
        print(version)
    elif args.release:
        print(release)
    elif args.dash_combined:
        print(f"{version}-{release}")
    elif args.dot_combined:
        print(f"{version}.{release}")

    sys.exit(0)


if __name__ == "__main__":
    main()
