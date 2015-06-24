# Mercurial bookmark support code
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
from mercurial.i18n import _
from mercurial.node import hex, bin
from mercurial import encoding, util, obsolete, lock as lockmod
import errno

class bmstore(dict):
    """Storage for bookmarks.

    This object should do all bookmark reads and writes, so that it's
    fairly simple to replace the storage underlying bookmarks without
    having to clone the logic surrounding bookmarks.

    This particular bmstore implementation stores bookmarks as
    {hash}\s{name}\n (the same format as localtags) in
    .hg/bookmarks. The mapping is stored as {name: nodeid}.

    This class does NOT handle the "active" bookmark state at this
    time.
    """

    def __init__(self, repo):
        dict.__init__(self)
        self._repo = repo
        try:
            bkfile = self.getbkfile(repo)
            for line in bkfile:
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
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise

    def getbkfile(self, repo):
        bkfile = None
        if 'HG_PENDING' in os.environ:
            try:
                bkfile = repo.vfs('bookmarks.pending')
            except IOError as inst:
                if inst.errno != errno.ENOENT:
                    raise
        if bkfile is None:
            bkfile = repo.vfs('bookmarks')
        return bkfile

    def recordchange(self, tr):
        """record that bookmarks have been changed in a transaction

        The transaction is then responsible for updating the file content."""
        tr.addfilegenerator('bookmarks', ('bookmarks',), self._write,
                            location='plain')
        tr.hookargs['bookmark_moved'] = '1'

    def write(self):
        '''Write bookmarks

        Write the given bookmark => hash dictionary to the .hg/bookmarks file
        in a format equal to those of localtags.

        We also store a backup of the previous state in undo.bookmarks that
        can be copied back on rollback.
        '''
        repo = self._repo
        self._writerepo(repo)
        repo.invalidatevolatilesets()

    def _writerepo(self, repo):
        """Factored out for extensibility"""
        if repo._activebookmark not in self:
            deactivate(repo)

        wlock = repo.wlock()
        try:

            file = repo.vfs('bookmarks', 'w', atomictemp=True)
            self._write(file)
            file.close()

            # touch 00changelog.i so hgweb reloads bookmarks (no lock needed)
            try:
                repo.svfs.utime('00changelog.i', None)
            except OSError:
                pass

        finally:
            wlock.release()

    def _write(self, fp):
        for name, node in self.iteritems():
            fp.write("%s %s\n" % (hex(node), encoding.fromlocal(name)))

def readactive(repo):
    """
    Get the active bookmark. We can have an active bookmark that updates
    itself as we commit. This function returns the name of that bookmark.
    It is stored in .hg/bookmarks.current
    """
    mark = None
    try:
        file = repo.vfs('bookmarks.current')
    except IOError as inst:
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

def activate(repo, mark):
    """
    Set the given bookmark to be 'active', meaning that this bookmark will
    follow new commits that are made.
    The name is recorded in .hg/bookmarks.current
    """
    if mark not in repo._bookmarks:
        raise AssertionError('bookmark %s does not exist!' % mark)

    active = repo._activebookmark
    if active == mark:
        return

    wlock = repo.wlock()
    try:
        file = repo.vfs('bookmarks.current', 'w', atomictemp=True)
        file.write(encoding.fromlocal(mark))
        file.close()
    finally:
        wlock.release()
    repo._activebookmark = mark

def deactivate(repo):
    """
    Unset the active bookmark in this reposiotry.
    """
    wlock = repo.wlock()
    try:
        repo.vfs.unlink('bookmarks.current')
        repo._activebookmark = None
    except OSError as inst:
        if inst.errno != errno.ENOENT:
            raise
    finally:
        wlock.release()

def isactivewdirparent(repo):
    """
    Tell whether the 'active' bookmark (the one that follows new commits)
    points to one of the parents of the current working directory (wdir).

    While this is normally the case, it can on occasion be false; for example,
    immediately after a pull, the active bookmark can be moved to point
    to a place different than the wdir. This is solved by running `hg update`.
    """
    mark = repo._activebookmark
    marks = repo._bookmarks
    parents = [p.node() for p in repo[None].parents()]
    return (mark in marks and marks[mark] in parents)

