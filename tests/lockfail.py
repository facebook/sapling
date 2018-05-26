# Dummy extension that throws if lock is taken
#
# This extension can be used to test that lock is not taken when it's not
# supposed to

from __future__ import absolute_import

from mercurial import error


def reposetup(ui, repo):

    class faillockrepo(repo.__class__):

        def lock(self, wait=True):
            raise error.Abort("lock is taken!")

        def wlock(self, wait=True):
            raise error.Abort("lock is taken!")

    repo.__class__ = faillockrepo
