# Copyright 2016-present Facebook. All Rights Reserved.
#
# context: context needed to annotate a file
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import os

from fastannotate import (
    revmap as revmapmod,
)

from mercurial import (
    lock as lockmod,
    util,
)

import linelog as linelogmod

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

class annotateopts(object):
    """like mercurial.mdiff.diffopts, but is for annotate

    followrename: follow renames, like "hg annotate -f"
    followmerge: follow p2 of a merge changeset, otherwise p2 is ignored
    """

    defaults = {
        'followrename': True,
        'followmerge': True,
    }

    def __init__(self, **opts):
        for k, v in self.defaults.iteritems():
            setattr(self, k, opts.get(k, v))

    @property
    def shortstr(self):
        """represent opts in a short string, suitable for a directory name"""
        result = ''
        if not self.followrename:
            result += 'r0'
        if not self.followmerge:
            result += 'm0'
        return result or 'default'

defaultopts = annotateopts()

class _annotatecontext(object):
    """do not use this class directly as it does not use lock to protect
    writes. use "with annotatecontext(...)" instead.
    """

    def __init__(self, repo, path, linelog, revmap, opts):
        self.repo = repo
        self.ui = repo.ui
        self.path = path
        self.linelog = linelog
        self.revmap = revmap
        self.opts = opts

def _unlinkpaths(paths):
    """silent, best-effort unlink"""
    for path in paths:
        try:
            util.unlink(path)
        except OSError:
            pass

@contextlib.contextmanager
def annotatecontext(repo, path, opts=defaultopts, rebuild=False):
    """context needed to perform (fast) annotate on a file

    an annotatecontext of a single file consists of two structures: the
    linelog and the revmap. this function takes care of locking. only 1
    process is allowed to write that file's linelog and revmap at a time.

    when something goes wrong, this function will assume the linelog and the
    revmap are in a bad state, and remove them from disk.

    use this function in the following way:

        with annotatecontext(...) as actx:
            actx. ....
    """
    # different options use different directories
    subpath = os.path.join('fastannotate', opts.shortstr, path)
    util.makedirs(repo.vfs.join(os.path.dirname(subpath)))
    lockpath = subpath + '.lock'
    lock = lockmod.lock(repo.vfs, lockpath)
    fullpath = repo.vfs.join(subpath)
    revmappath = fullpath + '.m'
    linelogpath = fullpath + '.l'
    linelog = revmap = None
    try:
        with lock:
            if rebuild:
                _unlinkpaths([revmappath, linelogpath])
            revmap = revmapmod.revmap(revmappath)
            linelog = linelogmod.linelog(linelogpath)
            yield _annotatecontext(repo, path, linelog, revmap, opts)
    except Exception:
        revmap = linelog = None
        _unlinkpaths([revmappath, linelogpath])
        repo.ui.debug('fastannotate: %s: cache broken and deleted\n' % path)
        raise
    finally:
        if revmap:
            revmap.flush()
        if linelog:
            linelog.close()