def deletedivergent(repo, deletefrom, bm):
    '''Delete divergent versions of bm on nodes in deletefrom.

    Return True if at least one bookmark was deleted, False otherwise.'''
    deleted = False
    marks = repo._bookmarks
    divergent = [b for b in marks if b.split('@', 1)[0] == bm.split('@', 1)[0]]
    for mark in divergent:
        if mark == '@' or '@' not in mark:
            # can't be divergent by definition
            continue
        if mark and marks[mark] in deletefrom:
            if mark != bm:
                del marks[mark]
                deleted = True
    return deleted

def calculateupdate(ui, repo, checkout):
    '''Return a tuple (targetrev, movemarkfrom) indicating the rev to
    check out and where to move the active bookmark from, if needed.'''
    movemarkfrom = None
    if checkout is None:
        activemark = repo._activebookmark
        if isactivewdirparent(repo):
            movemarkfrom = repo['.'].node()
        elif activemark:
            ui.status(_("updating to active bookmark %s\n") % activemark)
            checkout = activemark
    return (checkout, movemarkfrom)

def update(repo, parents, node):
    deletefrom = parents
    marks = repo._bookmarks
    update = False
    active = repo._activebookmark
    if not active:
        return False

    if marks[active] in parents:
        new = repo[node]
        divs = [repo[b] for b in marks
                if b.split('@', 1)[0] == active.split('@', 1)[0]]
        anc = repo.changelog.ancestors([new.rev()])
        deletefrom = [b.node() for b in divs if b.rev() in anc or b == new]
        if validdest(repo, repo[marks[active]], new):
            marks[active] = new.node()
            update = True

    if deletedivergent(repo, deletefrom, active):
        update = True

    if update:
        marks.write()
    return update

def listbookmarks(repo):
    # We may try to list bookmarks on a repo type that does not
    # support it (e.g., statichttprepository).
    marks = getattr(repo, '_bookmarks', {})

    d = {}
    hasnode = repo.changelog.hasnode
    for k, v in marks.iteritems():
        # don't expose local divergent bookmarks
        if hasnode(v) and ('@' not in k or k.endswith('@')):
            d[k] = hex(v)
    return d

def pushbookmark(repo, key, old, new):
    w = l = tr = None
    try:
        w = repo.wlock()
        l = repo.lock()
        tr = repo.transaction('bookmarks')
        marks = repo._bookmarks
        existing = hex(marks.get(key, ''))
        if existing != old and existing != new:
            return False
        if new == '':
            del marks[key]
        else:
            if new not in repo:
                return False
            marks[key] = repo[new].node()
        marks.recordchange(tr)
        tr.close()
        return True
    finally:
        lockmod.release(tr, l, w)

