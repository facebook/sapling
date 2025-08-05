# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import hashlib
import json
import os
import shutil
from collections import defaultdict
from dataclasses import asdict, dataclass
from enum import Enum
from typing import List, Optional, Set

import bindings

from .. import error, match as matchmod, node, pathutil, util
from ..i18n import _

# this config is for testing purpose only
CFG_ALLOW_ANY_SOURCE_COMMIT = "allow-any-source-commit"

# SUBTREE_BRANCH_KEY and SUBTREE_MERGE_KEY are deprecated by SUBTREE_KEY
SUBTREE_BRANCH_KEY = "test_subtree_copy"
SUBTREE_MERGE_KEY = "test_subtree_merge"

# the test key is used when server does not support subtree
TEST_SUBTREE_KEY = "test_subtree"
PROD_SUBTREE_KEY = "subtree"

SUBTREE_METADATA_VERSION = 1  # current version of subtree metadata
SUPPORTED_SUBTREE_METADATA_VERSIONS = {1}


def get_subtree_key(ui) -> str:
    """Get the key used in commit's extra for subtree metadata."""
    return (
        PROD_SUBTREE_KEY
        if ui.configbool("subtree", "use-prod-subtree-key")
        else TEST_SUBTREE_KEY
    )


def get_subtree_metadata_keys() -> Set[str]:
    """Keys used in subtree operations.

    Includes deprecated keys that are kept for backward compatibility with
    existing metadata.
    """
    keys = {
        SUBTREE_BRANCH_KEY,
        SUBTREE_MERGE_KEY,
        TEST_SUBTREE_KEY,
        PROD_SUBTREE_KEY,
    }
    return keys


def get_deprecated_subtree_metadata_keys(ui) -> Set[str]:
    """Keys that are no longer in use.

    These keys will be removed from commit's extra after folding draft commits.
    """
    keys = {
        SUBTREE_BRANCH_KEY,
        SUBTREE_MERGE_KEY,
    }
    if get_subtree_key(ui) != TEST_SUBTREE_KEY:
        keys.add(TEST_SUBTREE_KEY)
    return keys


# the following is the format of subtree metadata
# [
#   {
#     "deepcopies": [
#       {
#         "from_commit": "b4cb27eee4e2633aae0d62de87523007d1b5bfdd",
#         "from_path": "foo",
#         "to_path": "foo2"
#       },
#       {
#         "from_commit": "b4cb27eee4e2633aae0d62de87523007d1b5bfdd",
#         "from_path": "foo",
#         "to_path": "foo3"
#       }
#     ],
#     "v": 1
#   },
#   {
#     "merges": [
#       {
#         "from_commit": "eeb423c321b3fae8bffd501cecd7db6d8fa9b6da",
#         "from_path": "foo",
#         "to_path": "foo2"
#       }
#     ],
#     "v": 1
#   },
#   {
#     "imports": [
#       {
#         "from_commit": "1c9ffab8a5868369895cbdd5d42cf2b34361c5ae",
#         "from_path": "",
#         "to_path": "foo/sapling",
#         "url": "https://github.com/facebook/sapling.git"
#       }
#     ],
#     "v": 1
#   }
# ]


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

    def to_str(self):
        if self == BranchType.DEEP_COPY:
            return "deepcopy"
        elif self == BranchType.SHALLOW_COPY:
            return "copy"
        else:
            raise error.ProgrammingError("unknown branch type")


@dataclass
class SubtreeBranch:
    version: int
    branch_type: BranchType
    from_commit: str
    from_path: str
    to_path: str

    def to_minimal_dict(self):
        skip_keys = {"branch_type", "version"}
        return {k: v for k, v in self.__dict__.items() if k not in skip_keys}

    def to_full_dict(self):
        d = {k: v for k, v in self.__dict__.items() if k != "branch_type"}
        d["type"] = self.branch_type.to_str()
        return d


@dataclass
class SubtreeMerge:
    version: int
    from_commit: str
    from_path: str
    to_path: str

    def to_minimal_dict(self):
        return {k: v for k, v in self.__dict__.items() if k != "version"}

    def to_full_dict(self):
        return asdict(self)


@dataclass
class SubtreeImport:
    version: int
    url: str
    from_commit: str
    from_path: str  # "" means the root of the repo
    to_path: str

    def to_minimal_dict(self):
        return {k: v for k, v in self.__dict__.items() if k != "version"}

    def to_full_dict(self):
        return asdict(self)


