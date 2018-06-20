# __init__.py - Watchman client for the fsmonitor extension
#
# Copyright 2013-2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import getpass

from mercurial import progress, util
from mercurial.node import hex

from .. import pywatchman


def createclientforrepo(repo):
    """Creates a Watchman client and associates it with the repo if it does
    not already have one. Note that creating the client may raise an exception.

    To get the client associated with the repo, use getclientforrepo()."""
    if not util.safehasattr(repo, "_watchmanclient"):
        repo._watchmanclient = client(repo)


def getclientforrepo(repo):
    """Returns the Watchman client associated with the repo or None.

    createclientforrepo() must have be called previously to create the
    client."""
    if util.safehasattr(repo, "_watchmanclient"):
        return repo._watchmanclient
    else:
        return None


class Unavailable(Exception):
    def __init__(self, msg, warn=True, invalidate=False):
        self.msg = msg
        self.warn = warn
        if self.msg == "timed out waiting for response":
            self.warn = False
        self.invalidate = invalidate

    def __str__(self):
        if self.warn:
            return "warning: Watchman unavailable: %s" % self.msg
        else:
            return "Watchman unavailable: %s" % self.msg


class WatchmanNoRoot(Unavailable):
    def __init__(self, root, msg):
        self.root = root
        super(WatchmanNoRoot, self).__init__(msg)


class client(object):
    def __init__(self, repo, timeout=1.0):
        err = None
        if not self._user:
            err = "couldn't get user"
            warn = True
        if self._user in repo.ui.configlist("fsmonitor", "blacklistusers"):
            err = "user %s in blacklist" % self._user
            warn = False

        if err:
            raise Unavailable(err, warn)

        self._timeout = timeout
        self._watchmanclient = None
        self._root = repo.root
        self._ui = repo.ui
        self._firsttime = True

    def settimeout(self, timeout):
        self._timeout = timeout
        if self._watchmanclient is not None:
            self._watchmanclient.setTimeout(timeout)

    def getcurrentclock(self):
        result = self.command("clock")
        if not util.safehasattr(result, "clock"):
            raise Unavailable("clock result is missing clock value", invalidate=True)
        return result.clock

    def clearconnection(self):
        self._watchmanclient = None

    def available(self):
        return self._watchmanclient is not None or self._firsttime

    @util.propertycache
    def _user(self):
        try:
            return getpass.getuser()
        except KeyError:
            # couldn't figure out our user
            return None

    def _command(self, *args):
        watchmanargs = (args[0], self._root) + args[1:]
        self._ui.log(
            "watchman-command",
            "watchman command %r started with timeout = %s",
            watchmanargs,
            self._timeout,
        )
        try:
            if self._watchmanclient is None:
                self._firsttime = False
                self._watchmanclient = pywatchman.client(
                    timeout=self._timeout, useImmutableBser=True
                )
            result = self._watchmanclient.query(*watchmanargs)
            self._ui.log(
                "watchman-command", "watchman command %r completed", watchmanargs
            )
            return result
        except pywatchman.CommandError as ex:
            if "unable to resolve root" in ex.msg:
                raise WatchmanNoRoot(self._root, ex.msg)
            raise Unavailable(ex.msg)
        except pywatchman.WatchmanError as ex:
            raise Unavailable(str(ex))

    @util.timefunction("watchmanquery", 0, "_ui")
    def command(self, *args):
        with progress.spinner(self._ui, "querying watchman"):
            try:
                try:
                    return self._command(*args)
                except WatchmanNoRoot:
                    # this 'watch' command can also raise a WatchmanNoRoot if
                    # watchman refuses to accept this root
                    self._command("watch")
                    return self._command(*args)
            except Unavailable:
                # this is in an outer scope to catch Unavailable form any of the
                # above _command calls
                self._watchmanclient = None
                raise


# Estimate the distance between two nodes
def calcdistance(repo, oldnode, newnode):
    anc = repo.changelog.ancestor(oldnode, newnode)
    ancrev = repo[anc].rev()
    distance = abs(repo[oldnode].rev() - ancrev) + abs(repo[newnode].rev() - ancrev)
    return distance


class state_update(object):
    """ This context manager is responsible for dispatching the state-enter
        and state-leave signals to the watchman service. The enter and leave
        methods can be invoked manually (for scenarios where context manager
        semantics are not possible). If parameters oldnode and newnode are None,
        they will be populated based on current working copy in enter and
        leave, respectively. Similarly, if the distance is none, it will be
        calculated based on the oldnode and newnode in the leave method."""

    def __init__(
        self,
        repo,
        name,
        oldnode=None,
        newnode=None,
        distance=None,
        partial=False,
        metadata=None,
    ):
        self.repo = repo.unfiltered()
        self.name = name
        self.oldnode = oldnode
        self.newnode = newnode
        self.distance = distance
        self.partial = partial
        self._lock = None
        self.need_leave = False
        self.metadata = metadata or {}

    def __enter__(self):
        self.enter()

    def enter(self):
        # Make sure we have a wlock prior to sending notifications to watchman.
        # We don't want to race with other actors. In the update case,
        # merge.update is going to take the wlock almost immediately. We are
        # effectively extending the lock around several short sanity checks.
        if self.oldnode is None:
            self.oldnode = self.repo["."].node()

        if self.repo.currentwlock() is None:
            if util.safehasattr(self.repo, "wlocknostateupdate"):
                self._lock = self.repo.wlocknostateupdate()
            else:
                self._lock = self.repo.wlock()
        self.need_leave = self._state("state-enter", hex(self.oldnode))
        return self

    def __exit__(self, type_, value, tb):
        abort = True if type_ else False
        self.exit(abort=abort)

    def exit(self, abort=False):
        try:
            if self.need_leave:
                status = "failed" if abort else "ok"
                if self.newnode is None:
                    self.newnode = self.repo["."].node()
                if self.distance is None:
                    try:
                        self.distance = calcdistance(
                            self.repo, self.oldnode, self.newnode
                        )
                    except Exception:
                        # this happens in complex cases where oldnode
                        # or newnode might become unavailable.
                        pass
                self._state("state-leave", hex(self.newnode), status=status)
        finally:
            self.need_leave = False
            if self._lock:
                self._lock.release()

    def _state(self, cmd, commithash, status="ok"):
        client = getclientforrepo(self.repo)
        if not client:
            return False
        try:
            metadata = {
                # the target revision
                "rev": commithash,
                # approximate number of commits between current and target
                "distance": self.distance if self.distance else 0,
                # success/failure (only really meaningful for state-leave)
                "status": status,
                # whether the working copy parent is changing
                "partial": self.partial,
            }
            metadata.update(self.metadata)
            client.command(cmd, {"name": self.name, "metadata": metadata})
            return True
        except Exception as e:
            # Swallow any errors; fire and forget
            self.repo.ui.log("watchman", "Exception %s while running %s\n", e, cmd)
            return False
