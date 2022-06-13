#!hg debugpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
script to edit common feature headers of all tests in one place

    $0 [FEATURE...] [TEST...]

Edit supported features for all tests:

    $0

Edit specified features for all tests:

    $0 chg debugruntest

Edit features for selected tests:

    $0 chg test-a.t test-b.t
"""

import glob
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from typing import Dict, List


@dataclass
class Feature:
    name: str
    code: str


FEATURES = [
    Feature("chg", "#chg-compatible\n"),
    Feature("no-chg", "#chg-incompatible\n"),
    Feature("debugruntest", "#debugruntest-compatible\n"),
    Feature("no-treemanifest", "  $ disable treemanifest\n"),
    Feature(
        "no-narrowheads",
        "  $ setconfig experimental.narrow-heads=false\n",
    ),
    Feature(
        "no-segmented-changelog",
        "  $ setconfig format.use-segmented-changelog=true\n",
    ),
    Feature("no-ignore-revnum", "  $ setconfig ui.ignorerevnum=false\n"),
    Feature("py2", "#require py2\n"),
]


def scanfeatures(path: str, features: List[Feature]) -> List[str]:
    with open(path) as f:
        content = f.read()
    return [f.name for f in features if f.code in content]


def edit(featurepaths: Dict[str, List[str]]) -> Dict[str, List[str]]:
    codelines = []
    for name, paths in featurepaths.items():
        codelines.append(f"{name}:\n  ")
        codelines.append("\n  ".join(paths))
        codelines.append("\n\n")
    codelines.append("# vim: foldmethod=indent foldlevel=0\n")
    result = {}
    with tempfile.NamedTemporaryFile() as tmpfile:
        tmpfile.write("".join(codelines).encode())
        tmpfile.flush()
        editor = os.getenv("EDITOR") or "vim"
        subprocess.run([editor, tmpfile.name], check=True)
        # parse the edited content
        with open(tmpfile.name, "r") as f:
            feature = None
            for line in f:
                line = line.rstrip()
                if line.endswith(":"):
                    feature = line[:-1]
                elif line.startswith("  "):
                    path = line[2:]
                    if feature is not None:
                        fpaths = result.get(feature) or []
                        fpaths.append(path)
                        result[feature] = fpaths
    return result


def transpose(d: Dict[str, List[str]]) -> Dict[str, List[str]]:
    """{'a': ['b', 'c'], 'd': ['b']} => {'b': ['a', 'd'], 'c': ['a']}"""
    result = {}
    for name, values in d.items():
        for value in values:
            names = result.get(value) or []
            names.append(name)
            result[value] = names
    return result


def updatefile(path: str, wantedfeatures: List[str], checkedfeatures: List[Feature]):
    """update path so it has wantedfeatures but not other features in checkedfeatures"""
    with open(path) as f:
        code = f.read()

    existing = set(scanfeatures(path, checkedfeatures))
    wanted = set(wantedfeatures)
    for feature in checkedfeatures:
        if feature.name not in wanted and feature.name in existing:
            # remove it
            code = code.replace(feature.code, "", 1)
        if feature.name in wanted and feature.name not in existing:
            # add it - insert after existing '#' lines
            lines = code.splitlines(True)
            for i in range(0, len(lines)):
                if lines[i].startswith("#"):
                    continue
                if feature.code.startswith("  ") and not lines[i].strip():
                    continue
                break
            lines[i:i] = [feature.code]
            code = "".join(lines)
            break

    with open(path, "wb") as f:
        f.write(code.encode())


def main(args):
    features = FEATURES
    argset = set(args)
    if args:
        features = [h for h in features if h.name in argset]

    paths = glob.glob("test-*.t")
    if set(paths) & argset:
        paths = [p for p in paths if p in argset]

    pathfeatures: Dict[str, List[str]] = {p: scanfeatures(p, features) for p in paths}
    featurepaths: Dict[str, List[str]] = transpose(pathfeatures)

    # edit
    newfeaturepaths = edit(featurepaths)
    newpathfeatures = transpose(newfeaturepaths)

    for path, wantedfeatures in newpathfeatures.items():
        if wantedfeatures == pathfeatures.get(path):
            # nothing changed
            continue
        updatefile(path, wantedfeatures, features)


if __name__ == "__main__":
    main(sys.argv[1:])
