# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import base64, collections, functools

from .. import context, hg, json, mutation, scmutil
from ..i18n import _
from ..node import bin, hex
from .cmdtable import command


@command("debugexportstack", [("r", "rev", [], _("revisions to export"))])
def debugexportstack(ui, repo, **opts):
    """dump content of commits for automation consumption

    On success, print a list of commit information encoded in JSON, like::

        [{"node": "1f0eaeff1a01e2594efedceefac9987cda047204",
          "author": "Foo Bar <foo@bar.com>",
          "date": [unix_timestamp, timezone_offset],
          "text": "commit message",

          // Whether the commit is immutable (public).
          "immutable": true | false,

          // Whether information of this commit is explicitly requested.
          // This affects the meaning of "files" and decides whether
          // "parents" is present or not.
          "requested": true | false,

          // "parents" is only present for explicitly requested commits.
          "parents": [parent_node],

          // For explicitly requested commits, the "files" map contains
          // files explicitly changed or deleted by the commits.
          "files": {
            // null: file is deleted by this commit.
            "foo/bar.txt": null | {
              // Present if the file content is utf-8.
              "data": "utf-8 file content",

              // Present if the file content is not utf-8.
              "dataBase85": "base85 encoded data",

              // Present if the file is copied from "path/from".
              // The content of "path/from" will be included in "relevant_map"
              // of the parent commits.
              "copyFrom": "path/from",

              // "l": symlink; "x": executable.
              "flags": "",
            },
            ...
          },

          // Relevant files for diff rendering purpose.
          // "files" render as the "after" state, "relevant_map" (in parent
          // commits) render as the "before" state. If the file is already
          // included in "files" of the parent commit, then it will be skipped
          // in "relevantFiles"
          "relevantFiles": {
            "foo/bar.txt": null | { ... }
            ...
          },
         },
         ...
        ]

    The commits are sorted topologically. Ancestors (roots) first, descendants
    (heads) last.

    On error (ex. exceeds size limit), print a JSON object with "error" set
    to the actual problem:

        {"error": "too many commits"}
        {"error": "too many files changed"}

    Configs::

        # Maximum (explicitly requested) commits to export
        experimental.exportstack-max-commit-count = 50

        # Maximum files to export
        experimental.exportstack-max-file-count = 200

        # Maximum bytes of files to export
        experimental.exportstack-max-bytes = 2M
    """
    # size limits
    max_commit_count = ui.configint("experimental", "exportstack-max-commit-count")
    max_file_count = ui.configint("experimental", "exportstack-max-file-count")
    max_bytes = ui.configbytes("experimental", "exportstack-max-bytes")

    commit_limiter = _size_limiter(max_commit_count, "too many commits")
    file_limiter = _size_limiter(max_file_count, "too many files")
    bytes_limiter = _size_limiter(max_bytes, "too much data")

    # Figure out relevant commits and files:
    # - If rev X modifies or deleted (not "added") a file, then the file in
    #   parents(X) is relevant.
    # - If rev X contains a "copy from (rev Y, path Z)" file, then path Z in
    #   rev Y is relevant.

    # {node: {path: file}}
    relevant_map = collections.defaultdict(dict)
    requested_map = collections.defaultdict(dict)

    revs = scmutil.revrange(repo, opts.get("rev"))
    revs.sort()

    try:
        for ctx in revs.prefetch("text").iterctx():
            commit_limiter(1)
            pctx = ctx.p1()
            files = requested_map[ctx.node()]
            for path in ctx.files():
                file_limiter(1)
                parent_paths = [path]
                file_obj = _file_obj(ctx, path, parent_paths.append, bytes_limiter)
                files[path] = file_obj
                for pctx in ctx.parents():
                    pfiles = relevant_map[pctx.node()]
                    for ppath in parent_paths:
                        # Skip relevant_map if included by requested_map.
                        if ppath not in requested_map.get(pctx.node(), {}):
                            pfiles[ppath] = _file_obj(
                                pctx, ppath, limiter=bytes_limiter
                            )
    except ValueError as ex:
        ui.write("%s\n" % json.dumps({"error": str(ex)}))
        return 1

    # Put together final result
    result = []
    public_nodes = repo.dageval(lambda: public())

    for node in repo.dageval(
        lambda: sort(tonodes(revs) | list(relevant_map))
    ).iterrev():
        ctx = repo[node]
        requested = node in requested_map
        commit_obj = {
            "node": hex(node),
            "author": ctx.user(),
            "date": ctx.date(),
            "immutable": node in public_nodes,
            "requested": requested,
            "text": ctx.description(),
        }
        relevant_files = relevant_map.get(node)
        if relevant_files:
            commit_obj["relevantFiles"] = relevant_files
        if requested:
            commit_obj["parents"] = [p.hex() for p in ctx.parents()]
            commit_obj["files"] = requested_map[node]
        result.append(commit_obj)

    ui.write("%s\n" % json.dumps(result))
    return 0


