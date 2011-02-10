# Mercurial bookmark support code
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial.node import nullid, nullrev, bin, hex, short
from mercurial import encoding
import os

def write(repo):
    '''Write bookmarks

    Write the given bookmark => hash dictionary to the .hg/bookmarks file
    in a format equal to those of localtags.

    We also store a backup of the previous state in undo.bookmarks that
    can be copied back on rollback.
    '''
    refs = repo._bookmarks

    try:
        bms = repo.opener('bookmarks').read()
    except IOError:
        bms = ''
    repo.opener('undo.bookmarks', 'w').write(bms)

    if repo._bookmarkcurrent not in refs:
        setcurrent(repo, None)
    wlock = repo.wlock()
    try:
        file = repo.opener('bookmarks', 'w', atomictemp=True)
        for refspec, node in refs.iteritems():
            file.write("%s %s\n" % (hex(node), encoding.fromlocal(refspec)))
        file.rename()

        # touch 00changelog.i so hgweb reloads bookmarks (no lock needed)
        try:
            os.utime(repo.sjoin('00changelog.i'), None)
        except OSError:
            pass

    finally:
        wlock.release()

def setcurrent(repo, mark):
    '''Set the name of the bookmark that we are currently on

    Set the name of the bookmark that we are on (hg update <bookmark>).
    The name is recorded in .hg/bookmarks.current
    '''
    current = repo._bookmarkcurrent
    if current == mark:
        return

    refs = repo._bookmarks

    # do not update if we do update to a rev equal to the current bookmark
    if (mark and mark not in refs and
        current and refs[current] == repo.changectx('.').node()):
        return
    if mark not in refs:
        mark = ''
    wlock = repo.wlock()
    try:
        file = repo.opener('bookmarks.current', 'w', atomictemp=True)
        file.write(mark)
        file.rename()
    finally:
        wlock.release()
    repo._bookmarkcurrent = mark
