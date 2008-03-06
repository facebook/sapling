# streamclone.py - streaming clone server support for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, osutil, stat, util, lock

# if server supports streaming clone, it advertises "stream"
# capability with value that is version+flags of repo it is serving.
# client only streams if it can read that repo format.

def walkrepo(root):
    '''iterate over metadata files in repository.
    walk in natural (sorted) order.
    yields 2-tuples: name of .d or .i file, size of file.'''

    strip_count = len(root) + len(os.sep)
    def walk(path, recurse):
        for e, kind, st in osutil.listdir(path, stat=True):
            pe = os.path.join(path, e)
            if kind == stat.S_IFDIR:
                if recurse:
                    for x in walk(pe, True):
                        yield x
            else:
                if kind != stat.S_IFREG or len(e) < 2:
                    continue
                sfx = e[-2:]
                if sfx in ('.d', '.i'):
                    yield pe[strip_count:], st.st_size
    # write file data first
    for x in walk(os.path.join(root, 'data'), True):
        yield x
    # write manifest before changelog
    meta = list(walk(root, False))
    meta.sort()
    meta.reverse()
    for x in meta:
        yield x

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

    # get consistent snapshot of repo. lock during scan so lock not
    # needed while we stream, and commits can happen.
    repolock = None
    try:
        try:
            repolock = repo.lock()
        except (lock.LockHeld, lock.LockUnavailable), inst:
            repo.ui.warn('locking the repository failed: %s\n' % (inst,))
            fileobj.write('2\n')
            return

        fileobj.write('0\n')
        repo.ui.debug('scanning\n')
        entries = []
        total_bytes = 0
        for name, size in walkrepo(repo.spath):
            name = repo.decodefn(util.pconvert(name))
            entries.append((name, size))
            total_bytes += size
    finally:
        del repolock

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