def compare(repo, srcmarks, dstmarks,
            srchex=None, dsthex=None, targets=None):
    '''Compare bookmarks between srcmarks and dstmarks

    This returns tuple "(addsrc, adddst, advsrc, advdst, diverge,
    differ, invalid)", each are list of bookmarks below:

    :addsrc:  added on src side (removed on dst side, perhaps)
    :adddst:  added on dst side (removed on src side, perhaps)
    :advsrc:  advanced on src side
    :advdst:  advanced on dst side
    :diverge: diverge
    :differ:  changed, but changeset referred on src is unknown on dst
    :invalid: unknown on both side
    :same:    same on both side

    Each elements of lists in result tuple is tuple "(bookmark name,
    changeset ID on source side, changeset ID on destination
    side)". Each changeset IDs are 40 hexadecimal digit string or
    None.

    Changeset IDs of tuples in "addsrc", "adddst", "differ" or
     "invalid" list may be unknown for repo.

    This function expects that "srcmarks" and "dstmarks" return
    changeset ID in 40 hexadecimal digit string for specified
    bookmark. If not so (e.g. bmstore "repo._bookmarks" returning
    binary value), "srchex" or "dsthex" should be specified to convert
    into such form.

    If "targets" is specified, only bookmarks listed in it are
    examined.
    '''
    if not srchex:
        srchex = lambda x: x
    if not dsthex:
        dsthex = lambda x: x

    if targets:
        bset = set(targets)
    else:
        srcmarkset = set(srcmarks)
        dstmarkset = set(dstmarks)
        bset = srcmarkset | dstmarkset

    results = ([], [], [], [], [], [], [], [])
    addsrc = results[0].append
    adddst = results[1].append
    advsrc = results[2].append
    advdst = results[3].append
    diverge = results[4].append
    differ = results[5].append
    invalid = results[6].append
    same = results[7].append

    for b in sorted(bset):
        if b not in srcmarks:
            if b in dstmarks:
                adddst((b, None, dsthex(dstmarks[b])))
            else:
                invalid((b, None, None))
        elif b not in dstmarks:
            addsrc((b, srchex(srcmarks[b]), None))
        else:
            scid = srchex(srcmarks[b])
            dcid = dsthex(dstmarks[b])
            if scid == dcid:
                same((b, scid, dcid))
            elif scid in repo and dcid in repo:
                sctx = repo[scid]
                dctx = repo[dcid]
                if sctx.rev() < dctx.rev():
                    if validdest(repo, sctx, dctx):
                        advdst((b, scid, dcid))
                    else:
                        diverge((b, scid, dcid))
                else:
                    if validdest(repo, dctx, sctx):
                        advsrc((b, scid, dcid))
                    else:
                        diverge((b, scid, dcid))
            else:
                # it is too expensive to examine in detail, in this case
                differ((b, scid, dcid))

    return results

def _diverge(ui, b, path, localmarks, remotenode):
    '''Return appropriate diverged bookmark for specified ``path``

    This returns None, if it is failed to assign any divergent
    bookmark name.

    This reuses already existing one with "@number" suffix, if it
    refers ``remotenode``.
    '''
    if b == '@':
        b = ''
    # try to use an @pathalias suffix
    # if an @pathalias already exists, we overwrite (update) it
    if path.startswith("file:"):
        path = util.url(path).path
    for p, u in ui.configitems("paths"):
        if u.startswith("file:"):
            u = util.url(u).path
        if path == u:
            return '%s@%s' % (b, p)

    # assign a unique "@number" suffix newly
    for x in range(1, 100):
        n = '%s@%d' % (b, x)
        if n not in localmarks or localmarks[n] == remotenode:
            return n

    return None

def updatefromremote(ui, repo, remotemarks, path, trfunc, explicit=()):
    ui.debug("checking for updated bookmarks\n")
    localmarks = repo._bookmarks
    (addsrc, adddst, advsrc, advdst, diverge, differ, invalid, same
     ) = compare(repo, remotemarks, localmarks, dsthex=hex)

    status = ui.status
    warn = ui.warn
    if ui.configbool('ui', 'quietbookmarkmove', False):
        status = warn = ui.debug

    explicit = set(explicit)
    changed = []
    for b, scid, dcid in addsrc:
        if scid in repo: # add remote bookmarks for changes we already have
            changed.append((b, bin(scid), status,
                            _("adding remote bookmark %s\n") % (b)))
        elif b in explicit:
            explicit.remove(b)
            ui.warn(_("remote bookmark %s points to locally missing %s\n")
                    % (b, scid[:12]))

    for b, scid, dcid in advsrc:
        changed.append((b, bin(scid), status,
                        _("updating bookmark %s\n") % (b)))
    # remove normal movement from explicit set
    explicit.difference_update(d[0] for d in changed)

    for b, scid, dcid in diverge:
        if b in explicit:
            explicit.discard(b)
            changed.append((b, bin(scid), status,
                            _("importing bookmark %s\n") % (b)))
        else:
            snode = bin(scid)
            db = _diverge(ui, b, path, localmarks, snode)
            if db:
                changed.append((db, snode, warn,
                                _("divergent bookmark %s stored as %s\n") %
                                (b, db)))
            else:
                warn(_("warning: failed to assign numbered name "
                       "to divergent bookmark %s\n") % (b))
    for b, scid, dcid in adddst + advdst:
        if b in explicit:
            explicit.discard(b)
            changed.append((b, bin(scid), status,
                            _("importing bookmark %s\n") % (b)))
    for b, scid, dcid in differ:
        if b in explicit:
            explicit.remove(b)
            ui.warn(_("remote bookmark %s points to locally missing %s\n")
                    % (b, scid[:12]))

    if changed:
        tr = trfunc()
        for b, node, writer, msg in sorted(changed):
            localmarks[b] = node
            writer(msg)
        localmarks.recordchange(tr)

