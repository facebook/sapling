# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
introduces `--tool=internal:dumpjson` to `resolve` to output conflict info

N.B.: This is an extension-ized version but we hope to land this upstream and
then delete this extension.

Normally, `hg resolve` takes the user on a whistle-stop tour of each conflicted
file, pausing to launch the editor to resolve the conflicts. This is an
alternative workflow for resolving many conflicts in a random-access fashion. It
doesn't change/replace the default behavior.

This commit adds `--tool=internal:dumpjson`. It prints, for each conflict, the
"base", "other", and "ours" versions (the contents, as well as their exec/link
flags), and where the user/tool should write a resolved version (i.e., the
working copy) as JSON. The user will then resolve the conflicts at their leisure
and run `hg resolve --mark`.
"""

from __future__ import absolute_import

import copy
from typing import Any, Dict, Optional, Union

from edenscm import (
    commands,
    error,
    extensions,
    merge as mergemod,
    pycompat,
    scmutil,
    util,
)
from edenscm.filemerge import absentfilectx
from edenscm.i18n import _
from edenscm.node import bin


testedwith = "ships-with-fb-ext"

# `unfinishedstates` would be ideal for this except it does not include merge,
# and doesn't expose the command to run to resume by itself (it instead exposes
# a help string)
# Note: order matters (consider rebase v. merge).
CONFLICTSTATES = [
    [
        "graftstate",
        {
            "cmd": "graft",
            "to_continue": "graft --continue",
            "to_abort": "graft --abort",
        },
    ],
    [
        "rebasestate",
        {
            "cmd": "rebase",
            "to_continue": "rebase --continue",
            "to_abort": "rebase --abort",
        },
    ],
    [
        "shelvedstate",
        {
            "cmd": "unshelve",
            "to_continue": "unshelve --continue",
            "to_abort": "unshelve --abort",
        },
    ],
    [
        "histedit-state",
        {
            "cmd": "histedit",
            "to_continue": "histedit --continue",
            "to_abort": "histedit --abort",
        },
    ],
    # updatestate should be after all other commands, but before mergestate,
    # since some of the above commands run updates which concievably could be
    # interrupted. See coment for mergestate.
    [
        "updatestate",
        {"cmd": "update", "to_continue": "update", "to_abort": "goto --clean"},
    ],
    # Check for mergestate last, since other commands (shelve, rebase, histedit,
    # etc.) will leave a statefile of their own, as well as a mergestate, if
    # there were conflicts. The command-level statefile should be considered
    # first.
    [
        "merge/state",
        {
            "cmd": "merge",
            "to_continue": "merge --continue",
            "to_abort": "goto --clean",
        },
    ],
]


def extsetup(ui) -> None:
    extensions.wrapcommand(commands.table, "resolve", _resolve)


# Returns which command got us into the conflicted merge state. Since these
# states are mutually exclusive, we can use the existence of any one statefile
# as proof of culpability.
def _findconflictcommand(repo) -> Union[None, Dict[str, str], str]:
    for path, data in CONFLICTSTATES:
        if repo.localvfs.exists(path):
            return data
    return None


# To become a block in commands.py/resolve().
def _resolve(orig, ui, repo, *pats, **opts):
    # This block is duplicated from commands.py to maintain behavior.
    flaglist = "all mark unmark list no_status".split()
    all, mark, unmark, show, nostatus = [opts.get(o) for o in flaglist]

    if (show and (mark or unmark)) or (mark and unmark):
        raise error.Abort(_("too many options specified"))
    if pats and all:
        raise error.Abort(_("can't specify --all and patterns"))
    if not (all or pats or show or mark or unmark):
        raise error.Abort(
            _("no files or directories specified"),
            hint="use --all to re-merge all unresolved files",
        )
    # </duplication>

    if not show and opts.get("tool", "") == "internal:dumpjson":
        formatter = ui.formatter("resolve", {"template": "json"})
        mergestate = mergemod.mergestate.read(repo)
        matcher = scmutil.match(repo[None], pats, opts)
        workingctx = repo[None]

        fileconflicts = []
        pathconflicts = []
        for path in mergestate:
            if not matcher(path):
                continue

            info = _summarizefileconflicts(mergestate, path, workingctx)
            if info is not None:
                fileconflicts.append(info)

            info = _summarizepathconflicts(mergestate, path)
            if info is not None:
                pathconflicts.append(info)

        cmd = _findconflictcommand(repo)
        formatter.startitem()
        formatter.write("conflicts", "%s\n", fileconflicts)
        formatter.write("pathconflicts", "%s\n", pathconflicts)
        formatter.write("command", "%s\n", _findconflictcommand(repo))
        if cmd:
            formatter.write("command", "%s\n", cmd["cmd"])  # Deprecated
            formatter.write("command_details", "%s\n", cmd)
        else:
            formatter.write("command", "%s\n", None)  # For BC
        formatter.end()
        return 0

    return orig(ui, repo, *pats, **opts)


# To become merge.summarizefileconflicts().
def _summarizefileconflicts(self, path, workingctx):
    # 'd' = driver-resolved
    # 'r' = marked resolved
    # 'pr', 'pu' = path conflicts
    if self[path] in ("d", "r", "pr", "pu"):
        return None

    stateentry = self._state[path]
    localnode = bin(stateentry[1])
    ancestorfile = stateentry[3]
    ancestornode = bin(stateentry[4])
    otherfile = stateentry[5]
    othernode = bin(stateentry[6])
    otherctx = self._repo[self._other]
    extras = self.extras(path)
    anccommitnode = extras.get("ancestorlinknode")
    ancestorctx = self._repo[anccommitnode] if anccommitnode else None
    workingctx = self._filectxorabsent(localnode, workingctx, path)
    otherctx = self._filectxorabsent(othernode, otherctx, otherfile)
    basectx = self._repo.filectx(
        ancestorfile, fileid=ancestornode, changeid=ancestorctx
    )

    return _summarize(self._repo, workingctx, otherctx, basectx)


# To become merge.summarizepathconflicts().
def _summarizepathconflicts(self, path) -> Optional[Dict[str, Any]]:
    # 'pu' = unresolved path conflict
    if self[path] != "pu":
        return None

    stateentry = self._state[path]
    frename = stateentry[1]
    forigin = stateentry[2]
    return {
        "path": path,
        "fileorigin": "local" if forigin == "l" else "remote",
        "renamedto": frename,
    }


# To become filemerge.summarize().
def _summarize(repo, workingfilectx, otherctx, basectx) -> Dict[str, Any]:
    origfile = (
        None
        if workingfilectx.isabsent()
        else scmutil.origpath(repo.ui, repo, repo.wjoin(workingfilectx.path()))
    )

    def flags(context):
        if isinstance(context, absentfilectx):
            return {
                "contents": None,
                "exists": False,
                "isexec": None,
                "issymlink": None,
            }
        return {
            "contents": pycompat.decodeutf8(context.data()),
            "exists": True,
            "isexec": context.isexec(),
            "issymlink": context.islink(),
        }

    output = flags(workingfilectx)

    filestat = util.filestat.frompath(origfile) if origfile is not None else None
    if origfile and filestat.stat:
        # Since you can start a merge with a dirty working copy (either via
        # `up` or `merge -f`), "local" must reflect that, not the underlying
        # changeset. Those contents are available in the .orig version, so we
        # look there and mock up the schema to look like the other contexts.
        #
        # Test cases affected in test-merge-conflict-cornercases.t: #0
        local = {
            "contents": pycompat.decodeutf8(util.readfile(origfile)),
            "exists": True,
            "isexec": util.isexec(origfile),
            "issymlink": util.statislink(filestat.stat),
        }
    else:
        # No backup file. This happens whenever the merge was esoteric enough
        # that we didn't launch a merge tool*, and instead prompted the user to
        # "use (c)hanged version, (d)elete, or leave (u)nresolved".
        #
        # The only way to exit that prompt with a conflict is to choose "u",
        # which leaves the local version in the working copy (with all its
        # pre-merge properties including any local changes), so we can reuse
        # that.
        #
        # Affected test cases: #0b, #1, #6, #11, and #12.
        #
        # Another alternative might be to use repo['.'][path] but that wouldn't
        # have any dirty pre-merge changes.
        #
        # *If we had, we'd've we would've overwritten the working copy, made a
        # backup and hit the above case.
        #
        # Copy, so the addition of the `path` key below does not affect both
        # versions.
        local = copy.copy(output)

    output["path"] = repo.wjoin(workingfilectx.path())

    return {
        "base": flags(basectx),
        "local": local,
        "other": flags(otherctx),
        "output": output,
        "path": workingfilectx.path(),
    }
