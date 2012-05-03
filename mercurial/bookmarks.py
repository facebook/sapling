# Mercurial bookmark support code
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial.node import hex
from mercurial import encoding, util
import errno, os

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
    bookmarks = {}
    try:
        for line in repo.opener('bookmarks'):
            line = line.strip()
            if not line:
                continue
            if ' ' not in line:
                repo.ui.warn(_('malformed line in .hg/bookmarks: %r\n') % line)
                continue
            sha, refspec = line.split(' ', 1)
            refspec = encoding.tolocal(refspec)
            try:
                bookmarks[refspec] = repo.changelog.lookup(sha)
            except LookupError:
                pass
    except IOError, inst:
        if inst.errno != errno.ENOENT:
            raise
    return bookmarks

def readcurrent(repo):
    '''Get the current bookmark

    If we use gittishsh branches we have a current bookmark that
    we are on. This function returns the name of the bookmark. It
    is stored in .hg/bookmarks.current
    '''
    mark = None
    try:
        file = repo.opener('bookmarks.current')
    except IOError, inst:
        if inst.errno != errno.ENOENT:
            raise
        return None
    try:
        # No readline() in posixfile_nt, reading everything is cheap
        mark = encoding.tolocal((file.readlines() or [''])[0])
        if mark == '' or mark not in repo._bookmarks:
            mark = None
    finally:
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
        file.close()

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

    if mark not in repo._bookmarks:
        mark = ''
    if not valid(mark):
        raise util.Abort(_("bookmark '%s' contains illegal "
            "character" % mark))

    wlock = repo.wlock()
    try:
        file = repo.opener('bookmarks.current', 'w', atomictemp=True)
        file.write(encoding.fromlocal(mark))
        file.close()
    finally:
        wlock.release()
    repo._bookmarkcurrent = mark

def unsetcurrent(repo):
    wlock = repo.wlock()
    try:
        try:
            util.unlink(repo.join('bookmarks.current'))
            repo._bookmarkcurrent = None
        except OSError, inst:
            if inst.errno != errno.ENOENT:
                raise
    finally:
        wlock.release()

def updatecurrentbookmark(repo, oldnode, curbranch):
    try:
        return update(repo, oldnode, repo.branchtags()[curbranch])
    except KeyError:
        if curbranch == "default": # no default branch!
            return update(repo, oldnode, repo.lookup("tip"))
        else:
            raise util.Abort(_("branch %s not found") % curbranch)

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
        repo._writebookmarks(marks)
    return update

def listbookmarks(repo):
    # We may try to list bookmarks on a repo type that does not
    # support it (e.g., statichttprepository).
    marks = getattr(repo, '_bookmarks', {})

    d = {}
    for k, v in marks.iteritems():
        # don't expose local divergent bookmarks
        if '@' not in k or k.endswith('@'):
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

def updatefromremote(ui, repo, remote, path):
    ui.debug("checking for updated bookmarks\n")
    rb = remote.listkeys('bookmarks')
    changed = False
    for k in rb.keys():
        if k in repo._bookmarks:
            nr, nl = rb[k], repo._bookmarks[k]
            if nr in repo:
                cr = repo[nr]
                cl = repo[nl]
                if cl.rev() >= cr.rev():
                    continue
                if cr in cl.descendants():
                    repo._bookmarks[k] = cr.node()
                    changed = True
                    ui.status(_("updating bookmark %s\n") % k)
                else:
                    # find a unique @ suffix
                    for x in range(1, 100):
                        n = '%s@%d' % (k, x)
                        if n not in repo._bookmarks:
                            break
                    # try to use an @pathalias suffix
                    # if an @pathalias already exists, we overwrite (update) it
                    for p, u in ui.configitems("paths"):
                        if path == u:
                            n = '%s@%s' % (k, p)

                    repo._bookmarks[n] = cr.node()
                    changed = True
                    ui.warn(_("divergent bookmark %s stored as %s\n") % (k, n))

    if changed:
        write(repo)

def diff(ui, repo, remote):
    ui.status(_("searching for changed bookmarks\n"))

    lmarks = repo.listkeys('bookmarks')
    rmarks = remote.listkeys('bookmarks')

    diff = sorted(set(rmarks) - set(lmarks))
    for k in diff:
        mark = ui.debugflag and rmarks[k] or rmarks[k][:12]
        ui.write("   %-25s %s\n" % (k, mark))

    if len(diff) <= 0:
        ui.status(_("no changed bookmarks found\n"))
        return 1
    return 0
