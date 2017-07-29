# dirstateguard.py - class to allow restoring dirstate after failure
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .i18n import _

from . import (
    error,
    util,
)

class dirstateguard(util.transactional):
    '''Restore dirstate at unexpected failure.

    At the construction, this class does:

    - write current ``repo.dirstate`` out, and
    - save ``.hg/dirstate`` into the backup file

    This restores ``.hg/dirstate`` from backup file, if ``release()``
    is invoked before ``close()``.

    This just removes the backup file at ``close()`` before ``release()``.
    '''

    def __init__(self, repo, name):
        self._repo = repo
        self._active = False
        self._closed = False
        self._backupname = 'dirstate.backup.%s.%d' % (name, id(self))
        repo.dirstate.savebackup(repo.currenttransaction(), self._backupname)
        self._active = True

    def __del__(self):
        if self._active: # still active
            # this may occur, even if this class is used correctly:
            # for example, releasing other resources like transaction
            # may raise exception before ``dirstateguard.release`` in
            # ``release(tr, ....)``.
            self._abort()

    def close(self):
        if not self._active: # already inactivated
            msg = (_("can't close already inactivated backup: %s")
                   % self._backupname)
            raise error.Abort(msg)

        self._repo.dirstate.clearbackup(self._repo.currenttransaction(),
                                         self._backupname)
        self._active = False
        self._closed = True

    def _abort(self):
        self._repo.dirstate.restorebackup(self._repo.currenttransaction(),
                                           self._backupname)
        self._active = False

    def release(self):
        if not self._closed:
            if not self._active: # already inactivated
                msg = (_("can't release already inactivated backup: %s")
                       % self._backupname)
                raise error.Abort(msg)
            self._abort()
