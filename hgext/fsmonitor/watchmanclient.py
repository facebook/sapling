# watchmanclient.py - Watchman client for the fsmonitor extension
#
# Copyright 2013-2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import getpass

from mercurial import util

from . import pywatchman

class Unavailable(Exception):
    def __init__(self, msg, warn=True, invalidate=False):
        self.msg = msg
        self.warn = warn
        if self.msg == 'timed out waiting for response':
            self.warn = False
        self.invalidate = invalidate

    def __str__(self):
        if self.warn:
            return 'warning: Watchman unavailable: %s' % self.msg
        else:
            return 'Watchman unavailable: %s' % self.msg

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
        if self._user in repo.ui.configlist('fsmonitor', 'blacklistusers'):
            err = 'user %s in blacklist' % self._user
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
        result = self.command('clock')
        if not util.safehasattr(result, 'clock'):
            raise Unavailable('clock result is missing clock value',
                              invalidate=True)
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
        try:
            if self._watchmanclient is None:
                self._firsttime = False
                self._watchmanclient = pywatchman.client(
                    timeout=self._timeout,
                    useImmutableBser=True)
            return self._watchmanclient.query(*watchmanargs)
        except pywatchman.CommandError as ex:
            if ex.msg.startswith('unable to resolve root'):
                raise WatchmanNoRoot(self._root, ex.msg)
            raise Unavailable(ex.msg)
        except pywatchman.WatchmanError as ex:
            raise Unavailable(str(ex))

    def command(self, *args):
        try:
            try:
                return self._command(*args)
            except WatchmanNoRoot:
                # this 'watch' command can also raise a WatchmanNoRoot if
                # watchman refuses to accept this root
                self._command('watch')
                return self._command(*args)
        except Unavailable:
            # this is in an outer scope to catch Unavailable form any of the
            # above _command calls
            self._watchmanclient = None
            raise
