# Mercurial bookmark support code
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial.node import nullid, nullrev, bin, hex, short
from mercurial import encoding, util
import os

def valid(mark):
    for c in (':', '\0', '\n', '\r'):
        if c in mark:
            return False
    return True

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
        mark = encoding.tolocal((file.readlines() or [''])[0])
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
    for mark in refs.keys():
        if not valid(mark):
            raise util.Abort(_("bookmark '%s' contains illegal "
                "character" % mark))

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
    if not valid(mark):
        raise util.Abort(_("bookmark '%s' contains illegal "
            "character" % mark))

    wlock = repo.wlock()
    try:
        file = repo.opener('bookmarks.current', 'w', atomictemp=True)
        file.write(mark)
        file.rename()
    finally:
        wlock.release()
    repo._bookmarkcurrent = mark

def update(repo, parents, node):
    marks = repo._bookmarks
    update = False
    mark = repo._bookmarkcurrent
    if mark and marks[mark] in parents:
        old = repo[marks[mark]]
        new = repo[node]
        if new in old.descendants():
            marks[mark] = new.node()
            update = True
    if update:
        write(repo)

def listbookmarks(repo):
    # We may try to list bookmarks on a repo type that does not
    # support it (e.g., statichttprepository).
    if not hasattr(repo, '_bookmarks'):
        return {}

    d = {}
    for k, v in repo._bookmarks.iteritems():
        d[k] = hex(v)
    return d

def pushbookmark(repo, key, old, new):
    w = repo.wlock()
    try:
        marks = repo._bookmarks
        if hex(marks.get(key, '')) != old:
            return False
        if new == '':
            del marks[key]
        else:
            if new not in repo:
                return False
            marks[key] = repo[new].node()
        write(repo)
        return True
    finally:
        w.release()

def diff(ui, repo, remote):
    ui.status(_("searching for changed bookmarks\n"))

    lmarks = repo.listkeys('bookmarks')
    rmarks = remote.listkeys('bookmarks')

    diff = sorted(set(rmarks) - set(lmarks))
    for k in diff:
        ui.write("   %-25s %s\n" % (k, rmarks[k][:12]))

    if len(diff) <= 0:
        ui.status(_("no changed bookmarks found\n"))
        return 1
    return 0
