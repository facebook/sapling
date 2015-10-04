# streamclone.py - producing and consuming streaming repository data
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import time

from .i18n import _
from . import (
    branchmap,
    error,
    store,
    util,
)

def canperformstreamclone(repo, remote, heads, streamrequested=None):
    """Whether it is possible to perform a streaming clone as part of pull.

    Returns a tuple of (supported, requirements). ``supported`` is True if
    streaming clone is supported and False otherwise. ``requirements`` is
    a set of repo requirements from the remote, or ``None`` if stream clone
    isn't supported.
    """
    # Streaming clone only works on empty repositories.
    if len(repo):
        return False, None

    # Streaming clone only works if all data is being requested.
    if heads:
        return False, None

    # If we don't have a preference, let the server decide for us. This
    # likely only comes into play in LANs.
    if streamrequested is None:
        # The server can advertise whether to prefer streaming clone.
        streamrequested = remote.capable('stream-preferred')

    if not streamrequested:
        return False, None

    # In order for stream clone to work, the client has to support all the
    # requirements advertised by the server.
    #
    # The server advertises its requirements via the "stream" and "streamreqs"
    # capability. "stream" (a value-less capability) is advertised if and only
    # if the only requirement is "revlogv1." Else, the "streamreqs" capability
    # is advertised and contains a comma-delimited list of requirements.
    requirements = set()
    if remote.capable('stream'):
        requirements.add('revlogv1')
    else:
        streamreqs = remote.capable('streamreqs')
        # This is weird and shouldn't happen with modern servers.
        if not streamreqs:
            return False, None

        streamreqs = set(streamreqs.split(','))
        # Server requires something we don't support. Bail.
        if streamreqs - repo.supportedformats:
            return False, None
        requirements = streamreqs

    return True, requirements

def maybeperformstreamclone(pullop):
    repo = pullop.repo
    remote = pullop.remote

    r = canperformstreamclone(repo, remote, pullop.heads,
                              streamrequested=pullop.streamclonerequested)
    supported, requirements = r

    if not supported:
        return

    streamin(repo, remote, requirements)

def allowservergeneration(ui):
    """Whether streaming clones are allowed from the server."""
    return ui.configbool('server', 'uncompressed', True, untrusted=True)

# This is it's own function so extensions can override it.
def _walkstreamfiles(repo):
    return repo.store.walk()

def generatev1(repo):
    """Emit content for version 1 of a streaming clone.

    This is a generator of raw chunks that constitute a streaming clone.

    The stream begins with a line of 2 space-delimited integers containing the
    number of entries and total bytes size.

    Next, are N entries for each file being transferred. Each file entry starts
    as a line with the file name and integer size delimited by a null byte.
    The raw file data follows. Following the raw file data is the next file
    entry, or EOF.

    When used on the wire protocol, an additional line indicating protocol
    success will be prepended to the stream. This function is not responsible
    for adding it.

    This function will obtain a repository lock to ensure a consistent view of
    the store is captured. It therefore may raise LockError.
    """
    entries = []
    total_bytes = 0
    # Get consistent snapshot of repo, lock during scan.
    lock = repo.lock()
    try:
        repo.ui.debug('scanning\n')
        for name, ename, size in _walkstreamfiles(repo):
            if size:
                entries.append((name, size))
                total_bytes += size
    finally:
            lock.release()

    repo.ui.debug('%d files, %d bytes to transfer\n' %
                  (len(entries), total_bytes))
    yield '%d %d\n' % (len(entries), total_bytes)

    svfs = repo.svfs
    oldaudit = svfs.mustaudit
    debugflag = repo.ui.debugflag
    svfs.mustaudit = False

    try:
        for name, size in entries:
            if debugflag:
                repo.ui.debug('sending %s (%d bytes)\n' % (name, size))
            # partially encode name over the wire for backwards compat
            yield '%s\0%d\n' % (store.encodedir(name), size)
            if size <= 65536:
                fp = svfs(name)
                try:
                    data = fp.read(size)
                finally:
                    fp.close()
                yield data
            else:
                for chunk in util.filechunkiter(svfs(name), limit=size):
                    yield chunk
    finally:
        svfs.mustaudit = oldaudit

def consumev1(repo, fp):
    """Apply the contents from version 1 of a streaming clone file handle.

    This takes the output from "streamout" and applies it to the specified
    repository.

    Like "streamout," the status line added by the wire protocol is not handled
    by this function.
    """
    lock = repo.lock()
    try:
        repo.ui.status(_('streaming all changes\n'))
        l = fp.readline()
        try:
            total_files, total_bytes = map(int, l.split(' ', 1))
        except (ValueError, TypeError):
            raise error.ResponseError(
                _('unexpected response from remote server:'), l)
        repo.ui.status(_('%d files to transfer, %s of data\n') %
                       (total_files, util.bytecount(total_bytes)))
        handled_bytes = 0
        repo.ui.progress(_('clone'), 0, total=total_bytes)
        start = time.time()

        tr = repo.transaction(_('clone'))
        try:
            for i in xrange(total_files):
                # XXX doesn't support '\n' or '\r' in filenames
                l = fp.readline()
                try:
                    name, size = l.split('\0', 1)
                    size = int(size)
                except (ValueError, TypeError):
                    raise error.ResponseError(
                        _('unexpected response from remote server:'), l)
                if repo.ui.debugflag:
                    repo.ui.debug('adding %s (%s)\n' %
                                  (name, util.bytecount(size)))
                # for backwards compat, name was partially encoded
                ofp = repo.svfs(store.decodedir(name), 'w')
                for chunk in util.filechunkiter(fp, limit=size):
                    handled_bytes += len(chunk)
                    repo.ui.progress(_('clone'), handled_bytes,
                                     total=total_bytes)
                    ofp.write(chunk)
                ofp.close()
            tr.close()
        finally:
            tr.release()

        # Writing straight to files circumvented the inmemory caches
        repo.invalidate()

        elapsed = time.time() - start
        if elapsed <= 0:
            elapsed = 0.001
        repo.ui.progress(_('clone'), None)
        repo.ui.status(_('transferred %s in %.1f seconds (%s/sec)\n') %
                       (util.bytecount(total_bytes), elapsed,
                        util.bytecount(total_bytes / elapsed)))
    finally:
        lock.release()

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
    feeding into consumev1().
    """
    lock = repo.lock()
    try:
        consumev1(repo, fp)

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
