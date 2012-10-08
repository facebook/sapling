# Mercurial bookmark support code
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial.node import hex
from mercurial import encoding, error, util, obsolete
import errno, os

class bmstore(dict):
    """Storage for bookmarks.

    This object should do all bookmark reads and writes, so that it's
    fairly simple to replace the storage underlying bookmarks without
    having to clone the logic surrounding bookmarks.

    This particular bmstore implementation stores bookmarks as
    {hash}\s{name}\n (the same format as localtags) in
    .hg/bookmarks. The mapping is stored as {name: nodeid}.

    This class does NOT handle the "current" bookmark state at this
    time.
    """

    def __init__(self, repo):
        dict.__init__(self)
        self._repo = repo
        try:
            for line in repo.vfs('bookmarks'):
                line = line.strip()
                if not line:
                    continue
                if ' ' not in line:
                    repo.ui.warn(_('malformed line in .hg/bookmarks: %r\n')
                                 % line)
                    continue
                sha, refspec = line.split(' ', 1)
                refspec = encoding.tolocal(refspec)
                try:
                    self[refspec] = repo.changelog.lookup(sha)
                except LookupError:
                    pass
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise

    def write(self):
        '''Write bookmarks

        Write the given bookmark => hash dictionary to the .hg/bookmarks file
        in a format equal to those of localtags.

        We also store a backup of the previous state in undo.bookmarks that
        can be copied back on rollback.
        '''
        repo = self._repo
        if repo._bookmarkcurrent not in self:
            setcurrent(repo, None)

        wlock = repo.wlock()
        try:

            file = repo.vfs('bookmarks', 'w', atomictemp=True)
            for name, node in self.iteritems():
                file.write("%s %s\n" % (hex(node), encoding.fromlocal(name)))
            file.close()

            # touch 00changelog.i so hgweb reloads bookmarks (no lock needed)
            try:
                os.utime(repo.sjoin('00changelog.i'), None)
            except OSError:
                pass

        finally:
            wlock.release()

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
        # No readline() in osutil.posixfile, reading everything is cheap
        mark = encoding.tolocal((file.readlines() or [''])[0])
        if mark == '' or mark not in repo._bookmarks:
            mark = None
    finally:
        file.close()
    return mark

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
        return update(repo, oldnode, repo.branchtip(curbranch))
    except error.RepoLookupError:
        if curbranch == "default": # no default branch!
            return update(repo, oldnode, repo.lookup("tip"))
        else:
            raise util.Abort(_("branch %s not found") % curbranch)

def update(repo, parents, node):
    marks = repo._bookmarks
    update = False
    cur = repo._bookmarkcurrent
    if not cur:
        return False

    toupdate = [b for b in marks if b.split('@', 1)[0] == cur.split('@', 1)[0]]
    for mark in toupdate:
        if mark and marks[mark] in parents:
            old = repo[marks[mark]]
            new = repo[node]
            if old.descendant(new) and mark == cur:
                marks[cur] = new.node()
                update = True
            if mark != cur:
                del marks[mark]
    if update:
        marks.write()
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
        marks.write()
        return True
    finally:
        w.release()

def updatefromremote(ui, repo, remote, path):
    ui.debug("checking for updated bookmarks\n")
    rb = remote.listkeys('bookmarks')
    changed = False
    localmarks = repo._bookmarks
    for k in rb.keys():
        if k in localmarks:
            nr, nl = rb[k], localmarks[k]
            if nr in repo:
                cr = repo[nr]
                cl = repo[nl]
                if cl.rev() >= cr.rev():
                    continue
                if validdest(repo, cl, cr):
                    localmarks[k] = cr.node()
                    changed = True
                    ui.status(_("updating bookmark %s\n") % k)
                else:
                    if k == '@':
                        kd = ''
                    else:
                        kd = k
                    # find a unique @ suffix
                    for x in range(1, 100):
                        n = '%s@%d' % (kd, x)
                        if n not in localmarks:
                            break
                    # try to use an @pathalias suffix
                    # if an @pathalias already exists, we overwrite (update) it
                    for p, u in ui.configitems("paths"):
                        if path == u:
                            n = '%s@%s' % (kd, p)

                    localmarks[n] = cr.node()
                    changed = True
                    ui.warn(_("divergent bookmark %s stored as %s\n") % (k, n))
        elif rb[k] in repo:
            # add remote bookmarks for changes we already have
            localmarks[k] = repo[rb[k]].node()
            changed = True
            ui.status(_("adding remote bookmark %s\n") % k)

    if changed:
        localmarks.write()

def diff(ui, dst, src):
    ui.status(_("searching for changed bookmarks\n"))

    smarks = src.listkeys('bookmarks')
    dmarks = dst.listkeys('bookmarks')

    diff = sorted(set(smarks) - set(dmarks))
    for k in diff:
        mark = ui.debugflag and smarks[k] or smarks[k][:12]
        ui.write("   %-25s %s\n" % (k, mark))

    if len(diff) <= 0:
        ui.status(_("no changed bookmarks found\n"))
        return 1
    return 0

def validdest(repo, old, new):
    """Is the new bookmark destination a valid update from the old one"""
    repo = repo.unfiltered()
    if old == new:
        # Old == new -> nothing to update.
        return False
    elif not old:
        # old is nullrev, anything is valid.
        # (new != nullrev has been excluded by the previous check)
        return True
    elif repo.obsstore:
        # We only need this complicated logic if there is obsolescence
        # XXX will probably deserve an optimised revset.
        nm = repo.changelog.nodemap
        validdests = set([old])
        plen = -1
        # compute the whole set of successors or descendants
        while len(validdests) != plen:
            plen = len(validdests)
            succs = set(c.node() for c in validdests)
            mutable = [c.node() for c in validdests if c.mutable()]
            succs.update(obsolete.allsuccessors(repo.obsstore, mutable))
            known = (n for n in succs if n in nm)
            validdests = set(repo.set('%ln::', known))
        return new in validdests
    else:
        return old.descendant(new)
