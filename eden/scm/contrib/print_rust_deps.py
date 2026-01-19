#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Print potential Rust dependencies as buck rules for the current Rust project.

Naive implementation. Might produce inaccurate results.

Related: to remove unused deps, try: arc lint -e extra --take RUSTUNUSEDDEPS
"""

import glob
import itertools
import os
import pprint
import re
import sys
from typing import Dict, List, NoReturn, Pattern, Set

crate_re = re.compile(r"[ (](?:::)?([a-z0-9_]+)::\w")
target_rust_library_re = re.compile(
    r'rust_(?:python_)?library\(\s*name\s*=\s*"([a-z0-9_-]+)"', re.M
)
thirdparty_buck_re = re.compile(r'name\s*=\s"([a-z0-9_-]+)"')
flatten = itertools.chain.from_iterable


def glob_r(root: str, *patterns: str) -> List[str]:
    return list(
        flatten(glob.glob(os.path.join(root, p), recursive=True) for p in patterns)
    )


def scan_patterns(paths: List[str], pat: Pattern) -> Dict[str, Set[str]]:
    """return {matched, set(path)}"""
    matched = {}
    for path in paths:
        content = read_path(path)
        for name in pat.findall(content):
            if name not in matched:
                matched[name] = {path}
            else:
                matched[name].add(path)
    return matched


def find_root(*key_paths: str) -> str:
    path = os.path.realpath(".")
    while True:
        if all(os.path.exists(os.path.join(path, p)) for p in key_paths):
            return path
        next_path = os.path.dirname(path)
        if next_path == path:
            break
        path = next_path
    fatal(f"did not find a parent directory that contains {key_paths}")


def fatal(message: str) -> NoReturn:
    print(f"fatal: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_path(path: str) -> str:
    with open(path) as f:
        return f.read()


def main():
    rust_project_root = find_root("src", "BUCK")
    rust_srcs = glob_r(rust_project_root, "src/**/*.rs")
    crate_names = scan_patterns(rust_srcs, crate_re)

    # Some deps are 3rd party. Some are 1st party. Figure that out by scanning
    # through BUCK in the Sapling project.
    sapling_root = find_root("sapling", "saplingnative", "lib")
    target_paths = glob_r(sapling_root, "lib/**/BUCK", "saplingnative/**/BUCK")
    name_to_target_paths = scan_patterns(target_paths, target_rust_library_re)

    fbsource_root = find_root("third-party/rust/BUCK")
    third_party_names = scan_patterns(
        [os.path.join(fbsource_root, "third-party/rust/BUCK")], thirdparty_buck_re
    )

    rules = []
    ignored = []
    for name in crate_names:
        alt_name = name.replace("_", "-")
        if alt_name in name_to_target_paths or alt_name in third_party_names:
            name = alt_name
        target_path = name_to_target_paths.get(name)
        rule = None
        if target_path:
            relative_dir = os.path.dirname(next(iter(target_path))[len(sapling_root) :])
            rule = f"//eden/scm{relative_dir}:{name}"
        elif name in third_party_names:
            rule = f"fbsource//third-party/rust:{name}"
        if rule:
            rules.append(rule)
        else:
            ignored.append(name)

    if ignored:
        print(f"ignored: {ignored}", file=sys.stderr)

    pprint.pprint(sorted(rules))


if __name__ == "__main__":
    main()
