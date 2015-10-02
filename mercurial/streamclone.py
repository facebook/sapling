# streamclone.py - producing and consuming streaming repository data
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .i18n import _
from . import (
    branchmap,
    error,
    exchange,
    util,
)

def streamin(repo, remote, remotereqs):
    # Save remote branchmap. We will use it later
    # to speed up branchcache creation
    rbranchmap = None
    if remote.capable("branchmap"):
        rbranchmap = remote.branchmap()

    fp = remote.stream_out()
    l = fp.readline()
    try:
        resp = int(l)
    except ValueError:
        raise error.ResponseError(
            _('unexpected response from remote server:'), l)
    if resp == 1:
        raise util.Abort(_('operation forbidden by server'))
    elif resp == 2:
        raise util.Abort(_('locking the remote repository failed'))
    elif resp != 0:
        raise util.Abort(_('the server sent an unknown error code'))

    applyremotedata(repo, remotereqs, rbranchmap, fp)
    return len(repo.heads()) + 1

def applyremotedata(repo, remotereqs, remotebranchmap, fp):
    """Apply stream clone data to a repository.

    "remotereqs" is a set of requirements to handle the incoming data.
    "remotebranchmap" is the result of a branchmap lookup on the remote. It
    can be None.
    "fp" is a file object containing the raw stream data, suitable for
    feeding into exchange.consumestreamclone.
    """
    lock = repo.lock()
    try:
        exchange.consumestreamclone(repo, fp)

        # new requirements = old non-format requirements +
        #                    new format-related remote requirements
        # requirements from the streamed-in repository
        repo.requirements = remotereqs | (
                repo.requirements - repo.supportedformats)
        repo._applyopenerreqs()
        repo._writerequirements()

        if remotebranchmap:
            rbheads = []
            closed = []
            for bheads in remotebranchmap.itervalues():
                rbheads.extend(bheads)
                for h in bheads:
                    r = repo.changelog.rev(h)
                    b, c = repo.changelog.branchinfo(r)
                    if c:
                        closed.append(h)

            if rbheads:
                rtiprev = max((int(repo.changelog.rev(node))
                        for node in rbheads))
                cache = branchmap.branchcache(remotebranchmap,
                                              repo[rtiprev].node(),
                                              rtiprev,
                                              closednodes=closed)
                # Try to stick it as low as possible
                # filter above served are unlikely to be fetch from a clone
                for candidate in ('base', 'immutable', 'served'):
                    rview = repo.filtered(candidate)
                    if cache.validfor(rview):
                        repo._branchcaches[candidate] = cache
                        cache.write(rview)
                        break
        repo.invalidate()
    finally:
        lock.release()
