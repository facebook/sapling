# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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

import contextlib

from bindings import edenfsassertedstates
from sapling import extensions, filemerge, merge, node as nodemod, perftrace, util
from sapling.i18n import _

from ..extlib import watchmanclient

# This extension is incompatible with the following extensions
# and will disable itself when encountering one of these:
_incompatible_exts = ["largefiles", "eol"]


def extsetup(ui):
    extensions.wrapfunction(merge, "goto", wrapgoto)
    extensions.wrapfunction(merge, "merge", wrapmerge)
    extensions.wrapfunction(filemerge, "_xmerge", _xmerge)


def reposetup(ui, repo):
    exts = extensions.enabled()
    for ext in _incompatible_exts:
        if ext in exts:
            ui.warn(
                _(
                    "The hgevents extension is incompatible with the %s "
                    "extension and has been disabled.\n"
                )
                % ext
            )
            return

    # Ensure there is a Watchman client associated with the repo that
    # state_update() can use later.
    watchmanclient.createclientforrepo(repo)
    if (
        "eden" in repo.requirements
        and ui.configbool("experimental", "enable-edenfs-asserted-states")
        and not hasattr(repo, "_asserted_states_client")
    ):
        repo._asserted_states_client = edenfsassertedstates.AssertedStatesClient(
            repo.root
        )

    class hgeventsrepo(repo.__class__):
        def wlocknostateupdate(self, *args, **kwargs):
            return super(hgeventsrepo, self).wlock(*args, **kwargs)

        def wlock(self, *args, **kwargs):
            l = super(hgeventsrepo, self).wlock(*args, **kwargs)
            if isinstance(l, util.nullcontextmanager):
                return l
            if not self._eventreporting:
                return l
            if not self.ui.configbool("experimental", "fsmonitor.transaction_notify"):
                return l
            if l.held != 1:
                return l

            try:
                # We don't need to send hg.transaction event if we have no working copy.
                # hg.transaction is useful as an umbrella state to debounce a busy "rebase", but if
                # we have no working copy, the only interesting action is "goto" (which will still
                # produce an "hg.update" state).
                if self.dirstate.p1() == nodemod.nullid:
                    return l
            except Exception:
                # Ignore errors loading dirstate.
                pass

            origrelease = l.releasefn

            def staterelease():
                if origrelease:
                    origrelease()
                if l.stateupdate:
                    with perftrace.trace("Watchman State Exit"):
                        l.stateupdate.exit()
                    l.stateupdate = None
                if hasattr(l, "edenfs_asserted_state"):
                    del l.edenfs_asserted_state

            try:
                l.stateupdate = None
                l.stateupdate = watchmanclient.state_update(self, name="hg.transaction")
                l.edenfs_asserted_state = None
                if "eden" in repo.requirements and self.ui.configbool(
                    "experimental", "enable-edenfs-asserted-states"
                ):
                    l.edenfs_asserted_state = (
                        repo._asserted_states_client.enter_state_with_deadline(
                            "hg.transaction",
                            self.ui.configwith(
                                str, "edenfs", "eden-state-timeout", default="1s"
                            ),
                            self.ui.configwith(
                                str, "devel", "lock_backoff", default="0.1s"
                            ),
                        )
                    )
                with perftrace.trace("Watchman State Enter"):
                    l.stateupdate.enter()
                l.releasefn = staterelease
            except Exception:
                # Swallow any errors; fire and forget
                pass
            return l

    repo.__class__ = hgeventsrepo


# Bracket working copy updates with calls to the watchman state-enter
# and state-leave commands.  This allows clients to perform more intelligent
# settling during bulk file change scenarios
# https://facebook.github.io/watchman/docs/cmd/subscribe.html#advanced-settling
def wrapmerge(
    orig,
    to_repo,
    node,
    wc=None,
    from_repo=None,
    **kwargs,
):
    from_repo = from_repo or to_repo
    is_crossrepo = not to_repo.is_same_repo(from_repo)
    if (wc and wc.isinmemory()) or is_crossrepo:
        # Skip Watchman integration in the following cases:
        # - The working context (wc) is not on disk.
        # - This is a cross-repo merge, where computing path distance may not
        #   be meaningful.
        return orig(
            to_repo,
            node,
            wc=wc,
            from_repo=from_repo,
            **kwargs,
        )
    distance = 0
    oldnode = to_repo["."].node()
    newnode = to_repo[node].node()
    distance = watchmanclient.calcdistance(to_repo, oldnode, newnode)
    if (
        "eden" in to_repo.requirements
        and to_repo.ui.configbool("experimental", "enable-edenfs-asserted-states")
        and hasattr(to_repo, "_asserted_states_client")
    ):
        try:
            cm = to_repo._asserted_states_client.enter_state_with_deadline(
                "hg.update",
                to_repo.ui.configwith(
                    str, "edenfs", "eden-state-timeout", default="1s"
                ),
                to_repo.ui.configwith(str, "devel", "lock_backoff", default="0.1s"),
            )
        except:
            # For now, log and ignore errors.
            to_repo.ui.warn("Failed to set edenfs notifications state 'hg.update'")
            cm = contextlib.nullcontext()
    else:
        cm = contextlib.nullcontext()

    with watchmanclient.state_update(
        to_repo,
        name="hg.update",
        oldnode=oldnode,
        newnode=newnode,
        distance=distance,
        metadata={"merge": True},
    ):
        with cm:
            return orig(
                to_repo,
                node,
                wc=wc,
                from_repo=from_repo,
                **kwargs,
            )


def wrapgoto(
    orig,
    repo,
    node,
    force=False,
    updatecheck=None,
    **kwargs,
):
    distance = 0
    oldnode = repo["."].node()
    newnode = repo[node].node()
    distance = watchmanclient.calcdistance(repo, oldnode, newnode)
    if (
        "eden" in repo.requirements
        and repo.ui.configbool("experimental", "enable-edenfs-asserted-states")
        and hasattr(repo, "_asserted_states_client")
    ):
        try:
            cm = repo._asserted_states_client.enter_state_with_deadline(
                "hg.update",
                repo.ui.configwith(str, "edenfs", "eden-state-timeout", default="1s"),
                repo.ui.configwith(str, "devel", "lock_backoff", default="0.1s"),
            )
        except:
            # For now, log and ignore errors.
            repo.ui.warn("Failed to set edenfs notifications state 'hg.update'")
            cm = contextlib.nullcontext()
    else:
        cm = contextlib.nullcontext()

    with watchmanclient.state_update(
        repo,
        name="hg.update",
        oldnode=oldnode,
        newnode=newnode,
        distance=distance,
        metadata={"merge": False},
    ):
        with cm:
            return orig(repo, node, force=force, updatecheck=updatecheck, **kwargs)


def _xmerge(origfunc, repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    # _xmerge is called when an external merge tool is invoked.
    with state_filemerge(repo, fcd.path()):
        return origfunc(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels)


class state_filemerge:
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
