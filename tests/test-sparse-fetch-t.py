# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os

from bindings import tracing
from testutil.autofix import eq
from testutil.dott import feature, sh, testtmp  # noqa: F401


os.environ["EDENSCM_TRACE_LEVEL"] = "trace"
idtopath = {}


def getidtopath():
    """Return a dict mapping from id (in hex form) to path"""
    output = sh.hg("debugmanifestdirs", "-rall()")
    # debugmanifestdirs prints "<id> <path>" per line
    result = dict(l.split() for l in output.splitlines())
    return result


def collectprefetch(command):
    """Updating to commit, check prefetched paths"""
    d = tracing.tracingdata()

    with d:
        (sh % command).output

    ids = []
    for span in d.treespans().values()[0]:
        name = span.get("name")
        if name == "tree::store::prefetch":
            ids += span["ids"].split()
        elif name == "tree::store::get":
            ids.append(span["id"])
    idtopath.update(getidtopath())
    paths = set(idtopath[id] for id in set(ids)) - {"/"}

    # Translate ids to paths
    return sorted(filter(None, paths))


# Use some production settings. They avoid expensive paths.
sh % "setconfig experimental.copytrace=off copytrace.fastcopytrace=true perftweaks.disablecasecheck=true"
sh % "enable sparse treemanifest rebase copytrace"

# flatcompat calls '.text()' which invalidates fast paths. So disable it.
sh % "setconfig treemanifest.flatcompat=0"

sh % "newrepo"
sh % "drawdag" << r"""
B  # B/x/x/y/z=B1
|  # B/y/x/y/z=B2
|
|
| D # D/d/d/d/d=D1
| |
| C # C/c/c/c/c=C1
|/
A  # A/x/x/y/z=A1
   # A/y/x/y/z=A2
   # A/z/x/y/z=A3
"""

sh % "hg sparse include x"

# Suboptimal: Updating to A should avoid downloading y/ or z/
eq(
    collectprefetch("hg update -q $A"),
    ["x", "x/x", "x/x/y", "y", "y/x", "y/x/y", "z", "z/x", "z/x/y"],
)

# Suboptimal: Updating to B should avoid downloading y/
eq(collectprefetch("hg update -q $B"), ["x", "x/x", "x/x/y", "y", "y/x", "y/x/y"])


sh % "hg update -q $D"


# Good: Rebasing B to D should avoid downloading d/ or c/, or z/.
# (This is optimized by "rebase: use matcher to optimize manifestmerge",
#  https://www.mercurial-scm.org/repo/hg/rev/4d504e541d3d,
#  fbsource-hg: 94ad1b49ede1f8e5897c7c9381304785746fa460)
eq(
    collectprefetch("hg rebase -r $B -d $D -q"),
    ["x", "x/x", "x/x/y", "y", "y/x", "y/x/y"],
)

# Suboptimal: Changing sparse profile should not download everything.
eq(
    collectprefetch("hg sparse exclude y"),
    [
        "c",
        "c/c",
        "c/c/c",
        "d",
        "d/d",
        "d/d/d",
        "x",
        "x/x",
        "x/x/y",
        "y",
        "y/x",
        "y/x/y",
        "z",
        "z/x",
        "z/x/y",
    ],
)


# Test sparse profile change.

sh % "newrepo"
sh % "drawdag" << r"""
    # B/profile=[include]\nx\ny
B   # B/x/x/x=2
|   # B/y/y/y=2
|   # B/z/z/z=2
|
A   # A/profile=[include]\nx
    # A/x/x/x=1
    # A/y/y/y=1
    # A/z/z/z=1
"""

idtopath = getidtopath()

eq(collectprefetch("hg sparse enable profile"), [])

# Suboptimal: Updating to A should avoid downloading y/ or z/
eq(collectprefetch("hg update -q $A"), ["x", "x/x", "y", "y/y", "z", "z/z"])

# Suboptimal: Updating to B should avoid downloading z/
eq(collectprefetch("hg update -q $B"), ["x", "x/x", "y", "y/y", "z", "z/z"])


# Test 'status'.

sh % "newrepo"
sh % "drawdag" << r"""
A   # A/x/x/x=1
    # A/y/y/y=1
    # A/z/z/z=1
"""

eq(collectprefetch("hg sparse include x"), [])
sh % "hg up -q $A"
open("y", "w").write("2")
os.mkdir("z")
open("z/1", "w").write("2")
open("z/z", "w").write("2")

# Good: 'status' should avoid downloading y/ or z/.
eq(collectprefetch("hg status"), ["x", "x/x"])
