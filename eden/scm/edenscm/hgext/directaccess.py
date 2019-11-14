# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

""" This extension provides direct access

It is the ability to refer and access hidden sha in commands provided that you
know their value.
For example hg log -r xxx where xxx is a commit has should work whether xxx is
hidden or not as we assume that the user knows what he is doing when referring
to xxx.
"""

from edenscm.mercurial import (
    branchmap,
    commands,
    error,
    extensions,
    hg,
    registrar,
    repoview,
    revset,
    util,
)
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem("directaccess", "loadsafter", default=[])

# By default, all the commands have directaccess with warnings
# List of commands that have no directaccess and directaccess with no warning
directaccesslevel = [
    # Format:
    # ('nowarning', 'evolve', 'prune'),
    # means: no directaccess warning, for the command in evolve named prune
    #
    # ('error', None, 'serve'),
    # means: no directaccess for the command in core named serve
    #
    # The list is ordered alphabetically by command names, starting with all
    # the commands in core then all the commands in the extensions
    #
    # The general guideline is:
    # - remove directaccess warnings for read only commands
    # - no direct access for commands with consequences outside of the repo
    # - leave directaccess warnings for all the other commands
    #
    ("nowarning", None, "annotate"),
    ("nowarning", None, "archive"),
    ("nowarning", None, "bisect"),
    ("nowarning", None, "bookmarks"),
    ("nowarning", None, "bundle"),
    ("nowarning", None, "cat"),
    ("nowarning", None, "diff"),
    ("nowarning", None, "export"),
    ("nowarning", None, "identify"),
    ("nowarning", None, "incoming"),
    ("nowarning", None, "log"),
    ("nowarning", None, "manifest"),
    ("error", None, "outgoing"),  # confusing if push errors and not outgoing
    ("error", None, "push"),  # destructive
    ("nowarning", None, "revert"),
    ("error", None, "serve"),
    ("nowarning", None, "tags"),
    ("nowarning", None, "unbundle"),
    ("nowarning", None, "update"),
]


def reposetup(ui, repo):
    repo._explicitaccess = set()


def _computehidden(repo):
    hidden = repoview.filterrevs(repo, "visible")
    dynamic = hidden & repo._explicitaccess
    if dynamic:
        unfi = repo.unfiltered()
        # Explicitly disable revnum deprecation warnings.
        with repo.ui.configoverride({("devel", "legacy.revnum:real"): ""}):
            blocked = set(unfi.revs("(not public()) & ::%ld", dynamic))
        hidden -= blocked
    return hidden


def setupdirectaccess():
    """ Add two new filtername that behave like visible to provide direct access
    and direct access with warning. Wraps the commands to setup direct access
    """
    repoview.filtertable.update({"visible-directaccess-nowarn": _computehidden})
    repoview.filtertable.update({"visible-directaccess-warn": _computehidden})

    for warn, ext, cmd in directaccesslevel:
        try:
            cmdtable = extensions.find(ext).cmdtable if ext else commands.table
            wrapper = wrapwitherror if warn == "error" else wrapwithoutwarning
            extensions.wrapcommand(cmdtable, cmd, wrapper)
        except (error.UnknownCommand, KeyError):
            pass


def wrapwitherror(orig, ui, repo, *args, **kwargs):
    if repo and repo.filtername == "visible-directaccess-warn":
        repo = repo.filtered("visible")
    return orig(ui, repo, *args, **kwargs)


def wrapwithoutwarning(orig, ui, repo, *args, **kwargs):
    if repo and repo.filtername == "visible-directaccess-warn":
        repo = repo.filtered("visible-directaccess-nowarn")
    return orig(ui, repo, *args, **kwargs)


def uisetup(ui):
    """ Change ordering of extensions to ensure that directaccess extsetup comes
    after the one of the extensions in the loadsafter list """
    # No need to enable directaccess if narrow-heads is enabled.
    if ui.configbool("experimental", "narrow-heads"):
        return
    # internal config: directaccess.loadsafter
    loadsafter = ui.configlist("directaccess", "loadsafter")
    order = list(extensions._order)
    directaccesidx = order.index("directaccess")

    # The min idx for directaccess to load after all the extensions in loadafter
    minidxdirectaccess = directaccesidx

    for ext in loadsafter:
        try:
            minidxdirectaccess = max(minidxdirectaccess, order.index(ext))
        except ValueError:
            pass  # extension not loaded

    if minidxdirectaccess > directaccesidx:
        order.insert(minidxdirectaccess + 1, "directaccess")
        order.remove("directaccess")
        extensions._order = order


def _repository(orig, *args, **kwargs):
    """Make visible-directaccess-warn the default filter for new repos"""
    repo = orig(*args, **kwargs)
    return repo.filtered("visible-directaccess-warn")


def extsetup(ui):
    # No need to enable directaccess if narrow-heads is enabled.
    if ui.configbool("experimental", "narrow-heads"):
        return
    extensions.wrapfunction(revset, "posttreebuilthook", _posttreebuilthook)
    extensions.wrapfunction(hg, "repository", _repository)
    setupdirectaccess()


hashre = util.re.compile("[0-9a-fA-F]{1,40}")

_listtuple = ("symbol", "_list")


def _ishashsymbol(symbol, maxrev):
    # Returns true if symbol looks like a hash
    try:
        n = int(symbol)
        if n <= maxrev:
            # It's a rev number
            return False
    except ValueError:
        pass
    return hashre.match(symbol)


def gethashsymbols(tree, maxrev):
    # Returns the list of symbols of the tree that look like hashes
    # for example for the revset 3::abe3ff it will return ('abe3ff')
    if not tree:
        return []

    results = []
    if len(tree) == 2 and tree[0] in {"symbol", "string"}:
        results.append(tree[1])
    elif tree[0] == "func" and tree[1] == _listtuple:
        # the optimiser will group sequence of hash request
        results += tree[2][1].split("\0")
    else:
        for subtree in tree[1:]:
            results += gethashsymbols(subtree, maxrev)
        # return directly, we don't need to filter symbols again
        return results
    return [s for s in results if _ishashsymbol(s, maxrev)]


def _posttreebuilthook(orig, tree, repo):
    # This is use to enabled direct hash access
    # We extract the symbols that look like hashes and add them to the
    # explicitaccess set
    orig(tree, repo)
    filternm = ""
    if repo is not None:
        filternm = repo.filtername
    if filternm is not None and filternm.startswith("visible-directaccess"):
        prelength = len(repo._explicitaccess)
        accessbefore = set(repo._explicitaccess)
        cl = repo.unfiltered().changelog
        repo.symbols = gethashsymbols(tree, len(cl))
        for node in repo.symbols:
            try:
                node = cl._partialmatch(node)
            except error.LookupError:
                node = None
            if node is not None:
                rev = cl.rev(node)
                if rev not in repo.changelog:
                    repo._explicitaccess.add(rev)
        if prelength != len(repo._explicitaccess):
            if repo.filtername != "visible-directaccess-nowarn":
                unhiddencommits = repo._explicitaccess - accessbefore
                repo.ui.warn(
                    _(
                        "Warning: accessing hidden changesets %s "
                        "for write operation\n"
                    )
                    % (",".join([str(repo.unfiltered()[l]) for l in unhiddencommits]))
                )
            repo.invalidatevolatilesets()