### Generating metadata for branches (copies)


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
    return _encode_subtree_metadata_list(repo.ui, [metadata])


def _branches_to_dict(branches: List[SubtreeBranch], version: int):
    if not branches:
        return {}
    rs = {}
    sorted_branches = sorted(branches, key=lambda x: x.to_path)
    for b in sorted_branches:
        key = b.branch_type.to_key()
        item = b.to_minimal_dict()
        rs.setdefault(key, []).append(item)
    rs["v"] = version
    return rs


def _encode_subtree_metadata_list(ui, subtree_metadata):
    subtree_metadata = sorted(subtree_metadata, key=lambda x: x["v"])
    val_str = json.dumps(subtree_metadata, separators=(",", ":"), sort_keys=True)
    subtree_key = get_subtree_key(ui)
    return {subtree_key: val_str}


### Generating metadata for imports


def gen_import_info(
    ui, url, from_commit, from_paths, to_paths, version=SUBTREE_METADATA_VERSION
):
    imports = [
        SubtreeImport(
            version=version,
            url=url,
            from_commit=from_commit,
            from_path=from_path,
            to_path=to_path,
        )
        for from_path, to_path in zip(from_paths, to_paths)
    ]
    metadata = _imports_to_dict(imports, version)
    return _encode_subtree_metadata_list(ui, [metadata])


def _imports_to_dict(imports: List[SubtreeImport], version: int):
    if not imports:
        return {}
    sorted_imports = sorted(imports, key=lambda x: x.to_path)
    import_dist_list = [im.to_minimal_dict() for im in sorted_imports]
    return {"v": version, "imports": import_dist_list}


### Generating metadata for merges


def gen_merge_info(repo, subtree_merges, version=SUBTREE_METADATA_VERSION):
    merges = [
        m
        for m in subtree_merges
        if is_source_commit_allowed(repo.ui, repo[m["from_commit"]])
    ]
    if not merges:
        return {}

    # XXX: handle cross-repo merge metadata
    merges = [
        SubtreeMerge(
            version=version,
            from_commit=node.hex(m["from_commit"]),
            from_path=m["from_path"],
            to_path=m["to_path"],
        )
        for m in subtree_merges
    ]
    metadata = _merges_to_dict(merges, version)
    return _encode_subtree_metadata_list(repo.ui, [metadata])


def _merges_to_dict(merges: List[SubtreeMerge], version: int):
    if not merges:
        return {}
    merge_dict_list = []
    sorted_merges = sorted(merges, key=lambda x: x.to_path)
    for m in sorted_merges:
        item = m.to_minimal_dict()
        merge_dict_list.append(item)
    return {"v": version, "merges": merge_dict_list}


def get_subtree_metadata(extra):
    """Get the subtree metadata from commit's extra."""
    return {k: v for k, v in extra.items() if k in get_subtree_metadata_keys()}


### Getting subtree metadata: branches, imports, merges


def get_subtree_branches(repo, node) -> List[SubtreeBranch]:
    def detect_branch_type(repo, node):
        # we have not enabled shallow copies yet, so we use
        # a simple method here
        if not repo[node].changeset().files:
            return BranchType.SHALLOW_COPY
        else:
            return BranchType.DEEP_COPY

    extra = repo[node].extra()
    result = []
    if metadata_list := _get_subtree_metadata_by_subtree_keys(extra):
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
    if metadata_list := _get_subtree_metadata_by_subtree_keys(extra):
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


