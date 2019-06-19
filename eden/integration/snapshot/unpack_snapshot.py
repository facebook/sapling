#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
from pathlib import Path

from . import snapshot as snapshot_mod


RUN_EDEN_SCRIPT = """\
#!/bin/bash

# Find the Eden binary to use.
DEV_EDEN="buck-out/gen/eden/cli/edenfsctl.par"
if [[ -n "$EDEN" ]]; then
  # $EDEN is defined in the environment, so use that
  :
elif [[ -x "$DEV_EDEN" ]]; then
  # We appear to be running from an fbcode directory with a built
  # version of eden in buck-out.  Use it.
  EDEN="$DEV_EDEN"
else
  # Find eden from $PATH
  EDEN="eden"
fi

SNAPSHOT_PATH={snapshot_path}
exec "$EDEN" \\
  --etc-eden-dir "$SNAPSHOT_PATH/transient/etc_eden" \\
  --config-dir "$SNAPSHOT_PATH/data/eden" \\
  "$@"
"""


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("-o", "--output", required=True, help="The output directory path.")
    ap.add_argument(
        "--start", action="store_true", default=False, help="Also start EdenFS."
    )
    ap.add_argument("snapshot", help="The path of the snapshot file to unpack.")

    args = ap.parse_args()

    output_dir = Path(args.output)
    input_path = Path(args.snapshot)
    output_dir.mkdir()
    snapshot_mod.unpack_into(input_path, output_dir)

    # Write a small helper script that can be used to more easily invoke Eden in this
    # repository.
    run_eden_path = output_dir / "run_eden"
    with run_eden_path.open("w") as f:
        f.write(RUN_EDEN_SCRIPT.format(snapshot_path=output_dir))
        os.fchmod(f.fileno(), 0o755)


if __name__ == "__main__":
    main()
