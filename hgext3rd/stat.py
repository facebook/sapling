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

templatekeyword = registrar.templatekeyword()

@templatekeyword('stat')
def showdiffstat(repo, ctx, templ, **args):
    """String. Return diffstat-style summary of changes."""
    width = repo.ui.termwidth()
    return patch.diffstat(util.iterlines(ctx.diff(noprefix=False)), width=width)
