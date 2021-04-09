# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# txnutil.py - transaction related utilities
#
#  Copyright FUJIWARA Katsunori <foozy@lares.dti.ne.jp> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno

from . import encoding


def mayhavepending(root):
    """return whether 'root' may have pending changes, which are
    visible to this process.
    """
    localispending = root == encoding.environ.get("HG_PENDING")
    localissharedpending = root == encoding.environ.get("HG_SHAREDPENDING")
    return localispending or localissharedpending


def mayhavesharedpending(root, sharedroot):
    """return whether 'root' or 'sharedroot' may have pending changes, which
    are visible to this process.
    """
    localispending = root == encoding.environ.get("HG_PENDING")
    sharedissharedpending = sharedroot == encoding.environ.get("HG_SHAREDPENDING")
    return localispending or sharedissharedpending


def trypending(root, vfs, filename, **kwargs):
    """Open a file to be read according to the HG_PENDING environment variable.

    This opens '.pending' of specified 'filename' only when HG_PENDING
    is equal to 'root'.

    This returns '(fp, is_pending_opened)' tuple.
    """
    if mayhavepending(root):
        try:
            return (vfs("%s.pending" % filename, **kwargs), True)
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
    return (vfs(filename, **kwargs), False)


def trysharedpending(root, sharedroot, vfs, filename, **kwargs):
    if mayhavesharedpending(root, sharedroot):
        try:
            return (vfs("%s.pending" % filename, **kwargs), True)
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
    return (vfs(filename, **kwargs), False)
