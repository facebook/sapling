# filemerge.py - file-level merge handling for Mercurial
#
# Copyright 2006, 2007, 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from i18n import _
import util, os, tempfile, context

def filemerge(repo, fw, fd, fo, wctx, mctx):
    """perform a 3-way merge in the working directory

    fw = original filename in the working directory
    fd = destination filename in the working directory
    fo = filename in other parent
    wctx, mctx = working and merge changecontexts
    """

    def temp(prefix, ctx):
        pre = "%s~%s." % (os.path.basename(ctx.path()), prefix)
        (fd, name) = tempfile.mkstemp(prefix=pre)
        data = repo.wwritedata(ctx.path(), ctx.data())
        f = os.fdopen(fd, "wb")
        f.write(data)
        f.close()
        return name

    fcm = wctx.filectx(fw)
    fcmdata = wctx.filectx(fd).data()
    fco = mctx.filectx(fo)

    if not fco.cmp(fcmdata): # files identical?
        return None

    fca = fcm.ancestor(fco)
    if not fca:
        fca = repo.filectx(fw, fileid=nullrev)
    a = repo.wjoin(fd)
    b = temp("base", fca)
    c = temp("other", fco)

    if fw != fo:
        repo.ui.status(_("merging %s and %s\n") % (fw, fo))
    else:
        repo.ui.status(_("merging %s\n") % fw)

    repo.ui.debug(_("my %s other %s ancestor %s\n") % (fcm, fco, fca))

    cmd = (os.environ.get("HGMERGE") or repo.ui.config("ui", "merge")
           or "hgmerge")
    r = util.system('%s "%s" "%s" "%s"' % (cmd, a, b, c), cwd=repo.root,
                    environ={'HG_FILE': fd,
                             'HG_MY_NODE': str(wctx.parents()[0]),
                             'HG_OTHER_NODE': str(mctx),
                             'HG_MY_ISLINK': fcm.islink(),
                             'HG_OTHER_ISLINK': fco.islink(),
                             'HG_BASE_ISLINK': fca.islink(),})
    if r:
        repo.ui.warn(_("merging %s failed!\n") % fd)

    os.unlink(b)
    os.unlink(c)
    return r