def _size_limiter(limit, error_message):
    limit = limit or 0
    current = 0

    def increase(size):
        nonlocal current
        current += size
        if limit > 0 and current > limit:
            raise ValueError(error_message)

    return increase


def _file_obj(ctx, path, set_copy_from=None, limiter=None):
    if path in ctx:
        fctx = ctx[path]
        renamed = fctx.renamed()
        copy_from_path = None
        if renamed and renamed[0] != path:
            copy_from_path = renamed[0]
            if set_copy_from is not None:
                set_copy_from(copy_from_path)
        bdata = fctx.data()
        if limiter is not None:
            limiter(len(bdata))
        file_obj = {}
        try:
            file_obj["data"] = bdata.decode("utf-8")
        except UnicodeDecodeError:
            file_obj["dataBase85"] = base64.b85encode(bdata)
        if copy_from_path:
            file_obj["copyFrom"] = copy_from_path
        flags = fctx.flags()
        if flags:
            file_obj["flags"] = flags
    else:
        file_obj = None
    return file_obj


@command("debugimportstack")
def debugimportstack(ui, repo, **opts):
    """import a stack of commits

    Create a stack of commits based on information read from stdin.

    The stdin should provide a list of actions in JSON, like:

        [["commit", commit_info],
         ["commit", commit_info],
         ["goto", {"mark": mark}],
         ["reset", {"mark": mark}]]

    "goto" performs a checkout that will overwrite conflicted files.

    "reset" moves the "current commit" to the given rev without changing
    files in the working copy. It can be useful to implement "absorb" or
    "commit -i". It is similar to the ``reset -kr REV`` command.

    Both "goto" and "reset" accept "mark" only.

    "commit" makes a commit without changing files in the working copy,
    "commit_info" looks like:

        {"author": "Foo Bar <foo@bar.com>",
         "date": [unix_timestamp, timezone_offset],
         "text": "commit message",

         // Commit identity - commit hash is unknown for now.
         // mark always starts with ":".
         "mark": ":1",

         // Parent nodes or marks. They must be known already.
         // Do not refer to commits after this commit.
         "parents": [node | mark],

         // Predecessors that will be obsoleted. Optional.
         "predecessors": [node | mark],

         // Why predecessor was changed to this commit.
         // Optional. Ignored if predecessors are empty.
         "operation": "amend",

         // File changed by this commit.
         "files": {
           // null: file is deleted by this commit, otherwise
           // added or modified.
           "foo/bar.txt": null | {
             // The file content is utf-8.
             "data": "utf-8 file content",

             // The file content is encoded in base85.
             "dataBase85": "base85 encoded data",

             // Present if the file is copied from the parent commit.
             "copyFrom": "path/from",

             // "l": symlink; "x": executable.
             "flags": "",
           },
           ...
         }
        }

    The format of "commit_info" is similar to the output of
    ``debugexportstack``, with some differences:

    - No ``relevantFiles``.
    - No ``node``. Use ``mark`` instead. ``parents`` can refer to marks.
    - Has ``predecessors``.

    Bookmarks will be moved if they become obsoleted (referred by
    ``predecessors``).

    Working copy parent is not automatically moved. Use a separate
    ``goto`` or ``reset`` to move it.

    On success, print a list of commit hashes that original placeholders
    resolve into, in JSON, in the first line::

        [{"node": "1f0eaeff1a01e2594efedceefac9987cda047204",
          "mark": ":1"},
          ...
        ]

    On error, print a JSON object with "error" set to the actual problem::

        {"error": "mark didn't start with ':'"}
        {"error": "merges are unsupported"}
        {"error": "commits form a cycle"}

    There might be extra output caused by the "goto" operation after the first
    line. Those should be ignored by automation.
    """
    marks = Marks()
    wnode = repo["."].node()

    try:
        try:
            actions = json.loads(ui.fin.read())
        except json.JSONDecodeError as ex:
            raise ValueError(f"commit info is invalid JSON ({ex})")

        with repo.wlock(), repo.lock(), repo.transaction("importstack"):
            # Create commits.
            commit_infos = [action[1] for action in actions if action[0] == "commit"]
            _create_commits(repo, commit_infos, marks)

            # Handle "goto" or "reset".
            for action in actions:
                action_name = action[0]
                if action_name == "commit":
                    # Handled by _create_commits already.
                    continue
                elif action_name == "goto":
                    node = marks[action[1]["mark"]]
                    hg.updaterepo(repo, node, overwrite=True)
                elif action_name == "reset":
                    node = marks[action[1]["mark"]]
                    _reset(repo, node)
                else:
                    raise ValueError(f"unsupported action: {action}")

    except (ValueError, TypeError, KeyError, AttributeError) as ex:
        ui.write("%s\n" % json.dumps({"error": str(ex)}))
        return 1

    ui.write("%s\n" % json.dumps(marks.to_hex()))

    return 0


