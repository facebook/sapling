# perftweaks.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""extension for tweaking Mercurial features to improve performance."""

from mercurial import branchmap, merge, scmutil, tags
from mercurial.extensions import wrapcommand, wrapfunction
from mercurial.i18n import _
from mercurial.node import nullid, nullrev
import os

testedwith = 'internal'

def extsetup(ui):
    wrapfunction(tags, '_readtagcache', _readtagcache)
    wrapfunction(merge, '_checkcollision', _checkcollision)
    wrapfunction(branchmap.branchcache, 'update', _branchmapupdate)

def _readtagcache(orig, ui, repo):
    """Disables reading tags if the repo is known to not contain any."""
    if ui.configbool('perftweaks', 'disabletags'):
        return (None, None, None, {}, False)

    return orig(ui, repo)

def _checkcollision(orig, repo, wmf, actions):
    """Disables case collision checking since it is known to be very slow."""
    if repo.ui.configbool('perftweaks', 'disablecasecheck'):
        return
    orig(repo, wmf, actions)

def _branchmapupdate(orig, self, repo, revgen):
    if not repo.ui.configbool('perftweaks', 'disablebranchcache'):
        return orig(self, repo, revgen)

    cl = repo.changelog

    # Since we have no branches, the default branch heads are equal to
    # cl.headrevs().
    branchheads = sorted(cl.headrevs())

    self['default'] = [cl.node(rev) for rev in branchheads]
    tiprev = branchheads[-1]
    if tiprev > self.tiprev:
        self.tipnode = cl.node(tiprev)
        self.tiprev = tiprev

    # Copy and paste from branchmap.branchcache.update()
    if not self.validfor(repo):
        # cache key are not valid anymore
        self.tipnode = nullid
        self.tiprev = nullrev
        for heads in self.values():
            tiprev = max(cl.rev(node) for node in heads)
            if tiprev > self.tiprev:
                self.tipnode = cl.node(tiprev)
                self.tiprev = tiprev
    self.filteredhash = scmutil.filteredhash(repo, self.tiprev)
    repo.ui.log('branchcache', 'perftweaks updated %s branch cache\n',
                repo.filtername)
