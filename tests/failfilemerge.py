# extension to emulate interrupting filemerge._filemerge

from __future__ import absolute_import

from mercurial import (
    error,
    extensions,
    filemerge,
)

def failfilemerge(filemergefn,
                  premerge, repo, wctx, mynode, orig, fcd, fco, fca,
                  labels=None):
    raise error.Abort("^C")
    return filemergefn(premerge, repo, mynode, orig, fcd, fco, fca, labels)

def extsetup(ui):
    extensions.wrapfunction(filemerge, '_filemerge',
                            failfilemerge)
