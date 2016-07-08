# bundle2hooks.py - fix bundle2's support for hooks prior to lock acquisition.
#
# Copyright 2012 Facebook
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''Hooks arguments are typically stored on the transaction object.  However, we
may want to add hooks arguments without starting a transaction.  This allows us
to queue hook arguments on the bundle2 operation object.

'''

from mercurial import bundle2
from mercurial import error
from mercurial.i18n import _

from extutil import replaceclass

def reposetup(ui, repo):
    @replaceclass(bundle2, 'bundleoperation')
    class bundleoperationhooked(bundle2.bundleoperation):
        def __init__(self, repo, transactiongetter, *args, **kwargs):
            def gettransaction():
                transaction = transactiongetter()

                if self.hookargs is not None:
                    # the ones added to the transaction supercede those added
                    # to the operation.
                    self.hookargs.update(transaction.hookargs)
                    transaction.hookargs = self.hookargs

                    # mark the hookargs as flushed.  further attempts to add to
                    # hookargs will result in an abort.
                    self.hookargs = None

                return transaction

            super(bundleoperationhooked, self).__init__(repo, gettransaction,
                                                        *args, **kwargs)

            self.hookargs = {}

        def addhookargs(self, hookargs):
            if self.hookargs is None:
                raise error.Abort(
                    _('attempted to add hooks to operation after transaction '
                      'started'))
            self.hookargs.update(hookargs)
