#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import os
from pathlib import Path


# Build script that is invoked by automation to produce the artifacts for the
# Interactive Smartlog CLI that we will distribute via DotSlash.

fbcode = os.path.join(os.getcwd(), "fbcode")


async def main():
    """build //eden/addons:isl and unzip it in $MSDK_OUTPUT_DIR."""
    output_dir = os.environ.get("MSDK_OUTPUT_DIR")
    if output_dir is None:
        raise Exception("$MSDK_OUTPUT_DIR was not set!")

    await run(
        [
            "buck",
            "build",
            "//eden/addons:isl",
            "--out",
            output_dir,
        ],
        cwd=fbcode,
    )


async def run(args, cwd=None):
    process = await asyncio.create_subprocess_exec(*args, cwd=cwd)
    code = await process.wait()
    if code:
        raise Exception(f"command failed: {args}")


if __name__ == "__main__":
    # asyncio.run() is not available until Python 3.7.
    loop = asyncio.get_event_loop()
    try:
        loop.run_until_complete(main())
    finally:
        loop.close()
