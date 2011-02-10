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

def read(repo):
    '''Parse .hg/bookmarks file and return a dictionary

    Bookmarks are stored as {HASH}\\s{NAME}\\n (localtags format) values
    in the .hg/bookmarks file.
    Read the file and return a (name=>nodeid) dictionary
    '''
    try:
        bookmarks = {}
        for line in repo.opener('bookmarks'):
            sha, refspec = line.strip().split(' ', 1)
            refspec = encoding.tolocal(refspec)
            bookmarks[refspec] = repo.changelog.lookup(sha)
    except:
        pass
    return bookmarks

def readcurrent(repo):
    '''Get the current bookmark

    If we use gittishsh branches we have a current bookmark that
    we are on. This function returns the name of the bookmark. It
    is stored in .hg/bookmarks.current
    '''
    mark = None
    if os.path.exists(repo.join('bookmarks.current')):
        file = repo.opener('bookmarks.current')
        # No readline() in posixfile_nt, reading everything is cheap
        mark = (file.readlines() or [''])[0]
        if mark == '':
            mark = None
        file.close()
    return mark

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
