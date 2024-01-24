# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import base64, collections, functools, stat

import bindings

from .. import context, hg, json, mutation, scmutil, smartset, visibility
from ..i18n import _
from ..node import bin, hex, nullid, wdirhex, wdirrev
from .cmdtable import command


@command(
    "debugexportstack",
    [
        ("r", "rev", [], _("revisions to export")),
        ("", "assume-tracked", [], _("extra file paths in wdir to export")),
    ],
)
def debugexportstack(ui, repo, **opts):
    """dump content of commits for automation consumption

    On success, print a list of commit information encoded in JSON, like::

        [{
          // Hex commit hash.
          // "ffffffffffffffffffffffffffffffffffffffff" means the working copy.
          "node": "1f0eaeff1a01e2594efedceefac9987cda047204",
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

              // Content "reference" representation if the file is too large.
              "dataRef": {"node": hex_hash, "path": path},

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

    Configs::

        # Maximum bytes of a file to export. Exceeding the limit will turn the
        # file to "ref" representation.
        experimental.exportstack-max-bytes = 1M
    """
    # size limits
    extra_tracked = opts.get("assume_tracked")
    revs = scmutil.revrange(repo, opts.get("rev"))
    revs.sort()
    obj = _export(repo, revs, extra_tracked=extra_tracked)
    ui.write("%s\n" % json.dumps(obj))


