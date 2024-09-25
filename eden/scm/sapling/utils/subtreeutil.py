# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json

from .. import error, node

# todo: remove the 'test_' prefix when this feature is stable
SUBTREE_BRANCH_KEY = "test_subtree_copy"
SUBTREE_MERGE_KEY = "test_subtree_merge"


def gen_branch_info(from_commit, from_paths, to_paths):
    value = {
        "v": 1,
        "branches": [
            {
                "from_path": from_path,
                "to_path": to_path,
                "from_commit": from_commit,
            }
            for from_path, to_path in zip(from_paths, to_paths)
        ],
    }
    # compact JSON representation
    str_val = json.dumps(value, separators=(",", ":"))
    return {SUBTREE_BRANCH_KEY: str_val}


def gen_merge_info(subtree_merges):
    if not subtree_merges:
        return {}
    value = {
        "v": 1,
        "merges": [
            {
                "from_commit": node.hex(from_node),
                "from_path": from_path,
                "to_path": to_path,
            }
            for from_node, from_path, to_path in subtree_merges
        ],
    }
    # compact JSON representation
    str_val = json.dumps(value, separators=(",", ":"))
    return {SUBTREE_MERGE_KEY: str_val}


def get_branch_info(repo, node):
    return _get_subtree_metadata(repo, node, SUBTREE_BRANCH_KEY)


def get_merge_info(repo, node):
    return _get_subtree_metadata(repo, node, SUBTREE_MERGE_KEY)


def _get_subtree_metadata(repo, node, key):
    extra = repo[node].extra()
    try:
        val_str = extra[key]
    except KeyError:
        return None
    try:
        return json.loads(val_str)
    except json.JSONDecodeError:
        raise error.Abort(f"invalid {key} metadata: {val_str}")
