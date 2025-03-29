# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import itertools
import os
import subprocess
import sys

try:
    from sapling import bookmarks
    from sapling.node import hex, nullid
except ImportError:
    subprocess.run(["sl", "debugshell", __file__])
    sys.exit(0)


def yield_path_nodes(repo, main, n):
    c1, c2 = list(repo.set("%s + (%s~%z)", main, main, n))
    m1, m2 = c1.manifest(), c2.manifest()
    diff = m1.diff(m2)
    seen = set()
    for path, ((old_node, old_flags), (new_node, new_flags)) in diff.items():
        nodes = [n for n in (old_node, new_node) if n and n != nullid]
        if nodes and nodes[0] not in seen:
            seen.add(nodes[0])
            yield path, nodes[0]


def yield_tree_root_nodes(repo, main, n):
    # For simplicity, we just provide root trees.
    # Fetch 2x what we need in case there are duplicates.
    s = repo.revs("limit(reverse(::%s), %z)", main, 2 * n).prefetch("text")
    seen = set()
    for ctx in s.iterctx():
        mn = ctx.manifestnode()
        if mn not in seen:
            n -= 1
            seen.add(mn)
            yield mn
            if n == 0:
                break


def main(repo):
    n = int(os.getenv("N") or "20000")
    main = os.getenv("MAIN") or bookmarks.mainbookmark(repo)

    print(f"MAIN={main}\nN={n}", file=sys.stderr)

    out = "test-paths.txt"
    if not os.path.exists(out):
        diff = yield_path_nodes(repo, main, n)
        with open(out, "wb") as f:
            for path, node in itertools.islice(diff, n):
                f.write(("%s %s\n" % (hex(node), path)).encode())

        print(f"{out} written", file=sys.stderr)

    out = "test-trees.txt"
    if not os.path.exists(out):
        with open(out, "wb") as f:
            for node in yield_tree_root_nodes(repo, main, n):
                f.write(("%s\n" % hex(node)).encode())

        print(f"{out} written", file=sys.stderr)


main(repo)  # noqa
