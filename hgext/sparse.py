# sparse.py - shim that redirects to load fbsparse
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""allow sparse checkouts of the working directory
"""
from __future__ import absolute_import

from . import fbsparse

cmdtable = fbsparse.cmdtable.copy()

def _fbsparseexists(ui):
    with ui.configoverride({("devel", "all-warnings"): False}):
        return not ui.config('extensions', 'fbsparse', '!').startswith('!')

def uisetup(ui):
    if _fbsparseexists(ui):
        cmdtable.clear()
        return
    fbsparse.uisetup(ui)

def extsetup(ui):
    if _fbsparseexists(ui):
        cmdtable.clear()
        return
    fbsparse.extsetup(ui)

def reposetup(ui, repo):
    if _fbsparseexists(ui):
        return
    fbsparse.reposetup(ui, repo)
