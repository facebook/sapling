# Copyright 2016-present Facebook. All Rights Reserved.
#
# context: context needed to annotate a file
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import (
    util,
)

# extracted from mercurial.context.basefilectx.annotate
def _getbase(fctx):
    introrev = fctx.introrev()
    if fctx.rev() == introrev:
        return fctx
    else:
        return fctx.filectx(fctx.filenode(), changeid=introrev)

# extracted from mercurial.context.basefilectx.annotate
@util.lrucachefunc
def _getlog(f, x):
    return f._repo.file(x)

# extracted from mercurial.context.basefilectx.annotate
def _parents(f, follow=True):
    # Cut _descendantrev here to mitigate the penalty of lazy linkrev
    # adjustment. Otherwise, p._adjustlinkrev() would walk changelog
    # from the topmost introrev (= srcrev) down to p.linkrev() if it
    # isn't an ancestor of the srcrev.
    f._changeid
    pl = f.parents()

    # Don't return renamed parents if we aren't following.
    if not follow:
        pl = [p for p in pl if p.path() == f.path()]

    # renamed filectx won't have a filelog yet, so set it
    # from the cache to save time
    for p in pl:
        if not '_filelog' in p.__dict__:
            p._filelog = _getlog(f, p.path())

    return pl

# extracted from mercurial.context.basefilectx.annotate. slightly modified
# so it takes a fctx instead of a pair of text and fctx.
def _decorate(fctx):
    text = fctx.data()
    linecount = text.count('\n')
    if not text.endswith('\n'):
        linecount += 1
    return ([(fctx, i) for i in xrange(linecount)], text)

# extracted from mercurial.context.basefilectx.annotate. slightly modified
# so it takes an extra "blocks" parameter calculated elsewhere, instead of
# calculating diff here.
def _pair(parent, child, blocks):
    for (a1, a2, b1, b2), t in blocks:
        # Changed blocks ('!') or blocks made only of blank lines ('~')
        # belong to the child.
        if t == '=':
            child[0][b1:b2] = parent[0][a1:a2]
    return child