class Marks:
    """Track marks (pending commit hashes)"""

    def __init__(self):
        self._mark_to_node = {}  # {mark: node}

    def to_nodes(self, items):
        """Resolve hex or marks to (binary) nodes"""
        result = []
        for item in items or []:
            if item.startswith(":"):
                node = self._mark_to_node.get(item)
                if not node:
                    raise ValueError(f"cannot resolve mark {item} to node")
                result.append(node)
            else:
                node = bin(item)
                result.append(node)
        return result

    def set(self, mark, node):
        self._mark_to_node[mark] = node

    def to_hex(self):
        """Return {mark: hex_node}"""
        return {mark: hex(node) for mark, node in self._mark_to_node.items()}

    def __contains__(self, mark):
        return mark in self._mark_to_node

    def __getitem__(self, mark):
        return self._mark_to_node[mark]


def _create_commits(repo, commit_infos, marks: Marks):
    """Create commits based on commit_infos.
    Do not change the working copy.
    Assumes inside a transaction.
    """
    # Split pre-processing.
    # When A is split into A1 and A2, both A1 and A2 have
    # the same predecessor A. The mutation information only
    # tracks the head of the split. So we'll change
    # - A1: predecessors=[A]
    # - A2: predecessors=[A]
    # to:
    # - A1: predecessors=[]
    # - A2: predecessors=[A], split=[A1, A2]

    # For example, if A is split to A1-A2-A3 stack, where A3 is the top), then:
    # - pred_to_stack_top[A] = A3
    # - mark_to_split_marks[A3] = [A1, A2]
    # - mark_to_ignore_pred = [A1, A2]
    pred_to_stack_top = {}  # {node_or_mark: mark}
    mark_to_split_marks = collections.defaultdict(list)  # {mark: [mark]}
    mark_to_ignore_pred = set()  # {mark}
    for commit_info in commit_infos:
        preds = commit_info.get("predecessors")
        if preds and len(preds) == 1:
            mark = commit_info["mark"]
            pred = preds[0]
            top = pred_to_stack_top.get(pred)
            if top and top in (commit_info.get("parents") or ()):
                # This commit is a continuation of the split.
                split_marks = mark_to_split_marks.pop(top, [])
                split_marks.append(top)
                mark_to_split_marks[mark] = split_marks
                mark_to_ignore_pred.add(top)
            pred_to_stack_top[pred] = mark

    # Make commits.
    moves = collections.defaultdict(list)  # {node: [node]}
    for commit_info in commit_infos:
        mark = commit_info["mark"]
        if mark in marks or not mark.startswith(":"):
            raise ValueError(f"invalid mark: {mark}")
        if mark in mark_to_ignore_pred:
            pred_nodes = []
        else:
            pred_nodes = marks.to_nodes(commit_info.get("predecessors"))
        parent_nodes = marks.to_nodes(commit_info.get("parents"))
        parents = [repo[n] for n in parent_nodes]
        text = commit_info["text"]
        user = commit_info.get("author")
        date = commit_info.get("date")
        if isinstance(date, list):
            date = tuple(date)
        files_dict = commit_info.get("files") or {}
        split_nodes = None
        if len(pred_nodes) == 1:
            pred = commit_info["predecessors"][0]
            split_marks = mark_to_split_marks.get(mark)
            if split_marks:
                split_nodes = marks.to_nodes(split_marks)

        if pred_nodes:
            operation = commit_info.get("operation") or "importstack"
            mutinfo = mutation.record(repo, {}, pred_nodes, operation, split_nodes)
        else:
            mutinfo = None

        files = list(files_dict.keys())
        filectxfn = functools.partial(_filectxfn, files_dict=files_dict)
        mctx = context.memctx(
            repo,
            parents,
            text,
            files,
            filectxfn,
            user,
            date,
            mutinfo=mutinfo,
        )
        node = repo.commitctx(mctx)
        for pred in pred_nodes:
            moves[pred].append(node)
        marks.set(mark, node)

        # Move bookmarks and adjust visibility.
        scmutil.cleanupnodes(repo, moves, operation="importstack")


def _reset(repo, node):
    """Update "current commit" to point to ``node``.
    Mark changed files accordingly.
    """
    # This probably belongs to part of dirstate itself.
    wctx = repo[None]
    ctx = repo[node]
    m1 = wctx.manifest()
    m2 = ctx.manifest()
    diff = m1.diff(m2)
    dirstate = repo.dirstate
    changedfiles = list(diff)
    with dirstate.parentchange():
        dirstate.rebuild(node, m2, changedfiles)


def _filectxfn(repo, mctx, path, files_dict):
    file_info = files_dict[path]
    if file_info is None:
        return None
    else:
        if "data" in file_info:
            data = file_info["data"].encode("utf-8")
        else:
            data = base64.b85decode(file_info["dataBase85"])
        copied = file_info.get("copyFrom")
        flags = file_info.get("flags", "")
        return context.memfilectx(
            repo,
            mctx,
            path,
            data,
            islink="l" in flags,
            isexec="x" in flags,
            copied=copied,
        )
