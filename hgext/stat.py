# stat.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import (
    patch,
    registrar,
    util,
)

templatefunc = registrar.templatefunc()

@templatefunc('stat()')
def showdiffstat(context, mapping, args):
    """String. Return diffstat-style summary of changes."""
    repo = mapping['repo']
    ctx = mapping['ctx']
    width = repo.ui.termwidth()
    return patch.diffstat(util.iterlines(ctx.diff(noprefix=False)), width=width)