def incoming(ui, repo, other):
    '''Show bookmarks incoming from other to repo
    '''
    ui.status(_("searching for changed bookmarks\n"))

    r = compare(repo, other.listkeys('bookmarks'), repo._bookmarks,
                dsthex=hex)
    addsrc, adddst, advsrc, advdst, diverge, differ, invalid, same = r

    incomings = []
    if ui.debugflag:
        getid = lambda id: id
    else:
        getid = lambda id: id[:12]
    if ui.verbose:
        def add(b, id, st):
            incomings.append("   %-25s %s %s\n" % (b, getid(id), st))
    else:
        def add(b, id, st):
            incomings.append("   %-25s %s\n" % (b, getid(id)))
    for b, scid, dcid in addsrc:
        # i18n: "added" refers to a bookmark
        add(b, scid, _('added'))
    for b, scid, dcid in advsrc:
        # i18n: "advanced" refers to a bookmark
        add(b, scid, _('advanced'))
    for b, scid, dcid in diverge:
        # i18n: "diverged" refers to a bookmark
        add(b, scid, _('diverged'))
    for b, scid, dcid in differ:
        # i18n: "changed" refers to a bookmark
        add(b, scid, _('changed'))

    if not incomings:
        ui.status(_("no changed bookmarks found\n"))
        return 1

    for s in sorted(incomings):
        ui.write(s)

    return 0

def outgoing(ui, repo, other):
    '''Show bookmarks outgoing from repo to other
    '''
    ui.status(_("searching for changed bookmarks\n"))

    r = compare(repo, repo._bookmarks, other.listkeys('bookmarks'),
                srchex=hex)
    addsrc, adddst, advsrc, advdst, diverge, differ, invalid, same = r

    outgoings = []
    if ui.debugflag:
        getid = lambda id: id
    else:
        getid = lambda id: id[:12]
    if ui.verbose:
        def add(b, id, st):
            outgoings.append("   %-25s %s %s\n" % (b, getid(id), st))
    else:
        def add(b, id, st):
            outgoings.append("   %-25s %s\n" % (b, getid(id)))
    for b, scid, dcid in addsrc:
        # i18n: "added refers to a bookmark
        add(b, scid, _('added'))
    for b, scid, dcid in adddst:
        # i18n: "deleted" refers to a bookmark
        add(b, ' ' * 40, _('deleted'))
    for b, scid, dcid in advsrc:
        # i18n: "advanced" refers to a bookmark
        add(b, scid, _('advanced'))
    for b, scid, dcid in diverge:
        # i18n: "diverged" refers to a bookmark
        add(b, scid, _('diverged'))
    for b, scid, dcid in differ:
        # i18n: "changed" refers to a bookmark
        add(b, scid, _('changed'))

    if not outgoings:
        ui.status(_("no changed bookmarks found\n"))
        return 1

    for s in sorted(outgoings):
        ui.write(s)

    return 0

def summary(repo, other):
    '''Compare bookmarks between repo and other for "hg summary" output

    This returns "(# of incoming, # of outgoing)" tuple.
    '''
    r = compare(repo, other.listkeys('bookmarks'), repo._bookmarks,
                dsthex=hex)
    addsrc, adddst, advsrc, advdst, diverge, differ, invalid, same = r
    return (len(addsrc), len(adddst))

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
        return new.node() in obsolete.foreground(repo, [old.node()])
    else:
        # still an independent clause as it is lazier (and therefore faster)
        return old.descendant(new)
