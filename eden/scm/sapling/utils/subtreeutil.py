# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
from collections import defaultdict
from dataclasses import dataclass
from enum import Enum
from typing import List

from .. import error, node, pathutil, util
from ..i18n import _

# this config is for testing purpose only
CFG_ALLOW_ANY_SOURCE_COMMIT = "allow-any-source-commit"

# todo: remove the 'test_' prefix when this feature is stable
SUBTREE_BRANCH_KEY = "test_subtree_copy"
SUBTREE_MERGE_KEY = "test_subtree_merge"

# XXX: remove the 'test_' prefix when server-side support is ready
SUBTREE_KEY = "test_subtree"
SUBTREE_METADATA_VERSION = 1  # current version of subtree metadata
SUPPORTED_SUBTREE_METADATA_VERSIONS = {1}

# keys that are used for subtree operations, this list should
# include the keys for both O(n) copy and O(1) copy
SUBTREE_OPERATION_KEYS = [
    SUBTREE_BRANCH_KEY,
    SUBTREE_MERGE_KEY,
    SUBTREE_KEY,
]

# keys will be removed from commit's extra after folding commits
DEPRECATED_SUBTREE_METADATA_KEYS = [
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


def gen_branch_info(
    repo,
    from_commit: str,
    from_paths: List[str],
    to_paths: List[str],
    branch_type: BranchType,
    version: int = SUBTREE_METADATA_VERSION,
):
    if not is_source_commit_allowed(repo.ui, repo[from_commit]):
        return {}

    branches = [
        SubtreeBranch(
            version=version,
            branch_type=branch_type,
            from_commit=from_commit,
            from_path=from_path,
            to_path=to_path,
        )
        for from_path, to_path in zip(from_paths, to_paths)
    ]
    metadata = _branches_to_dict(branches, version)
    return _encode_subtree_metadata_list([metadata])


def _branches_to_dict(branches: List[SubtreeBranch], version: int):
    if not branches:
        return {}
    rs = {}
    sorted_branches = sorted(branches, key=lambda x: x.to_path)
    for b in sorted_branches:
        key = b.branch_type.to_key()
        item = b.to_dict()
        rs.setdefault(key, []).append(item)
    rs["v"] = version
    return rs


def _encode_subtree_metadata_list(subtree_metadata):
    subtree_metadata = sorted(subtree_metadata, key=lambda x: x["v"])
    val_str = json.dumps(subtree_metadata, separators=(",", ":"), sort_keys=True)
    return {SUBTREE_KEY: val_str}


def gen_merge_info(repo, subtree_merges, version=SUBTREE_METADATA_VERSION):
    merges = [
        m for m in subtree_merges if is_source_commit_allowed(repo.ui, repo[m[0]])
    ]
    if not merges:
        return {}

    merges = [
        SubtreeMerge(
            version=version,
            from_commit=node.hex(from_node),
            from_path=from_path,
            to_path=to_path,
        )
        for from_node, from_path, to_path in subtree_merges
    ]
    metadata = _merges_to_dict(merges, version)
    return _encode_subtree_metadata_list([metadata])


def _merges_to_dict(merges: List[SubtreeMerge], version: int):
    if not merges:
        return {}
    merge_dict_list = []
    sorted_merges = sorted(merges, key=lambda x: x.to_path)
    for m in sorted_merges:
        item = m.to_dict()
        merge_dict_list.append(item)
    return {"v": version, "merges": merge_dict_list}


def get_subtree_metadata(extra):
    """Get the subtree metadata from commit's extra."""
    return {k: v for k, v in extra.items() if k in SUBTREE_OPERATION_KEYS}


def get_subtree_branches(repo, node) -> List[SubtreeBranch]:
    def detect_branch_type(repo, node):
        # we have not enabled "subtree.copy-reuse-tree" yet, so we use
        # a simple method here
        if not repo[node].changeset().files:
            return BranchType.SHALLOW_COPY
        else:
            return BranchType.DEEP_COPY

    extra = repo[node].extra()
    result = []
    if metadata_list := _get_subtree_metadata(extra, SUBTREE_KEY):
        for metadata in metadata_list:
            for branch_type in BranchType:
                key = branch_type.to_key()
                branches = metadata.get(key, [])
                for b in branches:
                    result.append(
                        SubtreeBranch(
                            version=metadata["v"],
                            branch_type=branch_type,
                            from_commit=b["from_commit"],
                            from_path=b["from_path"],
                            to_path=b["to_path"],
                        )
                    )

    if branch_info := _get_subtree_metadata(extra, SUBTREE_BRANCH_KEY):
        for b in branch_info.get("branches", []):
            branch_type = detect_branch_type(repo, node)
            result.append(
                SubtreeBranch(
                    version=branch_info["v"],
                    branch_type=branch_type,
                    from_commit=b["from_commit"],
                    from_path=b["from_path"],
                    to_path=b["to_path"],
                )
            )
    return result


def get_subtree_merges(repo, node) -> List[SubtreeMerge]:
    extra = repo[node].extra()
    result = []
    if metadata_list := _get_subtree_metadata(extra, SUBTREE_KEY):
        for metadata in metadata_list:
            for merge in metadata.get("merges", []):
                result.append(
                    SubtreeMerge(
                        version=metadata["v"],
                        from_commit=merge["from_commit"],
                        from_path=merge["from_path"],
                        to_path=merge["to_path"],
                    )
                )

    if merge_info := _get_subtree_metadata(extra, SUBTREE_MERGE_KEY):
        for m in merge_info.get("merges", []):
            result.append(
                SubtreeMerge(
                    version=merge_info["v"],
                    from_commit=m["from_commit"],
                    from_path=m["from_path"],
                    to_path=m["to_path"],
                )
            )
    return result


def _get_subtree_metadata(extra, key):
    try:
        val_str = extra[key]
    except KeyError:
        return None
    try:
        return json.loads(val_str)
    except json.JSONDecodeError:
        raise error.Abort(f"invalid {key} metadata: {val_str}")


def merge_subtree_metadata(repo, ctxs):
    # XXX: add 'imports' support when implementing 'subtree import' command
    branches, merges = [], []

    # get subtree metadata
    for ctx in ctxs:
        node = ctx.node()
        branches.extend(get_subtree_branches(repo, node))
        merges.extend(get_subtree_merges(repo, node))

    if not branches and not merges:
        return {}

    # metadata merge validation
    if branches and merges:
        # Even if we currently reject subtree copy and merge, the following logic
        # supports it. This allows us to enable it later if needed
        raise error.Abort(_("cannot combine commits with both subtree copy and merge"))

    from_paths = [b.from_path for b in branches] + [m.from_path for m in merges]
    to_paths = [b.to_path for b in branches] + [m.to_path for m in merges]
    try:
        validate_path_overlap(from_paths, to_paths)
    except error.Abort as e:
        raise error.Abort(
            _("cannot combine commits with overlapping subtree copy/merge paths"),
            hint=str(e),
        )

    # group by version
    version_to_branches = defaultdict(list)
    version_to_merges = defaultdict(list)
    for b in branches:
        version_to_branches[b.version].append(b)
    for m in merges:
        version_to_merges[m.version].append(m)

    # validate versions
    versions = set(version_to_branches.keys()) | set(version_to_merges.keys())
    if unsupported_versions := versions - SUPPORTED_SUBTREE_METADATA_VERSIONS:
        raise error.Abort(
            _("unsupported subtree metadata versions: %s")
            % ", ".join(map(str, unsupported_versions))
        )

    # gen merged metadata
    result = []
    for v in versions:
        item = _branches_to_dict(version_to_branches[v], v)
        item.update(_merges_to_dict(version_to_merges[v], v))
        result.append(item)

    if not result:
        return {}
    return _encode_subtree_metadata_list(result)


def remove_old_subtree_keys_from_extra(extra):
    """Remove old subtree metadata keys from commit's extra after folding commits"""
    for k in DEPRECATED_SUBTREE_METADATA_KEYS:
        if k in extra:
            del extra[k]


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


def contains_shallow_copy(repo, node):
    branches = get_subtree_branches(repo, node)
    for b in branches:
        if b.branch_type == BranchType.SHALLOW_COPY:
            return True
    return False


def check_commit_splitability(repo, node):
    """Check if the given commit can be split into multiple commits.

    This function checks if the given commit contains any subtree metadata.
    If so, it aborts the command because splitting commits will lose subtree
    metadata.
    """
    extra = repo[node].extra()
    for key in SUBTREE_OPERATION_KEYS:
        if key in extra:
            raise error.Abort(_("cannot split subtree copy/merge commits"))
