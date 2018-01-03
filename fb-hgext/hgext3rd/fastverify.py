# fastverify.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""
disables parts of hg verify

Manifest verification can be extremely slow on large repos, so this extension
disables it if ``verify.skipmanifests`` is True.::

    [verify]
    skipmanifests = true
"""

from __future__ import absolute_import

from mercurial import (
    extensions,
    verify
)
from mercurial.i18n import _

testedwith = 'ships-with-fb-hgext'

class fastverifier(verify.verifier):
    def __init__(self, *args, **kwargs):
        super(fastverifier, self).__init__(*args, **kwargs)

    def _verifymanifest(self, *args, **kwargs):
        if self.ui.configbool("verify", "skipmanifests", True):
            self.ui.warn(_("verify.skipmanifests is enabled; skipping "
                           "verification of manifests\n"))
            return []

        return super(fastverifier, self)._verifymanifest(*args, **kwargs)

    def _crosscheckfiles(self, *args, **kwargs):
        if self.ui.configbool("verify", "skipmanifests", True):
            return

        return super(fastverifier, self)._crosscheckfiles(*args, **kwargs)

    def _verifyfiles(self, *args, **kwargs):
        if self.ui.configbool("verify", "skipmanifests", True):
            return 0, 0

        return super(fastverifier, self)._verifyfiles(*args, **kwargs)

def extsetup(ui):
    extensions.wrapfunction(verify, 'verify', _verify)

def _verify(orig, repo, *args, **kwds):
    with repo.lock():
        return fastverifier(repo, *args, **kwds).verify()