def _export(repo, revs, max_bytes=None, extra_tracked=None):
    if max_bytes is None:
        max_bytes = repo.ui.configbytes("experimental", "exportstack-max-bytes")

    # Figure out relevant commits and files:
    # - If rev X modifies or deleted (not "added") a file, then the file in
    #   parents(X) is relevant.
    # - If rev X contains a "copy from (rev Y, path Z)" file, then path Z in
    #   rev Y is relevant.

    # {node: {path: file}}
    relevant_map = collections.defaultdict(dict)
    requested_map = collections.defaultdict(dict)

    for ctx in revs.prefetch("text").iterctx():
        pctx = ctx.p1()
        files = requested_map[ctx.node()]
        changed_files = ctx.files()
        if ctx.node() is None:
            # Consider other (untracked) files in wdir() defined by
            # status.force-tracked.
            if extra_tracked:
                existing_tracked = set(changed_files)
                for path in extra_tracked:
                    if path not in existing_tracked:
                        changed_files.append(path)
        for path in changed_files:
            parent_paths = [path]
            file_obj = _file_obj(ctx, path, parent_paths.append, max_bytes=max_bytes)
            files[path] = file_obj
            for pctx in ctx.parents():
                pfiles = relevant_map[pctx.node()]
                for ppath in parent_paths:
                    # Skip relevant_map if included by requested_map.
                    if ppath not in requested_map.get(pctx.node(), {}):
                        pfiles[ppath] = _file_obj(pctx, ppath, max_bytes=max_bytes)

    # Put together final result
    result = []
    public_nodes = repo.dageval(lambda: public())

    # Handle virtual commit wdir().
    # Ideally we insert a "wdir()" when loading the Rust dag, and ensure that
    # "wdir()" never gets flushed. For now the Rust dag does not have "wdir()"
    # and we have to special case it.
    non_virtual_revs = revs
    has_wdir_rev = wdirrev in non_virtual_revs
    if has_wdir_rev:
        non_virtual_revs -= smartset.baseset([wdirrev], repo=repo)

    nodes = list(
        repo.dageval(
            lambda: sort(tonodes(non_virtual_revs) | list(relevant_map))
        ).iterrev()
    )

    if has_wdir_rev:
        nodes.append(None)

    for node in nodes:
        ctx = repo[node]
        requested = node in requested_map
        commit_obj = {
            "node": node and hex(node) or wdirhex,
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

    return result


def _size_limiter(limit, error_message):
    limit = limit or 0
    current = 0

    def increase(size):
        nonlocal current
        current += size
        if limit > 0 and current > limit:
            raise ValueError(error_message)

    return increase


def _file_obj(ctx, path, set_copy_from=None, max_bytes=None):
    if ctx.node() is None:
        # For the working copy, use wvfs directly.
        # This allows exporting untracked files and properly report deleting
        # files via --assume-tracked, without running `addremove` first.
        fctx = context.workingfilectx(ctx.repo(), path)
        if not fctx.lexists():
            fctx = None
    elif path in ctx:
        fctx = ctx[path]
    else:
        fctx = None
    if fctx is not None:
        renamed = fctx.renamed()
        copy_from_path = None
        if renamed and renamed[0] != path:
            copy_from_path = renamed[0]
            if set_copy_from is not None:
                set_copy_from(copy_from_path)
        file_obj = {}
        if max_bytes is not None and fctx.size() > max_bytes:
            file_obj["dataRef"] = {"node": ctx.hex(), "path": path}
        else:
            bdata = fctx.data()
            try:
                file_obj["data"] = bdata.decode("utf-8")
            except UnicodeDecodeError:
                file_obj["dataBase85"] = base64.b85encode(bdata).decode()
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
         ["amend", {"node": node, ..commit_info}],
         ["goto", {"mark": mark}],
         ["reset", {"mark": mark}],
         ["hide", {"nodes": [node]}],
         ["write", {path: file_info}]]

    "goto" performs a checkout that will overwrite conflicted files.

    "reset" moves the "current commit" to the given rev without changing
    files in the working copy. It can be useful to implement "absorb" or
    "commit -i". It is similar to the ``reset -kr REV`` command.

    "hide" hides commits if they do not have visible descendants.

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
         // "." means the current working parent, before making any
         // new commits.
         "parents": [node | mark | "."],

         // Predecessors that will be obsoleted. Optional.
         "predecessors": [node | mark],

         // Why predecessor was changed to this commit.
         // Optional. Ignored if predecessors are empty.
         "operation": "amend",

         // File changed by this commit.
         "files": {
           // null: file is deleted by this commit, otherwise
           // added or modified.
           // ".": use file content from the working copy.
           "foo/bar.txt": null | "." | {
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

    "amend" is similar to commit, but it will reuse the old commit's
    messages, parents, files by default. The "files" field will merge
    with (not replace) the old commit's "files". The "node" field is
    required to specify the old commit.

    "write" can be used to write files to the working copy.
    It will be executed after creating commits.

    The format of "commit_info" is similar to the output of
    ``debugexportstack``, with some differences:

    - No ``relevantFiles``.
    - No ``node``. Use ``mark`` instead. ``parents`` can refer to marks.
    - Has ``predecessors``.
    - File can be ``.``, which means reading it from the working copy.
    - ``copyFrom`` can be ``.``, which means reading from the working copy.
    - ``flags`` can be ``.``, which means reading from the working copy,
      or the parent of the working copy, if the file is in "R" or "!"
      status.

    Bookmarks will be moved if they become obsoleted (referred by
    ``predecessors``).

    ``debugimportstack`` supports files to be referred as ".".
    This can be useful to avoid reading file contents first.

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
    try:
        actions = json.loads(ui.fin.read())
    except json.JSONDecodeError as ex:
        obj = {"error": f"commit info is invalid JSON ({ex})"}
    else:
        try:
            obj = _import(repo, actions)
        except (ValueError, TypeError, KeyError, AttributeError) as ex:
            obj = {"error": str(ex)}
    ui.write("%s\n" % json.dumps(obj))
    if obj and "error" in obj:
        return 1


def _import(repo, actions):
    wnode = repo["."].node()
    marks = Marks(wnode)

    with repo.wlock(), repo.lock(), repo.transaction("importstack"):
        # Create commits.
        commit_infos = [action[1] for action in actions if action[0] in "commit"]
        _create_commits(repo, commit_infos, marks)

        # Handle "amend"
        commit_infos = [action[1] for action in actions if action[0] in "amend"]
        _create_commits(repo, commit_infos, marks, amend=True)

        # Handle "goto" or "reset".
        to_hide = []
        for action in actions:
            action_name = action[0]
            if action_name in {"commit", "amend"}:
                # Handled by _create_commits already.
                continue
            elif action_name == "goto":
                node = marks[action[1]["mark"]]
                hg.updaterepo(repo, node, overwrite=True)
            elif action_name == "reset":
                node = marks[action[1]["mark"]]
                _reset(repo, node)
            elif action_name == "hide":
                to_hide += [bin(n) for n in action[1]["nodes"]]
            elif action_name == "write":
                _write_files(repo, action[1])
            else:
                raise ValueError(f"unsupported action: {action}")

        # Handle "hide".
        if to_hide:
            visibility.remove(repo, to_hide)

    return marks.to_hex()


class Marks:
    """Track marks (pending commit hashes)"""

    def __init__(self, wnode):
        self._mark_to_node = {}  # {mark: node}
        self._wnode = wnode

    def to_nodes(self, items):
        """Resolve hex or marks to (binary) nodes"""
        result = []
        for item in items or []:
            if item.startswith(":"):
                node = self._mark_to_node.get(item)
                if not node:
                    raise ValueError(f"cannot resolve mark {item} to node")
                result.append(node)
            elif item == ".":
                node = self._wnode
                if node != nullid:
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


def _create_commits(repo, commit_infos, marks: Marks, amend=False):
    """Create or amend commits based on commit_infos.
    Do not change the working copy.
    Assumes inside a transaction.
    """
    if amend:
        # Merge commit_info with information from the original commit.
        new_commit_infos = []
        for commit_info in commit_infos:
            node = commit_info["node"]
            ctx = repo[node]
            files = commit_info.get("files", {})
            for path in ctx.files():
                if path not in files:
                    files[path] = _file_obj(ctx, path)
            commit_info["files"] = files
            new_commit_info = {
                "author": ctx.user(),
                "text": ctx.description(),
                "parents": [p.hex() for p in ctx.parents()],
                "predecessors": [node],
                "operation": "amend",
                **commit_info,
            }
            new_commit_infos.append(new_commit_info)
        commit_infos = new_commit_infos

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
    elif file_info == ".":
        # Get file from the working copy, instead of wctx.
        # This removes the need to `addremove` files first, avoids errors when
        # reading `!` files (`!` are treated as `R` and `?` are treated as `A`).
        if repo.wvfs.lexists(path):
            return context.workingfilectx(repo, path)
        else:
            return None
    else:
        if "data" in file_info:
            data = file_info["data"].encode("utf-8")
        elif "dataBase85" in file_info:
            data = base64.b85decode(file_info["dataBase85"])
        else:
            data_ref = file_info["dataRef"]
            ref_path = data_ref["path"]
            ctx = repo[data_ref["node"]]
            if ref_path in ctx:
                data = ctx[ref_path].data()
            else:
                return None
        copied = file_info.get("copyFrom")
        flags = file_info.get("flags", "")
        if copied == ".":
            # Read copied from dirstate.
            renamed = repo[None][path].renamed()
            if renamed:
                copied = renamed[0]
            else:
                copied = None
        if flags == ".":
            # Read flags from wdir(), or ".".
            if repo.wvfs.lexists(path):
                flags = repo[None][path].flags()
            else:
                flags = repo["."][path].flags()
        return context.memfilectx(
            repo,
            mctx,
            path,
            data,
            islink="l" in flags,
            isexec="x" in flags,
            copied=copied,
        )


def _write_files(repo, file_infos):
    wvfs = repo.wvfs
    unlinked = set()
    for path, file_info in file_infos.items():
        if file_info is None:
            # Delete this file.
            wvfs.tryunlink(path)
            unlinked.add(path)
        else:
            if file_info == ".":
                # Use the file from the working *parent*.
                ctx = repo["."]
                if path in ctx:
                    fctx = ctx[path]
                    data = fctx.data()
                    flags = fctx.flags()
                else:
                    wvfs.tryunlink(path)
                    unlinked.add(path)
                    continue
            else:
                if "data" in file_info:
                    data = file_info["data"].encode()
                else:
                    data = base64.b85decode(file_info["dataBase85"])
                flags = file_infos.get("flags")
                if flags is None or flags == ".":
                    flags = _existing_flags(wvfs, path)
            wvfs.write(path, data)
            wvfs.setflags(path, l="l" in flags, x="x" in flags)

    # Update dirstate. Forget deleted files, undelete written files.
    with repo.wlock():
        ds = repo.dirstate
        for path in file_infos:
            if path in unlinked:
                # forget
                if ds[path] == "a":
                    ds.untrack(path)
            else:
                # undelete
                if ds[path] == "r":
                    ds.normallookup(path)


def _existing_flags(wvfs, path):
    flags = ""
    try:
        st = wvfs.lstat(path)
        if stat.S_ISLNK(st.st_mode):
            flags = "l"
        elif (st.st_mode & 0o111) != 0:
            flags = "x"
    except FileNotFoundError:
        pass
    return flags


@command(
    "debugimportexport",
    [
        ("", "node-ipc", False, _("use node IPC to communicate messages")),
    ],
)
def debugimportexport(ui, repo, **opts):
    """interactively import and export contents

    This command will read line delimited json from stdin and write results in stdout.
    With ``--node-ipc``, the messages will be written via the nodejs channel instead of
    stdio.

    Supported inputs and their outputs:

        # input: export commits (see debugexportstack); revs will be passed to formatspec
        ["export", {"revs": ["parents(%s)", "."], "assumeTracked": [], "sizeLimit": 100}]
        # => ["ok", debugexportstack result]

        # input: import commits (see debugimportstack)
        ["import", importStackActions]
        # => ["ok", debugimportstack result]

        # input: ping, output: ["ack"]; useful for heartbeat detection
        ["ping"]
        # => ["ok", "ack"]

        # input: exit
        ["exit"]
        # => ["ok", null]
        # side effect: process exit

        # input: others or errors
        ["error", {"message": "unsupported: ..."}]
    """
    if opts.get("node_ipc"):
        ipc = bindings.nodeipc.IPC
        recv = ipc.recv
        send = ipc.send
    else:

        def recv():
            line = ui.fin.readline()
            if line is None:
                return line
            return json.loads(line.decode())

        def send(obj):
            line = json.dumps(obj) + "\n"
            ui.write(line)
            ui.flush()

    while (req := recv()) is not None:
        name = req[0]
        try:
            if name == "export":
                revs = repo.revs(*req[1]["revs"])
                max_bytes = req[1].get("sizeLimit")
                extra_tracked = req[1].get("assumeTracked")
                res = _export(repo, revs, max_bytes, extra_tracked)
            elif name == "import":
                actions = req[1]
                res = _import(repo, actions)
            elif name == "ping":
                res = "ack"
            elif name == "exit":
                res = None
            else:
                raise ValueError(f"unsupported {name}")
        except Exception as ex:
            send(["error", {"message": str(ex)}])
        else:
            send(["ok", res])
        if name == "exit":
            break
