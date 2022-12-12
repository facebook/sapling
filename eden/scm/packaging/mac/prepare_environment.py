#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
import re
import shutil
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

parser.add_argument(
    "-t",
    "--target",
    default=None,
    type=str,
    help="Compilation target (e.g. aarch64-apple-darwin)",
    required=True,
)

parser.add_argument(
    "-r",
    "--release-version",
    default=None,
    type=str,
    help="Version for Sapling",
    required=True,
)

parser.add_argument(
    "-d",
    "--dotdir",
    default="git",
    type=str,
    help="Dot directory of the current repo (e.g. .git, .hg, .sl)",
    required=False,
)

parser.add_argument(
    "-o",
    "--formula-out",
    default="sapling.rb",
    type=str,
    help="Location of the resultant filled in formula",
    required=False,
)


def run_cmd(cmd: List[str]) -> str:
    return subprocess.check_output(cmd).decode("utf-8").rstrip()


def get_bottle(bottle_name: str, bottle_hash: str, tmpdir: str):
    """Downloads a bottle from homebrew-core given a bottle name and a hash.

    The hash corresponds to the hash of some bottle (which can be specified in
    the bottle section of homebrew formulas).

    https://github.com/Homebrew/homebrew-core/blob/75bac0ef0c7a68d3607fc5d7e94ef417d93df138/Formula/python%403.11.rb#L14
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
        raise RuntimeError(f"Unable to get actual url when querying {auth_url}")
    cmd = [
        "curl",
        "--location",
        "--remote-time",
        url,
        "--output",
        os.path.join(tmpdir, f"{bottle_name}.bottle.tar.gz"),
    ]
    run_cmd(cmd)


def set_up_downloaded_crates(tmpdir):
    # Set Python crate
    brew_cmd = ["brew", "--cellar"]
    brew_location = run_cmd(brew_cmd)
    print(f"LOCATION IS {brew_location}")
    dylib_location = os.path.join(
        brew_location,
        "python@3.11/3.11.0/Frameworks/Python.framework/Versions/3.11/lib/libpython3.11.dylib",
    )
    run_cmd(
        [
            "tar",
            "-zxvf",
            os.path.join(tmpdir, "python@3.11.bottle.tar.gz"),
            "-C",
            tmpdir,
            "python@3.11/3.11.0/Frameworks/Python.framework/Versions/3.11/Python",
        ]
    )
    os.remove(dylib_location)
    shutil.copy(
        os.path.join(
            tmpdir,
            "python@3.11/3.11.0/Frameworks/Python.framework/Versions/3.11/Python",
        ),
        dylib_location,
    )
    # Set OpenSSL crate
    run_cmd(
        [
            "tar",
            "-zxvf",
            os.path.join(tmpdir, "openssl@1.1.bottle.tar.gz"),
            "-C",
            tmpdir,
        ]
    )


def create_repo_tarball(dotdir):
    run_cmd(
        [
            "tar",
            "--exclude",
            f".{dotdir}/**",
            "--exclude",
            "sapling.tar.gz",
            "-czf",
            "./sapling.tar.gz",
            ".",
        ]
    )


def fill_in_formula_template(target, version, tmpdir, filled_formula_dir):
    brew_formula_rb = os.path.join(os.path.dirname(__file__), "brew_formula.rb")
    with open(brew_formula_rb, "r") as f:
        formula = f.read()
    sha256 = run_cmd(["shasum", "-a", "256", "./sapling.tar.gz"]).split()[0]
    cachedir = run_cmd(["brew", "--cache"])
    formula = formula.replace(
        "%URL%",
        f"file://{os.path.join(os.path.abspath(os.getcwd()), 'sapling.tar.gz')}",
    )
    formula = formula.replace("%VERSION%", version)
    formula = formula.replace("%SHA256%", sha256)
    formula = formula.replace("%TMPDIR%", tmpdir)
    formula = formula.replace("%TARGET%", target)
    formula = formula.replace("%CACHEDIR%", cachedir)
    with open(filled_formula_dir, "w") as f:
        f.write(formula)


if __name__ == "__main__":
    args = parser.parse_args()

    if len(args.hash) != len(args.formula):
        print("Number of hashes and formulas to download must be the same")
        exit(1)

    if "python@3.11" not in args.formula or "openssl@1.1" not in args.formula:
        print("Must specify both python3.11 and openssl@1.1 bottles to download")
        exit(1)

    tmpdir = tempfile.mkdtemp()
    print(f"TMPDIR is {tmpdir}")

    for (name, hash) in zip(args.formula, args.hash):
        get_bottle(name, hash, tmpdir)
    set_up_downloaded_crates(tmpdir)

    create_repo_tarball(args.dotdir)
    fill_in_formula_template(
        args.target, args.release_version, tmpdir, args.formula_out
    )
