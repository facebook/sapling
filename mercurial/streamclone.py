# streamclone.py - streaming clone server support for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import util, lock

# if server supports streaming clone, it advertises "stream"
# capability with value that is version+flags of repo it is serving.
# client only streams if it can read that repo format.

# stream file format is simple.
#
# server writes out line that says how many files, how many total
# bytes.  separator is ascii space, byte counts are strings.
#
# then for each file:
#
#   server writes out line that says file name, how many bytes in
#   file.  separator is ascii nul, byte count is string.
#
#   server writes out raw file data.

def stream_out(repo, fileobj, untrusted=False):
    '''stream out all metadata files in repository.
    writes to file-like object, must support write() and optional flush().'''

    if not repo.ui.configbool('server', 'uncompressed', untrusted=untrusted):
        fileobj.write('1\n')
        return

    entries = []
    total_bytes = 0
    try:
        l = None
        try:
            repo.ui.debug('scanning\n')
            # get consistent snapshot of repo, lock during scan
            l = repo.lock()
            for name, ename, size in repo.store.walk():
                entries.append((name, size))
                total_bytes += size
        finally:
            del l
    except (lock.LockHeld, lock.LockUnavailable), inst:
        repo.ui.warn('locking the repository failed: %s\n' % (inst,))
        fileobj.write('2\n')
        return

    fileobj.write('0\n')
    repo.ui.debug('%d files, %d bytes to transfer\n' %
                  (len(entries), total_bytes))
    fileobj.write('%d %d\n' % (len(entries), total_bytes))
    for name, size in entries:
        repo.ui.debug('sending %s (%d bytes)\n' % (name, size))
        fileobj.write('%s\0%d\n' % (name, size))
        for chunk in util.filechunkiter(repo.sopener(name), limit=size):
            fileobj.write(chunk)
    flush = getattr(fileobj, 'flush', None)
    if flush: flush()
