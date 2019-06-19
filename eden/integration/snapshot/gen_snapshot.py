#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import logging
import sys
import time
from pathlib import Path

from eden.integration.lib.find_executables import FindExe

from . import snapshot as snapshot_mod


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "-l",
        "--list",
        action="store_true",
        help="List all known snapshot generator types.",
    )
    ap.add_argument("-o", "--output", help="The output file path.")
    ap.add_argument(
        "name",
        nargs="?",
        help="The name of the snapshot generator to run.  Use --list to list the "
        "available generators.",
    )
    args = ap.parse_args()

    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s")

    if args.list:
        print("Available generators:")
        for name, snapshot_class in snapshot_mod.snapshot_types.items():
            print(f"  {name}: {snapshot_class.DESCRIPTION}")
        return 0

    if args.name is None:
        ap.error("must specify a snapshot type or --list")
        return 1

    snapshot_type = snapshot_mod.snapshot_types.get(args.name)
    if snapshot_type is None:
        ap.error(
            f'unknown snapshot type "{args.name}".  '
            "Use --list to see a list of available generators."
        )
        return 1

    if args.output is not None:
        output_path = Path(args.output)
    else:
        date_stamp = time.strftime("%Y%m%d")
        base_name = f"{args.name}-{date_stamp}.tar.xz"
        output_path = Path(
            FindExe.REPO_ROOT, "eden", "test-data", "snapshots", base_name
        )

    logging.info(f'Running "{args.name}" snapshot generator')

    with snapshot_mod.generate(snapshot_type) as snapshot:
        snapshot.create_tarball(output_path)

    logging.info(f"Successfully generated {output_path}")

    return 0


if __name__ == "__main__":
    rc = main()
    sys.exit(rc)
