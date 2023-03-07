# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import base64, collections

from .. import json, scmutil
from ..i18n import _
from ..node import hex
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
              "data_base85": "base85 encoded data",

              // Present if the file is copied from "path/from".
              // The content of "path/from" will be included in "relevant_map"
              // of the parent commits.
              "copy_from": "path/from",

              // "l": symlink; "x": executable.
              "flags": "",
            },
            ...
          },

          // Relevant files for diff rendering purpose.
          // "files" render as the "after" state, "relevant_map" (in parent
          // commits) render as the "before" state. If the file is already
          // included in "files" of the parent commit, then it will be skipped
          // in "relevant_files"
          "relevant_files": {
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
            commit_obj["relevant_files"] = relevant_files
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
            file_obj["data_base85"] = base64.b85encode(bdata)
        if copy_from_path:
            file_obj["copy_from"] = copy_from_path
        flags = fctx.flags()
        if flags:
            file_obj["flags"] = flags
    else:
        file_obj = None
    return file_obj
