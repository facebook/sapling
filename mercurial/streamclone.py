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

def canperformstreamclone(pullop, bailifbundle2supported=False):
    """Whether it is possible to perform a streaming clone as part of pull.

    ``bailifbundle2supported`` will cause the function to return False if
    bundle2 stream clones are supported. It should only be called by the
    legacy stream clone code path.

    Returns a tuple of (supported, requirements). ``supported`` is True if
    streaming clone is supported and False otherwise. ``requirements`` is
    a set of repo requirements from the remote, or ``None`` if stream clone
    isn't supported.
    """
    repo = pullop.repo
    remote = pullop.remote

    bundle2supported = False
    if pullop.canusebundle2:
        if 'v1' in pullop.remotebundle2caps.get('stream', []):
            bundle2supported = True
        # else
            # Server doesn't support bundle2 stream clone or doesn't support
            # the versions we support. Fall back and possibly allow legacy.

    # Ensures legacy code path uses available bundle2.
    if bailifbundle2supported and bundle2supported:
        return False, None
    # Ensures bundle2 doesn't try to do a stream clone if it isn't supported.
    #elif not bailifbundle2supported and not bundle2supported:
    #    return False, None

    # Streaming clone only works on empty repositories.
    if len(repo):
        return False, None

    # Streaming clone only works if all data is being requested.
    if pullop.heads:
        return False, None

    streamrequested = pullop.streamclonerequested

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

def maybeperformlegacystreamclone(pullop):
    """Possibly perform a legacy stream clone operation.

    Legacy stream clones are performed as part of pull but before all other
    operations.

    A legacy stream clone will not be performed if a bundle2 stream clone is
    supported.
    """
    supported, requirements = canperformstreamclone(pullop)

    if not supported:
        return

    repo = pullop.repo
    remote = pullop.remote

    # Save remote branchmap. We will use it later to speed up branchcache
    # creation.
    rbranchmap = None
    if remote.capable('branchmap'):
        rbranchmap = remote.branchmap()

    repo.ui.status(_('streaming all changes\n'))

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

    l = fp.readline()
    try:
        filecount, bytecount = map(int, l.split(' ', 1))
    except (ValueError, TypeError):
        raise error.ResponseError(
            _('unexpected response from remote server:'), l)

    lock = repo.lock()
    try:
        consumev1(repo, fp, filecount, bytecount)

        # new requirements = old non-format requirements +
        #                    new format-related remote requirements
        # requirements from the streamed-in repository
        repo.requirements = requirements | (
                repo.requirements - repo.supportedformats)
        repo._applyopenerreqs()
        repo._writerequirements()

        if rbranchmap:
            branchmap.replacecache(repo, rbranchmap)

        repo.invalidate()
    finally:
        lock.release()

def allowservergeneration(ui):
    """Whether streaming clones are allowed from the server."""
    return ui.configbool('server', 'uncompressed', True, untrusted=True)

# This is it's own function so extensions can override it.
def _walkstreamfiles(repo):
    return repo.store.walk()

def generatev1(repo):
    """Emit content for version 1 of a streaming clone.

    This returns a 3-tuple of (file count, byte size, data iterator).

    The data iterator consists of N entries for each file being transferred.
    Each file entry starts as a line with the file name and integer size
    delimited by a null byte.

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

    svfs = repo.svfs
    oldaudit = svfs.mustaudit
    debugflag = repo.ui.debugflag
    svfs.mustaudit = False

    def emitrevlogdata():
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

    return len(entries), total_bytes, emitrevlogdata()

def generatev1wireproto(repo):
    """Emit content for version 1 of streaming clone suitable for the wire.

    This is the data output from ``generatev1()`` with a header line
    indicating file count and byte size.
    """
    filecount, bytecount, it = generatev1(repo)
    yield '%d %d\n' % (filecount, bytecount)
    for chunk in it:
        yield chunk

def consumev1(repo, fp, filecount, bytecount):
    """Apply the contents from version 1 of a streaming clone file handle.

    This takes the output from "streamout" and applies it to the specified
    repository.

    Like "streamout," the status line added by the wire protocol is not handled
    by this function.
    """
    lock = repo.lock()
    try:
        repo.ui.status(_('%d files to transfer, %s of data\n') %
                       (filecount, util.bytecount(bytecount)))
        handled_bytes = 0
        repo.ui.progress(_('clone'), 0, total=bytecount)
        start = time.time()

        tr = repo.transaction(_('clone'))
        try:
            for i in xrange(filecount):
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
                    repo.ui.progress(_('clone'), handled_bytes, total=bytecount)
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
                       (util.bytecount(bytecount), elapsed,
                        util.bytecount(bytecount / elapsed)))
    finally:
        lock.release()
