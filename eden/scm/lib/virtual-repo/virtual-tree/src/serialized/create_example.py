# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Sapling script to generate the bytes used by Rust `virtual_tree`'s
`SerializedTree`. Run `sl dbsh THIS_SCRIPT` in a (small-ish) repo.

Requires:
- sl: to run this script via `sl dbsh`
- zstd: used for compression

Output will be written to `trees.zst`.
"""

from collections import defaultdict
from functools import partial
from subprocess import run

import bindings


def assign_id(name, mapping):
    id = mapping.get(name)
    if id is None:
        id = len(mapping) + 1
        mapping[name] = id
    return id


def vlq_array(array):
    return b"".join(map(bindings.vlq.encode, array))


def main(repo):
    tree_store = bindings.storemodel.TreeStore.from_store(repo.fileslog.filestore)

    get_tree_id = partial(assign_id, mapping={})  # (node) -> int
    trees = {}  # [int] -> {tree}

    # [path](name) -> int
    get_name_id_per_path = defaultdict(lambda: partial(assign_id, mapping={}))

    # [path](node) -> int
    get_blob_id_per_path = defaultdict(lambda: partial(assign_id, mapping={}))

    # (path) -> int
    get_id_per_path = partial(assign_id, mapping={})
    tree_id_to_path_id = {}

    def read_tree_recursive(tree_node, path):
        # Return tree_id
        # trees[tree_id] is set to {name_id: content_id}, full trees, no delta
        # content_id: tree_id * 4 | blob_generation * 4 + file_flag
        # file_flag: 1: regular; 2: executable; 3: symlink
        tree_id = get_tree_id(tree_node)
        path_id = get_id_per_path(path)
        tree_id_to_path_id[tree_id] = path_id
        if tree_id not in trees:
            entries = tree_store.get_local_tree("", tree_node)
            tree = {}
            trees[tree_id] = tree
            for name, node, kind in entries:
                # kind: {'file': 'regular' | 'executable' | 'symlink' | 'git_submodule'} | 'directory'
                name_id = get_name_id_per_path[path](name)
                match kind:
                    case "directory":
                        sub_tree_id = read_tree_recursive(node, "/".join((path, name)))
                        tree[name_id] = sub_tree_id * 4
                    case {"file": file_type}:
                        content_id = get_blob_id_per_path["/".join((path, name))](node)
                        match file_type:
                            case "executable":
                                flag = 2
                            case "symlink":
                                flag = 3
                            case _:
                                flag = 1
                        tree[name_id] = content_id * 4 + flag
                    case _:
                        raise ValueError(f"unsupported {kind=}")
        return tree_id

    # Commits in the main branch, linearized, old to new
    nodes = list(repo.dageval(lambda: firstancestors(list(public().take(1)))).reverse())  # noqa: F821

    # Pre-assign root tree ids so we don't need O(N) space to track what trees are root trees.
    # Root trees are just the first MAX_ROOT_TREE_ID items in the regular tree array.
    max_root_tree_id = 1
    for i, node in enumerate(nodes):
        tree_node = repo[node].manifestnode()
        max_root_tree_id = get_tree_id(tree_node)

    # Follow the commits, read all trees recursively.
    for i, node in enumerate(nodes):
        tree_node = repo[node].manifestnode()
        read_tree_recursive(tree_node, "")
        print(f"Commit {i:,}. Tree {len(trees):,}", end="\r")

    # Serialized the trees.
    # Each tree is serialized as: SEED, CONTENT[1], CONTENT[2], ...
    # where CONTENT[x] matches NAME[x] and CONTENT=0 implies absent/deleted.
    tree_bufs = []
    for tree_id in range(1, len(trees) + 1):
        tree = trees[tree_id]
        path_id = tree_id_to_path_id[tree_id]
        values = [tree.get(k) or 0 for k in range(1, max(tree) + 1)]
        buf = vlq_array([path_id] + values)
        tree_bufs.append(buf)

    # Uncompressed buffer: VLQ(TREE_LEN), VLQ(MAX_ROOT_TREE_ID), TREES.
    # Tree index starts from 1.
    all_tree_buf = (
        vlq_array([len(tree_bufs), max_root_tree_id])
        + vlq_array([len(buf) for buf in tree_bufs])
        + b"".join(tree_bufs)
    )

    # util.debugger()

    out_name = "trees"
    with open(out_name, "wb") as f:
        f.write(all_tree_buf)

    run(["zstd", "-19", out_name])


main(repo)  # noqa: F821

# It's tricky to maintain a balance of smaller size, good performance
# (de-serialization, random lookup). I tried some size reduction ideas,
# listed below, size numbers are in MB:
#
# +------------------------------------------------------------------------+
# | Compressed | Uncompressed | Adopted | Idea                             |
# +------------------------------------------------------------------------+
# | 2.3 -> 2.1 | 16.0 -> 12.2 | Yes     | Placeholder. Turn dict to list.  |
# |            |              |         | Store {1:x,3:y,4:z} as [x,0,y,z] |
# +------------------------------------------------------------------------+
# | 2.1 -> 2.9 | 12.7 ->  3.6 | No      | Delta-ed, not full trees.        |
# +------------------------------------------------------------------------+
# | 2.1 -> 2.0 | 12.7 -> 12.1 | No      | Cap file content id by `& 0x1f`. |
# +------------------------------------------------------------------------+
# | 2.1 -> 3.4 | 12.3 -> 12.1 | No      | De-duplicate serialized trees.   |
# +------------------------------------------------------------------------+
