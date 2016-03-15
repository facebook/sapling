# streamclone.py - producing and consuming streaming repository data
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import struct
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
        raise error.Abort(_('operation forbidden by server'))
    elif resp == 2:
        raise error.Abort(_('locking the remote repository failed'))
    elif resp != 0:
        raise error.Abort(_('the server sent an unknown error code'))

    l = fp.readline()
    try:
        filecount, bytecount = map(int, l.split(' ', 1))
    except (ValueError, TypeError):
        raise error.ResponseError(
            _('unexpected response from remote server:'), l)

    with repo.lock():
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
    with repo.lock():
        repo.ui.debug('scanning\n')
        for name, ename, size in _walkstreamfiles(repo):
            if size:
                entries.append((name, size))
                total_bytes += size

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
                    with svfs(name, 'rb') as fp:
                        yield fp.read(size)
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

def generatebundlev1(repo, compression='UN'):
    """Emit content for version 1 of a stream clone bundle.

    The first 4 bytes of the output ("HGS1") denote this as stream clone
    bundle version 1.

    The next 2 bytes indicate the compression type. Only "UN" is currently
    supported.

    The next 16 bytes are two 64-bit big endian unsigned integers indicating
    file count and byte count, respectively.

    The next 2 bytes is a 16-bit big endian unsigned short declaring the length
    of the requirements string, including a trailing \0. The following N bytes
    are the requirements string, which is ASCII containing a comma-delimited
    list of repo requirements that are needed to support the data.

    The remaining content is the output of ``generatev1()`` (which may be
    compressed in the future).

    Returns a tuple of (requirements, data generator).
    """
    if compression != 'UN':
        raise ValueError('we do not support the compression argument yet')

    requirements = repo.requirements & repo.supportedformats
    requires = ','.join(sorted(requirements))

    def gen():
        yield 'HGS1'
        yield compression

        filecount, bytecount, it = generatev1(repo)
        repo.ui.status(_('writing %d bytes for %d files\n') %
                         (bytecount, filecount))

        yield struct.pack('>QQ', filecount, bytecount)
        yield struct.pack('>H', len(requires) + 1)
        yield requires + '\0'

        # This is where we'll add compression in the future.
        assert compression == 'UN'

        seen = 0
        repo.ui.progress(_('bundle'), 0, total=bytecount, unit=_('bytes'))

        for chunk in it:
            seen += len(chunk)
            repo.ui.progress(_('bundle'), seen, total=bytecount,
                             unit=_('bytes'))
            yield chunk

        repo.ui.progress(_('bundle'), None)

    return requirements, gen()

def consumev1(repo, fp, filecount, bytecount):
    """Apply the contents from version 1 of a streaming clone file handle.

    This takes the output from "streamout" and applies it to the specified
    repository.

    Like "streamout," the status line added by the wire protocol is not handled
    by this function.
    """
    with repo.lock():
        repo.ui.status(_('%d files to transfer, %s of data\n') %
                       (filecount, util.bytecount(bytecount)))
        handled_bytes = 0
        repo.ui.progress(_('clone'), 0, total=bytecount, unit=_('bytes'))
        start = time.time()

        with repo.transaction('clone'):
            with repo.svfs.backgroundclosing(repo.ui, expectedcount=filecount):
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
                    path = store.decodedir(name)
                    with repo.svfs(path, 'w', backgroundclose=True) as ofp:
                        for chunk in util.filechunkiter(fp, limit=size):
                            handled_bytes += len(chunk)
                            repo.ui.progress(_('clone'), handled_bytes,
                                             total=bytecount, unit=_('bytes'))
                            ofp.write(chunk)

        # Writing straight to files circumvented the inmemory caches
        repo.invalidate()

        elapsed = time.time() - start
        if elapsed <= 0:
            elapsed = 0.001
        repo.ui.progress(_('clone'), None)
        repo.ui.status(_('transferred %s in %.1f seconds (%s/sec)\n') %
                       (util.bytecount(bytecount), elapsed,
                        util.bytecount(bytecount / elapsed)))

def readbundle1header(fp):
    compression = fp.read(2)
    if compression != 'UN':
        raise error.Abort(_('only uncompressed stream clone bundles are '
            'supported; got %s') % compression)

    filecount, bytecount = struct.unpack('>QQ', fp.read(16))
    requireslen = struct.unpack('>H', fp.read(2))[0]
    requires = fp.read(requireslen)

    if not requires.endswith('\0'):
        raise error.Abort(_('malformed stream clone bundle: '
                            'requirements not properly encoded'))

    requirements = set(requires.rstrip('\0').split(','))

    return filecount, bytecount, requirements

def applybundlev1(repo, fp):
    """Apply the content from a stream clone bundle version 1.

    We assume the 4 byte header has been read and validated and the file handle
    is at the 2 byte compression identifier.
    """
    if len(repo):
        raise error.Abort(_('cannot apply stream clone bundle on non-empty '
                            'repo'))

    filecount, bytecount, requirements = readbundle1header(fp)
    missingreqs = requirements - repo.supportedformats
    if missingreqs:
        raise error.Abort(_('unable to apply stream clone: '
                            'unsupported format: %s') %
                            ', '.join(sorted(missingreqs)))

    consumev1(repo, fp, filecount, bytecount)

class streamcloneapplier(object):
    """Class to manage applying streaming clone bundles.

    We need to wrap ``applybundlev1()`` in a dedicated type to enable bundle
    readers to perform bundle type-specific functionality.
    """
    def __init__(self, fh):
        self._fh = fh

    def apply(self, repo):
        return applybundlev1(repo, self._fh)
