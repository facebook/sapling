# __init__.py - hgevents
#
# Copyright 2013-2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""publishes state-enter and state-leave events to Watchman

Extension that is responsible for publishing state-enter and state-leave
events to Watchman for the following states:

- hg.filemerge
- hg.update

This was originally part of the fsmonitor extension, but it was split into its
own extension that can be used with Eden. (Note that fsmonitor is supposed to be
disabled when 'eden' is in repo.requirements.)

Note that hg.update state changes must be published to Watchman in order for it
to support SCM-aware subscriptions:
https://facebook.github.io/watchman/docs/scm-query.html.
"""

from __future__ import absolute_import

from edenscm.mercurial import extensions, filemerge, merge, perftrace, registrar
from edenscm.mercurial.i18n import _

from ..extlib import watchmanclient


configtable = {}
configitem = registrar.configitem(configtable)

configitem("experimental", "fsmonitor.transaction_notify", default=False)

# This extension is incompatible with the following blacklisted extensions
# and will disable itself when encountering one of these:
_blacklist = ["largefiles", "eol"]


def extsetup(ui):
    extensions.wrapfunction(merge, "update", wrapupdate)
    extensions.wrapfunction(filemerge, "_xmerge", _xmerge)


def reposetup(ui, repo):
    exts = extensions.enabled()
    for ext in _blacklist:
        if ext in exts:
            ui.warn(
                _(
                    "The hgevents extension is incompatible with the %s "
                    "extension and has been disabled.\n"
                )
                % ext
            )
            return

    if not repo.local():
        return

    # Ensure there is a Watchman client associated with the repo that
    # state_update() can use later.
    try:
        watchmanclient.createclientforrepo(repo)
    except Exception as ex:
        ui.log("hgevents", "Watchman exception: %s\n", ex)
        return

    class hgeventsrepo(repo.__class__):
        def wlocknostateupdate(self, *args, **kwargs):
            return super(hgeventsrepo, self).wlock(*args, **kwargs)

        def wlock(self, *args, **kwargs):
            l = super(hgeventsrepo, self).wlock(*args, **kwargs)
            if not self.ui.configbool("experimental", "fsmonitor.transaction_notify"):
                return l
            if l.held != 1:
                return l
            origrelease = l.releasefn

            def staterelease():
                if origrelease:
                    origrelease()
                if l.stateupdate:
                    with perftrace.trace("Watchman State Exit"):
                        l.stateupdate.exit()
                    l.stateupdate = None

            try:
                l.stateupdate = None
                l.stateupdate = watchmanclient.state_update(self, name="hg.transaction")
                with perftrace.trace("Watchman State Enter"):
                    l.stateupdate.enter()
                l.releasefn = staterelease
            except Exception as e:
                # Swallow any errors; fire and forget
                self.ui.log("watchman", "Exception in state update %s\n", e)
            return l

    repo.__class__ = hgeventsrepo


# Bracket working copy updates with calls to the watchman state-enter
# and state-leave commands.  This allows clients to perform more intelligent
# settling during bulk file change scenarios
# https://facebook.github.io/watchman/docs/cmd/subscribe.html#advanced-settling
def wrapupdate(
    orig,
    repo,
    node,
    branchmerge,
    force,
    ancestor=None,
    mergeancestor=False,
    labels=None,
    matcher=None,
    wc=None,
    **kwargs
):
    if wc and wc.isinmemory():
        # If the working context isn't on disk, there's no need to invoke
        # watchman.
        return orig(
            repo,
            node,
            branchmerge,
            force,
            ancestor,
            mergeancestor,
            labels,
            matcher,
            wc=wc,
            **kwargs
        )
    distance = 0
    partial = True
    oldnode = repo["."].node()
    newnode = repo[node].node()
    if matcher is None or matcher.always():
        partial = False
        distance = watchmanclient.calcdistance(repo.unfiltered(), oldnode, newnode)

    with watchmanclient.state_update(
        repo,
        name="hg.update",
        oldnode=oldnode,
        newnode=newnode,
        distance=distance,
        partial=partial,
        metadata={"merge": branchmerge},
    ):
        return orig(
            repo,
            node,
            branchmerge,
            force,
            ancestor,
            mergeancestor,
            labels,
            matcher,
            **kwargs
        )


def _xmerge(origfunc, repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    # _xmerge is called when an external merge tool is invoked.
    with state_filemerge(repo, fcd.path()):
        return origfunc(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels)


class state_filemerge(object):
    """Context manager for single filemerge event"""

    def __init__(self, repo, path):
        self.repo = repo
        self.path = path

    def __enter__(self):
        self._state("state-enter")

    def __exit__(self, errtype, value, tb):
        self._state("state-leave")

    def _state(self, name):
        client = getattr(self.repo, "_watchmanclient", None)
        if client:
            metadata = {"path": self.path}
            try:
                client.command(name, {"name": "hg.filemerge", "metadata": metadata})
            except Exception:
                # State notifications are advisory only, and so errors
                # don't block us from performing a checkout
                pass
