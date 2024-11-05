# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
from dataclasses import dataclass
from enum import Enum
from operator import itemgetter

from .. import error, node, pathutil, util
from ..i18n import _

# this config is for testing purpose only
CFG_ALLOW_ANY_SOURCE_COMMIT = "allow-any-source-commit"

# todo: remove the 'test_' prefix when this feature is stable
SUBTREE_BRANCH_KEY = "test_subtree_copy"
SUBTREE_MERGE_KEY = "test_subtree_merge"

# keys that are used for subtree operations, this list should
# include the keys for both O(n) copy and O(1) copy
SUBTREE_OPERATION_KEYS = [
    SUBTREE_BRANCH_KEY,
    SUBTREE_MERGE_KEY,
]


class BranchType(Enum):
    DEEP_COPY = 1  # O(n) subtree copy
    SHALLOW_COPY = 2  # O(1) subtree copy

    def to_key(self):
        # the `key` is used in subtree metadata
        if self == BranchType.DEEP_COPY:
            return "deepcopies"
        elif self == BranchType.SHALLOW_COPY:
            return "copies"
        else:
            # unreachable
            raise error.ProgrammingError("unknown branch type")


@dataclass
class SubtreeBranch:
    version: int
    branch_type: BranchType
    from_commit: str
    from_path: str
    to_path: str

    def to_dict(self):
        return {
            "from_commit": self.from_commit,
            "from_path": self.from_path,
            "to_path": self.to_path,
        }


@dataclass
class SubtreeMerge:
    version: int
    from_commit: str
    from_path: str
    to_path: str

    def to_dict(self):
        return {
            "from_commit": self.from_commit,
            "from_path": self.from_path,
            "to_path": self.to_path,
        }


def gen_branch_info(repo, from_commit, from_paths, to_paths):
    if not is_source_commit_allowed(repo.ui, repo[from_commit]):
        return {}

    # sort by to_path
    path_mapping = sorted(zip(from_paths, to_paths), key=itemgetter(1))
    value = {
        "v": 1,
        "branches": [
            {
                "from_commit": from_commit,
                "from_path": from_path,
                "to_path": to_path,
            }
            for from_path, to_path in path_mapping
        ],
    }
    # compact JSON representation
    str_val = json.dumps(value, separators=(",", ":"), sort_keys=True)
    return {SUBTREE_BRANCH_KEY: str_val}


def gen_merge_info(repo, subtree_merges):
    subtree_merges = [
        m for m in subtree_merges if is_source_commit_allowed(repo.ui, repo[m[0]])
    ]
    if not subtree_merges:
        return {}

    # sort by to_path
    subtree_merges = sorted(subtree_merges, key=itemgetter(2))
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
    str_val = json.dumps(value, separators=(",", ":"), sort_keys=True)
    return {SUBTREE_MERGE_KEY: str_val}


def get_subtree_metadata(extra):
    """Get the subtree metadata from commit's extra."""
    return {k: v for k, v in extra.items() if k in SUBTREE_OPERATION_KEYS}


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


def validate_path_exist(ui, ctx, paths, abort_on_missing=False):
    """Validate that the given path exists in the given context."""
    for p in paths:
        if not (p in ctx or ctx.hasdir(p)):
            msg = _("path '%s' does not exist in commit %s") % (p, ctx)
            if abort_on_missing:
                raise error.Abort(msg)
            else:
                ui.status(msg + "\n")


def validate_path_size(from_paths, to_paths, abort_on_empty=False):
    if len(from_paths) != len(to_paths):
        raise error.Abort(_("must provide same number of --from-path and --to-path"))

    if abort_on_empty and not from_paths:
        raise error.Abort(_("must provide --from-path and --to-path"))


def validate_path_overlap(from_paths, to_paths):
    # Disallow overlapping --to-path to keep things simple.
    to_dirs = util.dirs(to_paths)
    seen = set()
    for p in to_paths:
        if p in to_dirs or p in seen:
            raise error.Abort(_("overlapping --to-path entries"))
        seen.add(p)

    from_dirs = util.dirs(from_paths)
    for from_path, to_path in zip(from_paths, to_paths):
        if from_path in to_dirs or to_path in from_dirs:
            raise error.Abort(
                _("overlapping --from-path '%s' and --to-path '%s'")
                % (from_path, to_path)
            )


def find_enclosing_dest(target_path, paths):
    """Find the path that contains the target path.

    >>> is_in_subtree_copy_dest("a/b/c", ["a/b"])
    'a/b'
    >>> is_in_subtree_copy_dest("a/b/c", ["a/b/c"])
    'a/b/c'
    >>> is_in_subtree_copy_dest("a/b/c", ["a/b", "e/f"])
    'a/b'
    >>> is_in_subtree_copy_dest("a/b/c", ["a/b/c/d", "e/f"])
    """
    target_dir = pathutil.dirname(target_path)
    for path in paths:
        if target_dir.startswith(path) or path == target_path:
            return path
    return None


def validate_source_commit(ui, source_ctx, subcmd_name):
    if is_source_commit_allowed(ui, source_ctx):
        return

    if educationpage := ui.config("subtree", "education-page"):
        hint = _("see subtree %s at %s for the impacts on subtree merge and log") % (
            subcmd_name,
            educationpage,
        )
    else:
        hint = _("see '@prog@ help subtree' for the impacts on subtree merge and log")
    prompt_msg = _(
        "subtree %s from a non-public commit is not recommended. However, you can\n"
        "still proceed and use subtree copy and merge for common cases.\n"
        "(hint: %s)\n"
        "Continue with subtree %s (y/n)? $$ &Yes $$ &No"
    ) % (subcmd_name, hint, subcmd_name)
    if ui.promptchoice(prompt_msg, default=1) != 0:
        raise error.Abort(
            f"subtree {subcmd_name} from a non-public commit is not allowed"
        )


def is_source_commit_allowed(ui, source_ctx) -> bool:
    # Currently, we only allow public commits as source commits
    # later, we will allow ancestor-draft commits as well.
    if ui.configbool("subtree", CFG_ALLOW_ANY_SOURCE_COMMIT):
        return True
    if source_ctx.ispublic():
        return True
    return False
