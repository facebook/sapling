# extension to emulate interupting filemerge._filemerge

from __future__ import absolute_import

from mercurial import (
    filemerge,
    extensions,
    error,
)

def failfilemerge(filemergefn,
        premerge, repo, mynode, orig, fcd, fco, fca, labels=None):
    raise error.Abort("^C")
    return filemergefn(premerge, repo, mynode, orig, fcd, fco, fca, labels)

def extsetup(ui):
    extensions.wrapfunction(filemerge, '_filemerge',
                            failfilemerge)