def get_subtree_imports(repo, node) -> List[SubtreeImport]:
    extra = repo[node].extra()
    result = []
    if metadata_list := _get_subtree_metadata_by_subtree_keys(extra):
        for metadata in metadata_list:
            for im in metadata.get("imports", []):
                result.append(
                    SubtreeImport(
                        version=metadata["v"],
                        url=im["url"],
                        from_commit=im["from_commit"],
                        from_path=im["from_path"],
                        to_path=im["to_path"],
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


def _get_subtree_metadata_by_subtree_keys(extra):
    if PROD_SUBTREE_KEY in extra and TEST_SUBTREE_KEY in extra:
        raise error.Abort(_("commit extra contains multiple subtree keys"))

    if metadata := _get_subtree_metadata(extra, PROD_SUBTREE_KEY):
        return metadata
    else:
        # backward compatibility with existing metadata
        return _get_subtree_metadata(extra, TEST_SUBTREE_KEY)


def merge_subtree_metadata(repo, ctxs):
    branches, merges, imports = [], [], []

    # get subtree metadata
    for ctx in ctxs:
        node = ctx.node()
        branches.extend(get_subtree_branches(repo, node))
        merges.extend(get_subtree_merges(repo, node))
        imports.extend(get_subtree_imports(repo, node))

    if not branches and not merges and not imports:
        return {}

    # metadata merge validation
    if bool(branches) + bool(merges) + bool(imports) > 1:
        # Although we currently reject combining different subtree operations,
        # the logic below supports it. This allows us to enable it later if needed.
        raise error.Abort(
            _("cannot combine different subtree operations: copy, merge or import")
        )

    from_paths = [b.from_path for b in branches] + [m.from_path for m in merges]
    to_paths = (
        [b.to_path for b in branches]
        + [m.to_path for m in merges]
        + [i.to_path for i in imports]
    )
    try:
        validate_path_overlap(from_paths, to_paths)
    except error.Abort as e:
        raise error.Abort(
            _("cannot combine commits with overlapping subtree paths"),
            hint=str(e),
        )
    # skip checking the from_paths of imports
    from_paths += [i.from_path for i in imports]

    # group by version
    version_to_branches = defaultdict(list)
    version_to_merges = defaultdict(list)
    version_to_imports = defaultdict(list)
    for b in branches:
        version_to_branches[b.version].append(b)
    for m in merges:
        version_to_merges[m.version].append(m)
    for i in imports:
        version_to_imports[i.version].append(i)

    # validate versions
    versions = (
        set(version_to_branches.keys())
        | set(version_to_merges.keys())
        | set(version_to_imports.keys())
    )
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
        item.update(_imports_to_dict(version_to_imports[v], v))
        result.append(item)

    if not result:
        return {}
    return _encode_subtree_metadata_list(repo.ui, result)


def remove_old_subtree_keys_from_extra(ui, extra):
    """Remove old subtree metadata keys from commit's extra after folding commits"""
    for k in get_deprecated_subtree_metadata_keys(ui):
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


def validate_path_depth(ui, paths):
    """Validate that the given path is at least the given depth."""
    min_depth = ui.configint("subtree", "min-path-depth")
    if min_depth is None:
        return
    for p in paths:
        p = util.pconvert(p)
        if p.count("/") < (min_depth - 1):
            raise error.Abort(
                _("path should be at least %d levels deep: '%s'") % (min_depth, p)
            )


def validate_file_count(repo, ctx, paths):
    max_file_count = repo.ui.configint("subtree", "max-file-count")
    if max_file_count is None:
        return
    mf = ctx.manifest()
    for p in paths:
        matcher = matchmod.match(repo.root, "", [f"path:{p}"])
        count = mf.countfiles(matcher)
        if count > max_file_count:
            raise error.Abort(
                _("subtree path '%s' includes too many files: %d (max: %d)")
                % (p, count, max_file_count)
            )


def validate_path_size(from_paths, to_paths, abort_on_empty=False):
    if len(from_paths) != len(to_paths):
        raise error.Abort(
            _("must provide same number of --from-path %s and --to-path %s")
            % (from_paths, to_paths)
        )

    if abort_on_empty and not from_paths:
        raise error.Abort(_("must provide --from-path and --to-path"))


def validate_path_overlap(from_paths, to_paths, is_crossrepo=False):
    # Disallow overlapping --to-path to keep things simple.
    to_dirs = util.dirs(to_paths)
    seen = set()
    for p in to_paths:
        if p in to_dirs or p in seen:
            raise error.Abort(_("overlapping --to-path entries"))
        seen.add(p)

    if is_crossrepo:
        # skip checking the overlap between from_paths and to_paths, since
        # they are in different repos
        return
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


def is_commit_graftable(repo, rev) -> bool:
    ui = repo.ui
    if ui.configbool("subtree", "allow-graft-subtree-commit"):
        return True

    ctx = repo[rev]
    if get_subtree_metadata(ctx.extra()):
        subtree_ops = ", ".join(["copy", "merge", "import"])
        ui.warn(
            _("skipping ungraftable subtree (%s) revision %s\n") % (subtree_ops, ctx)
        )
        return False
    return True


def contains_shallow_copy(repo, node):
    branches = get_subtree_branches(repo, node)
    for b in branches:
        if b.branch_type == BranchType.SHALLOW_COPY:
            return True
    return False


def extra_contains_shallow_copy(extra) -> bool:
    """Check if the given commitctx extra contains any shallow copy metadata.

    N.B. This function does not apply to "v0" subtree metadata because "v0" does
    not have shallow copy type. It is used for newly incoming commits.
    """
    shallow_copy_key = BranchType.SHALLOW_COPY.to_key()
    if metadata_list := _get_subtree_metadata_by_subtree_keys(extra):
        for metadata in metadata_list:
            if shallow_copy_key in metadata:
                return True
    return False


def check_commit_splitability(repo, node):
    """Check if the given commit can be split into multiple commits.

    This function checks if the given commit contains any subtree metadata.
    If so, it aborts the command because splitting commits will lose subtree
    metadata.
    """
    extra = repo[node].extra()
    for key in get_subtree_metadata_keys():
        if key in extra:
            raise error.Abort(_("cannot split subtree copy/merge commits"))


def get_or_clone_git_repo(ui, url, from_rev=None):
    def try_reuse_git_repo(git_repo_dir):
        """try to reuse an existing git repo, otherwise return None"""
        if not os.path.exists(git_repo_dir):
            return None
        if not os.path.isdir(git_repo_dir):
            # should not happen, but just in case
            os.unlink(git_repo_dir)
            return None

        try:
            git_repo = localrepo.localrepository(ui, git_repo_dir)
        except Exception:
            # invalid git repo directory, remove it
            shutil.rmtree(git_repo_dir)
            return None

        ui.status(_("using cached git repo at %s\n") % git_repo_dir)
        nodes, pullnames = [], []
        if from_rev:
            if from_node := git.try_get_node(from_rev):
                nodes.append(from_node)
            else:
                pullnames.append(from_rev)
        git.pull(git_repo, "default", names=pullnames, nodes=nodes)
        return git_repo

    from .. import git, localrepo

    url_hash = hashlib.sha256(url.encode("utf-8")).hexdigest()
    if cache_dir := ui.config("remotefilelog", "cachepath"):
        cache_dir = util.expandpath(cache_dir)
        git_repo_dir = os.path.join(cache_dir, "gitrepos", url_hash)
    else:
        user_cache_dir = bindings.dirs.cache_dir()
        git_repo_dir = os.path.join(user_cache_dir, "Sapling", "gitrepos", url_hash)

    if git_repo := try_reuse_git_repo(git_repo_dir):
        return git_repo
    else:
        ui.status(_("creating git repo at %s\n") % git_repo_dir)
        # PERF: shallow clone, then partial checkout
        git_repo = git.clone(ui, url, git_repo_dir, update=from_rev)
        return git_repo


def find_subtree_copy(repo, node, path):
    """find the source commit and path of a subtree copy (directory branch)"""
    branches = get_subtree_branches(repo, node)
    for branch in branches:
        if path_starts_with(path, branch.to_path):
            source_path = branch.from_path + path[len(branch.to_path) :]
            return (branch.from_commit, source_path)
    return None


def find_subtree_import(repo, node, path):
    """find the source url, commit and path of a subtree imported file/directory"""
    imports = get_subtree_imports(repo, node)
    for im in imports:
        if path_starts_with(path, im.to_path):
            source_path = im.from_path + path[len(im.to_path) :]
            source_path = source_path.lstrip("/")
            return (im.url, im.from_commit, source_path)
    return None


def path_starts_with(path, prefix):
    """Return True if 'path' is the same as 'prefix' or lives underneath it.

    Examples
    --------
    >>> path_starts_with("/var/log/nginx/error.log", "/var/log")
    True
    >>> path_starts_with("/var/logs", "/var/log")   # subtle typo
    False
    >>> path_starts_with("src/module/util.py", "src")  # relative paths fine
    True
    """
    if path == prefix:
        return True
    return path.startswith(prefix + "/")


def xrepo_link(
    repo, from_url: str, from_commit: str, from_path: str, lineno: int
) -> Optional[str]:
    ui = repo.ui
    github_link_format = "https://github.com/%s/blob/%s/%s#L%s"
    if from_url.startswith("git@github.com:"):
        repo_name = from_url[len("git@github.com:") : -4]
        return github_link_format % (repo_name, from_commit, from_path, lineno)
    elif from_url.startswith("https://github.com/"):
        repo_name = from_url[len("https://github.com/") : -4]
        return github_link_format % (repo_name, from_commit, from_path, lineno)
    elif link_format := ui.config("blame", from_url):
        return link_format.format(
            from_commit=from_commit, from_path=from_path, lineno=lineno
        )
    return None
