#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
import re
import subprocess
import tempfile
from typing import List


# Create the parser
parser = argparse.ArgumentParser(
    description="""Creates a homebrew bottle for the specified architecture

Also downloads additional brew bottles as required.
"""
)

parser.add_argument(
    "-s",
    "--hash",
    default=None,
    action="append",
    type=str,
    help="Hash of the bottle to be downloaded",
    required=True,
)

parser.add_argument(
    "-f",
    "--formula",
    default=None,
    action="append",
    type=str,
    help="Name of the bottle to be downloaded",
    required=True,
)


def run_cmd(cmd: List[str]) -> str:
    return subprocess.check_output(cmd).decode("utf-8").rstrip()


def get_bottle(bottle_name: str, bottle_hash: str, tmpdir: str):
    """Downloads a bottle from homebrew-core given a bottle name and a hash.

    The hash corresponds to the hash of some bottle (which can be specified in
    the bottle section of homebrew formulas).

    https://github.com/Homebrew/homebrew-core/blob/75bac0ef0c7a68d3607fc5d7e94ef417d93df138/Formula/python%403.8.rb#L14
    is an example of this.
    """
    auth_url = f"https://ghcr.io/v2/homebrew/core/{bottle_name.replace('@', '/')}/blobs/sha256:{bottle_hash}"
    auth_cmd = [
        "curl",
        "--header",
        "Authorization: Bearer QQ==",
        "--location",
        "--silent",
        "--head",
        "--request",
        "GET",
        auth_url,
    ]
    url = None
    for line in run_cmd(auth_cmd).split("\n"):
        if re.match("^location: ", line):
            url = line.split()[1]
            break
    if url is None:
        raise Exception(f"Unable to get actual url when querying {auth_url}")
    cmd = [
        "curl",
        "--location",
        "--remote-time",
        url,
        "--output",
        os.path.join(tmpdir, f"{bottle_name}.bottle.tar.gz"),
    ]
    run_cmd(cmd)


if __name__ == "__main__":
    args = parser.parse_args()

    if len(args.hash) != len(args.formula):
        print("Number of hashes and formulas to download must be the same")
        exit(1)

    if "python@3.8" not in args.formula or "openssl@1.1" not in args.formula:
        print("Must specify both python3.8 and openssl@1.1 bottles to download")
        exit(1)

    tmpdir = tempfile.mkdtemp()
    print(f"TMPDIR is {tmpdir}")

    for (name, hash) in zip(args.formula, args.hash):
        get_bottle(name, hash, tmpdir)
