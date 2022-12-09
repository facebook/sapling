#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from bindings import tracing
idtopath = {}

def getidtopath():
    """Return a dict mapping from id (in hex form) to path"""
    output = sheval('hg debugmanifestdirs -r "all()"')
    # debugmanifestdirs prints "<id> <path>" per line
    result = dict(l.split() for l in output.splitlines())
    return result

def collectprefetch(command):
    """Updating to commit, check prefetched paths"""
    d = tracing.tracingdata()

    with d:
        sheval(f"EDENSCM_LOG=manifest_tree=debug {command} 2>/dev/null")

    ids = []
    for spans in d.treespans().values():
        for span in spans.flatten():
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

  $ setconfig experimental.copytrace=off copytrace.fastcopytrace=true perftweaks.disablecasecheck=true
  $ enable sparse treemanifest rebase copytrace

  $ newrepo
  $ drawdag << 'EOS'
  > B  # B/x/x/y/z=B1
  > |  # B/y/x/y/z=B2
  > |
  > |
  > | D # D/d/d/d/d=D1
  > | |
  > | C # C/c/c/c/c=C1
  > |/
  > A  # A/x/x/y/z=A1
  >    # A/y/x/y/z=A2
  >    # A/z/x/y/z=A3
  > EOS

  $ hg sparse include x

# Good: Updating to A should avoid downloading y/ or z/

  >>> collectprefetch("hg update -q $A")
  ['x', 'x/x', 'x/x/y']


# Good: Updating to B should avoid downloading y/

  >>> collectprefetch("hg update -q $B")
  ['x', 'x/x', 'x/x/y']

  $ hg goto -q $D

# Good: Rebasing B to D should avoid downloading d/ or c/, or z/.
# (This is optimized by "rebase: use matcher to optimize manifestmerge",
#  https://www.mercurial-scm.org/repo/hg/rev/4d504e541d3d,
#  fbsource-hg: 94ad1b49ede1f8e5897c7c9381304785746fa460)

  >>> collectprefetch("hg rebase -r $B -d $D -q")
  ['x', 'x/x', 'x/x/y', 'y', 'y/x', 'y/x/y']

# Good: Changing sparse profile should not download everything.

  >>> collectprefetch("hg sparse exclude y")
  ['x', 'x/x', 'x/x/y']

# Test sparse profile change.

  $ newrepo
  $ drawdag << 'EOS'
  >     # B/profile=[include]\nx\ny
  > B   # B/x/x/x=2
  > |   # B/y/y/y=2
  > |   # B/z/z/z=2
  > |
  > A   # A/profile=[include]\nx
  >     # A/x/x/x=1
  >     # A/y/y/y=1
  >     # A/z/z/z=1
  > EOS

  >>> idtopath = getidtopath()
  >>> collectprefetch("hg sparse enable profile")
  []

# Good: Updating to A should avoid downloading y/ or z/

  >>> collectprefetch("hg update -q $A")
  ['x', 'x/x']

# Good: Updating to B should avoid downloading z/

  >>> collectprefetch("hg update -q $B")
  ['x', 'x/x', 'y', 'y/y']

# Test 'status'.

  $ newrepo
  $ drawdag << 'EOS'
  > A   # A/x/x/x=1
  >     # A/y/y/y=1
  >     # A/z/z/z=1
  > EOS

  >>> collectprefetch("hg sparse include x")
  []

  $ hg up -q $A

  $ printf 2 > y
  $ mkdir -p z
  $ printf 2 > z/1
  $ printf 2 > z/z

# Good: 'status' should avoid downloading y/ or z/.

  >>> sorted(set(collectprefetch("hg status")) - {"x", "x/x"})
  []

