# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Mercurial bookmark support code
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import struct
import typing

import bindings

from . import (
    encoding,
    error,
    git,
    lock as lockmod,
    mutation,
    pycompat,
    scmutil,
    txnutil,
    util,
    visibility,
)
from .i18n import _
from .node import bin, hex, nullid, short, wdirid
from .pycompat import decodeutf8, encodeutf8


# label constants
# until 3.5, bookmarks.current was the advertised name, not
# bookmarks.active, so we must use both to avoid breaking old
# custom styles
activebookmarklabel = "bookmarks.active bookmarks.current"

# namespace to use when recording an hg journal entry
journalremotebookmarktype = "remotebookmark"


def _getbkfile(repo):
    """Hook so that extensions that mess with the store can hook bm storage.

    For core, this just handles wether we should see pending
    bookmarks or the committed ones. Other extensions (like share)
    may need to tweak this behavior further.
    """

    fp, pending = txnutil.trypending(repo.root, repo.svfs, "bookmarks")
    return fp


class bmstore(dict):
    r"""Storage for bookmarks.

    This object should do all bookmark-related reads and writes, so
    that it's fairly simple to replace the storage underlying
    bookmarks without having to clone the logic surrounding
    bookmarks. This type also should manage the active bookmark, if
    any.

    This particular bmstore implementation stores bookmarks as
    {hash}\s{name}\n (the same format as localtags) in
    .hg/bookmarks. The mapping is stored as {name: nodeid}.
    """

    def __init__(self, repo):
        if util.istest():
            repo.hook("pre-bookmark-load", throw=True)
        dict.__init__(self)
        self._repo = repo
        self._clean = True
        self._aclean = True
        nm = repo.changelog.nodemap
        setitem = dict.__setitem__
        with _getbkfile(repo) as bkfile:
            data = bkfile.read()
        decoded = bindings.refencode.decodebookmarks(data)
        try:
            for refspec, node in sorted(decoded.items()):
                if node in nm:
                    refspec = encoding.tolocal(refspec)
                    setitem(self, refspec, node)
                else:
                    # This might happen if:
                    # - changelog was loaded, bookmarks are not loaded
                    # - bookmarks was changed to point to unknown nodes
                    # - bookmarks are loaded
                    #
                    # Try to mitigate by reloading changelog.
                    repo.invalidate()
                    nm = repo.changelog.nodemap
                    if node in nm:
                        refspec = encoding.tolocal(refspec)
                        setitem(self, refspec, node)
                        repo.ui.log("features", feature="fix-bookmark-changelog-order")
                    else:
                        repo.ui.log(
                            "features",
                            feature="fix-bookmark-changelog-order-failed",
                        )
                        repo.ui.warn(
                            _("unknown reference in .hg/bookmarks: %s %s\n")
                            % (refspec, hex(node))
                        )

        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
        self._active = _readactive(repo, self)

    @property
    def active(self):
        return self._active

    @active.setter
    def active(self, mark):
        if mark is not None and mark not in self:
            raise AssertionError("bookmark %s does not exist!" % mark)

        self._active = mark
        self._aclean = False

    def __setitem__(self, *args, **kwargs):
        msg = "'bookmarks[name] = node' is deprecated, " "use 'bookmarks.applychanges'"
        self._repo.ui.deprecwarn(msg, "4.3")
        self._set(*args, **kwargs)

    def _set(self, key, value):
        self._clean = False
        return dict.__setitem__(self, key, value)

    def __delitem__(self, key):
        msg = "'del bookmarks[name]' is deprecated, " "use 'bookmarks.applychanges'"
        self._repo.ui.deprecwarn(msg, "4.3")
        self._del(key)

    def _del(self, key):
        self._clean = False
        return dict.__delitem__(self, key)

    def applychanges(self, repo, tr, changes):
        """Apply a list of changes to bookmarks"""
        bmchanges = tr.changes.get("bookmarks")
        for name, node in changes:
            old = self.get(name)
            if node is None:
                self._del(name)
            else:
                self._set(name, node)
            if bmchanges is not None:
                # if a previous value exist preserve the "initial" value
                previous = bmchanges.get(name)
                if previous is not None:
                    old = previous[0]
                bmchanges[name] = (old, node)
        self._recordchange(tr)

    def recordchange(self, tr):
        msg = "'bookmarks.recorchange' is deprecated, " "use 'bookmarks.applychanges'"
        self._repo.ui.deprecwarn(msg, "4.3")
        return self._recordchange(tr)

    def _recordchange(self, tr):
        """record that bookmarks have been changed in a transaction

        The transaction is then responsible for updating the file content."""
        tr.addfilegenerator("bookmarks", ("bookmarks",), self._write, location="")
        tr.hookargs["bookmark_moved"] = "1"

    def _writeactive(self):
        if self._aclean:
            return
        with self._repo.wlock():
            if self._active is not None:
                f = self._repo.localvfs(
                    "bookmarks.current", "w", atomictemp=True, checkambig=True
                )
                try:
                    f.write(encodeutf8(encoding.fromlocal(self._active)))
                finally:
                    f.close()
            else:
                self._repo.localvfs.tryunlink("bookmarks.current")
        self._aclean = True

    def _write(self, fp):
        encoded = bindings.refencode.encodebookmarks(self)
        fp.write(encoded)
        self._clean = True
        self._repo.invalidatevolatilesets()

    def expandname(self, bname):
        if bname == ".":
            if self.active:
                return self.active
            else:
                raise error.Abort(_("no active bookmark"))
        return bname

    def checkconflict(self, mark, force=False, target=None):
        """check repo for a potential clash of mark with an existing bookmark,
        branch, or hash

        If target is supplied, then check that we are moving the bookmark
        forward.

        If force is supplied, then forcibly move the bookmark to a new commit
        regardless if it is a move forward.

        If divergent bookmark are to be deleted, they will be returned as list.
        """
        cur = self._repo.changectx(".").node()
        if mark in self and not force:
            if target:
                if self[mark] == target and target == cur:
                    # re-activating a bookmark
                    return []
                rev = self._repo[target].rev()
                anc = self._repo.changelog.ancestors([rev])
                bmctx = self._repo[self[mark]]
                divs = [
                    self._repo[b].node()
                    for b in self
                    if b.split("@", 1)[0] == mark.split("@", 1)[0]
                ]

                # allow resolving a single divergent bookmark even if moving
                # the bookmark across branches when a revision is specified
                # that contains a divergent bookmark
                if bmctx.rev() not in anc and target in divs:
                    return divergent2delete(self._repo, [target], mark)

                deletefrom = [
                    b for b in divs if self._repo[b].rev() in anc or b == target
                ]
                delbms = divergent2delete(self._repo, deletefrom, mark)
                if validdest(self._repo, bmctx, self._repo[target]):
                    self._repo.ui.status(
                        _("moving bookmark '%s' forward from %s\n")
                        % (mark, short(bmctx.node()))
                    )
                    return delbms
            raise error.Abort(
                _("bookmark '%s' already exists " "(use -f to force)") % mark
            )
        if len(mark) > 3 and not force:
            try:
                shadowhash = mark in self._repo
            except error.LookupError:  # ambiguous identifier
                shadowhash = False
            if shadowhash:
                self._repo.ui.warn(
                    _(
                        "bookmark %s matches a changeset hash\n"
                        "(did you leave a -r out of an 'hg bookmark' "
                        "command?)\n"
                    )
                    % mark
                )
        return []


def _readactive(repo, marks):
    """
    Get the active bookmark. We can have an active bookmark that updates
    itself as we commit. This function returns the name of that bookmark.
    It is stored in .hg/bookmarks.current
    """
    mark = repo.localvfs.tryreadutf8("bookmarks.current") or None
    if mark and mark not in marks:
        mark = None
    return mark


def activate(repo, mark):
    """
    Set the given bookmark to be 'active', meaning that this bookmark will
    follow new commits that are made.
    The name is recorded in .hg/bookmarks.current
    """
    repo._bookmarks.active = mark
    repo._bookmarks._writeactive()


def deactivate(repo):
    """
    Unset the active bookmark in this repository.
    """
    repo._bookmarks.active = None
    repo._bookmarks._writeactive()


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
    return mark in marks and marks[mark] in parents


def divergent2delete(repo, deletefrom, bm):
    """find divergent versions of bm on nodes in deletefrom.

    the list of bookmark to delete."""
    todelete = []
    marks = repo._bookmarks
    divergent = [b for b in marks if b.split("@", 1)[0] == bm.split("@", 1)[0]]
    for mark in divergent:
        if mark == "@" or "@" not in mark:
            # can't be divergent by definition
            continue
        if mark and marks[mark] in deletefrom:
            if mark != bm:
                todelete.append(mark)
    return todelete


def headsforactive(repo):
    """Given a repo with an active bookmark, return divergent bookmark nodes.

    Args:
      repo: A repository with an active bookmark.

    Returns:
      A list of binary node ids that is the full list of other
      revisions with bookmarks divergent from the active bookmark. If
      there were no divergent bookmarks, then this list will contain
      only one entry.
    """
    if not repo._activebookmark:
        raise ValueError("headsforactive() only makes sense with an active bookmark")
    name = repo._activebookmark.split("@", 1)[0]
    heads = []
    for mark, n in pycompat.iteritems(repo._bookmarks):
        if mark.split("@", 1)[0] == name:
            heads.append(n)
    return heads


def calculateupdate(ui, repo, checkout):
    """Return a tuple (targetrev, movemarkfrom) indicating the rev to
    check out and where to move the active bookmark from, if needed."""
    movemarkfrom = None
    if checkout is None:
        activemark = repo._activebookmark
        if isactivewdirparent(repo):
            movemarkfrom = repo["."].node()
        elif activemark:
            ui.status(_("updating to active bookmark %s\n") % activemark)
            checkout = activemark
    return (checkout, movemarkfrom)


def update(repo, parents, node):
    deletefrom = parents
    marks = repo._bookmarks
    active = marks.active
    if not active:
        return False

    bmchanges = []
    if marks[active] in parents:
        new = repo[node]
        divs = [repo[b] for b in marks if b.split("@", 1)[0] == active.split("@", 1)[0]]
        anc = repo.changelog.ancestors([new.rev()])
        deletefrom = [b.node() for b in divs if b.rev() in anc or b == new]
        if validdest(repo, repo[marks[active]], new):
            bmchanges.append((active, new.node()))

    for bm in divergent2delete(repo, deletefrom, active):
        bmchanges.append((bm, None))

    if bmchanges:
        lock = tr = None
        try:
            lock = repo.lock()
            tr = repo.transaction("bookmark")
            marks.applychanges(repo, tr, bmchanges)
            tr.close()
        finally:
            lockmod.release(tr, lock)
    return bool(bmchanges)


def listbinbookmarks(repo):
    # We may try to list bookmarks on a repo type that does not
    # support it.
    marks = getattr(repo, "_bookmarks", {})

    hasnode = repo.changelog.hasnode
    for k, v in pycompat.iteritems(marks):
        # don't expose local divergent bookmarks
        if hasnode(v) and ("@" not in k or k.endswith("@")):
            yield k, v


def listbookmarks(repo):
    d = {}
    for book, node in listbinbookmarks(repo):
        d[book] = hex(node)
    return d


def pushbookmark(repo, key, old, new):
    w = l = tr = None
    try:
        w = repo.wlock()
        l = repo.lock()
        tr = repo.transaction("bookmarks")
        marks = repo._bookmarks
        existing = hex(marks.get(key, b""))
        if existing != old and existing != new:
            return False
        if new == "":
            changes = [(key, None)]
        else:
            if new not in repo:
                return False
            changes = [(key, repo[new].node())]
        marks.applychanges(repo, tr, changes)
        tr.close()
        return True
    finally:
        lockmod.release(tr, l, w)


def comparebookmarks(repo, srcmarks, dstmarks, targets=None):
    """Compare bookmarks between srcmarks and dstmarks

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

    If "targets" is specified, only bookmarks listed in it are
    examined.
    """

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
                adddst((b, None, dstmarks[b]))
            else:
                invalid((b, None, None))
        elif b not in dstmarks:
            addsrc((b, srcmarks[b], None))
        else:
            scid = srcmarks[b]
            dcid = dstmarks[b]
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
    """Return appropriate diverged bookmark for specified ``path``

    This returns None, if it is failed to assign any divergent
    bookmark name.

    This reuses already existing one with "@number" suffix, if it
    refers ``remotenode``.
    """
    if b == "@":
        b = ""
    # try to use an @pathalias suffix
    # if an @pathalias already exists, we overwrite (update) it
    if path.startswith("file:"):
        path = util.url(path).path
    for p, u in ui.configitems("paths"):
        if u.startswith("file:"):
            u = util.url(u).path
        if path == u:
            return "%s@%s" % (b, p)

    # assign a unique "@number" suffix newly
    for x in range(1, 100):
        n = "%s@%d" % (b, x)
        if n not in localmarks or localmarks[n] == remotenode:
            return n

    return None


def unhexlifybookmarks(marks):
    binremotemarks = {}
    for name, node in marks.items():
        binremotemarks[name] = bin(node)
    return binremotemarks


_binaryentry = struct.Struct(">20sH")


def binaryencode(bookmarks: typing.Iterable[typing.Tuple[str, bytes]]) -> bytes:
    """encode a '(bookmark, node)' iterable into a binary stream

    the binary format is:

        <node><bookmark-length><bookmark-name>

    :node: is a 20 bytes binary node,
    :bookmark-length: an unsigned short,
    :bookmark-name: the name of the bookmark (of length <bookmark-length>)

    wdirid (all bits set) will be used as a special value for "missing"
    """
    binarydata = []
    for book, node in bookmarks:
        if not node:  # None or ''
            node = wdirid
        book = pycompat.encodeutf8(book)
        binarydata.append(_binaryentry.pack(node, len(book)))
        binarydata.append(book)
    return b"".join(binarydata)


def binarydecode(stream):
    """decode a binary stream into an '(bookmark, node)' iterable

    the binary format is:

        <node><bookmark-length><bookmark-name>

    :node: is a 20 bytes binary node,
    :bookmark-length: an unsigned short,
    :bookmark-name: the name of the bookmark (of length <bookmark-length>))

    wdirid (all bits set) will be used as a special value for "missing"
    """
    entrysize = _binaryentry.size
    books = []
    while True:
        entry = stream.read(entrysize)
        if len(entry) < entrysize:
            if entry:
                raise error.Abort(_("bad bookmark stream"))
            break
        node, length = _binaryentry.unpack(entry)
        bookmark = stream.read(length)
        if len(bookmark) < length:
            if entry:
                raise error.Abort(_("bad bookmark stream"))
        if node == wdirid:
            node = None
        books.append((decodeutf8(bookmark), node))
    return books


def updatefromremote(ui, repo, remotemarks, path, trfunc, explicit=()):
    ui.debug("checking for updated bookmarks\n")
    localmarks = repo._bookmarks
    (addsrc, adddst, advsrc, advdst, diverge, differ, invalid, same) = comparebookmarks(
        repo, remotemarks, localmarks
    )

    status = ui.status
    warn = ui.warn
    if ui.configbool("ui", "quietbookmarkmove"):
        status = warn = ui.debug

    explicit = set(explicit)
    changed = []
    for b, scid, dcid in addsrc:
        if scid in repo:  # add remote bookmarks for changes we already have
            changed.append((b, scid, status, _("adding remote bookmark %s\n") % b))
        elif b in explicit:
            explicit.remove(b)
            ui.warn(
                _("remote bookmark %s points to locally missing %s\n")
                % (b, hex(scid)[:12])
            )

    for b, scid, dcid in advsrc:
        changed.append((b, scid, status, _("updating bookmark %s\n") % b))
    # remove normal movement from explicit set
    explicit.difference_update(d[0] for d in changed)

    for b, scid, dcid in diverge:
        if b in explicit:
            explicit.discard(b)
            changed.append((b, scid, status, _("importing bookmark %s\n") % b))
        else:
            db = _diverge(ui, b, path, localmarks, scid)
            if db:
                changed.append(
                    (
                        db,
                        scid,
                        warn,
                        _("divergent bookmark %s stored as %s\n") % (b, db),
                    )
                )
            else:
                warn(
                    _(
                        "warning: failed to assign numbered name "
                        "to divergent bookmark %s\n"
                    )
                    % b
                )
    for b, scid, dcid in adddst + advdst:
        if b in explicit:
            explicit.discard(b)
            changed.append((b, scid, status, _("importing bookmark %s\n") % b))
    for b, scid, dcid in differ:
        if b in explicit:
            explicit.remove(b)
            ui.warn(
                _("remote bookmark %s points to locally missing %s\n")
                % (b, hex(scid)[:12])
            )

    if changed:
        tr = trfunc()
        changes = []
        for b, node, writer, msg in sorted(changed):
            changes.append((b, node))
            writer(msg)
        localmarks.applychanges(repo, tr, changes)


def incoming(ui, repo, other):
    """Show bookmarks incoming from other to repo"""
    ui.status(_("searching for changed bookmarks\n"))

    remotemarks = unhexlifybookmarks(other.listkeys("bookmarks"))
    r = comparebookmarks(repo, remotemarks, repo._bookmarks)
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
        add(b, hex(scid), _("added"))
    for b, scid, dcid in advsrc:
        # i18n: "advanced" refers to a bookmark
        add(b, hex(scid), _("advanced"))
    for b, scid, dcid in diverge:
        # i18n: "diverged" refers to a bookmark
        add(b, hex(scid), _("diverged"))
    for b, scid, dcid in differ:
        # i18n: "changed" refers to a bookmark
        add(b, hex(scid), _("changed"))

    if not incomings:
        ui.status(_("no changed bookmarks found\n"))
        return 1

    for s in sorted(incomings):
        ui.write(s)

    return 0


def outgoing(ui, repo, other):
    """Show bookmarks outgoing from repo to other"""
    ui.status(_("searching for changed bookmarks\n"))

    remotemarks = unhexlifybookmarks(other.listkeys("bookmarks"))
    r = comparebookmarks(repo, repo._bookmarks, remotemarks)
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
        add(b, hex(scid), _("added"))
    for b, scid, dcid in adddst:
        # i18n: "deleted" refers to a bookmark
        add(b, " " * 40, _("deleted"))
    for b, scid, dcid in advsrc:
        # i18n: "advanced" refers to a bookmark
        add(b, hex(scid), _("advanced"))
    for b, scid, dcid in diverge:
        # i18n: "diverged" refers to a bookmark
        add(b, hex(scid), _("diverged"))
    for b, scid, dcid in differ:
        # i18n: "changed" refers to a bookmark
        add(b, hex(scid), _("changed"))

    if not outgoings:
        ui.status(_("no changed bookmarks found\n"))
        return 1

    for s in sorted(outgoings):
        ui.write(s)

    return 0


def summary(repo, other):
    """Compare bookmarks between repo and other for "hg summary" output

    This returns "(# of incoming, # of outgoing)" tuple.
    """
    remotemarks = unhexlifybookmarks(other.listkeys("bookmarks"))
    r = comparebookmarks(repo, remotemarks, repo._bookmarks)
    addsrc, adddst, advsrc, advdst, diverge, differ, invalid, same = r
    return (len(addsrc), len(adddst))


def validdest(repo, old, new):
    """Is the new bookmark destination a valid update from the old one"""
    if old == new:
        # Old == new -> nothing to update.
        return False
    elif not old:
        # old is nullrev, anything is valid.
        # (new != nullrev has been excluded by the previous check)
        return True
    elif mutation.enabled(repo):
        return new.node() in mutation.foreground(repo, [old.node()])
    else:
        # still an independent clause as it is lazier (and therefore faster)
        return old.descendant(new)


def checkformat(repo, mark):
    """return a valid version of a potential bookmark name

    Raises an abort error if the bookmark name is not valid.
    """
    mark = mark.strip()
    if not mark:
        raise error.Abort(_("bookmark names cannot consist entirely of " "whitespace"))
    scmutil.checknewlabel(repo, mark, "bookmark")
    return mark


def delete(repo, tr, names):
    """remove a mark from the bookmark store

    Raises an abort error if mark does not exist.
    """
    marks = repo._bookmarks
    changes = []
    for mark in names:
        if mark not in marks:
            raise error.Abort(_("bookmark '%s' does not exist") % mark)
        if mark == repo._activebookmark:
            deactivate(repo)
        changes.append((mark, None))
    marks.applychanges(repo, tr, changes)


def rename(repo, tr, old, new, force=False, inactive=False):
    """rename a bookmark from old to new

    If force is specified, then the new name can overwrite an existing
    bookmark.

    If inactive is specified, then do not activate the new bookmark.

    Raises an abort error if old is not in the bookmark store.
    """
    marks = repo._bookmarks
    mark = checkformat(repo, new)
    if old not in marks:
        raise error.Abort(_("bookmark '%s' does not exist") % old)
    changes = []
    for bm in marks.checkconflict(mark, force):
        changes.append((bm, None))
    changes.extend([(mark, marks[old]), (old, None)])
    marks.applychanges(repo, tr, changes)
    if repo._activebookmark == old and not inactive:
        activate(repo, mark)


def addbookmarks(repo, tr, names, rev=None, force=False, inactive=False):
    """add a list of bookmarks

    If force is specified, then the new name can overwrite an existing
    bookmark.

    If inactive is specified, then do not activate any bookmark. Otherwise, the
    first bookmark is activated.

    Raises an abort error if old is not in the bookmark store.
    """
    marks = repo._bookmarks
    cur = repo.changectx(".").node()
    newact = None
    changes = []
    for mark in names:
        mark = checkformat(repo, mark)
        if newact is None:
            newact = mark
        if inactive and mark == repo._activebookmark:
            deactivate(repo)
            return
        tgt = cur
        if rev:
            tgt = scmutil.revsingle(repo, rev).node()
        for bm in marks.checkconflict(mark, force, tgt):
            changes.append((bm, None))
        changes.append((mark, tgt))
    marks.applychanges(repo, tr, changes)
    if not inactive and cur == marks[newact] and not rev:
        activate(repo, newact)
    elif cur != tgt and newact == repo._activebookmark:
        deactivate(repo)


def _printbookmarks(ui, repo, bmarks, **opts):
    """private method to print bookmarks

    Provides a way for extensions to control how bookmarks are printed (e.g.
    prepend or postpend names)
    """
    fm = ui.formatter("bookmarks", opts)
    hexfn = fm.hexfunc
    if len(bmarks) == 0 and fm.isplain():
        ui.status(_("no bookmarks set\n"))
    for bmark, (n, prefix, label) in sorted(pycompat.iteritems(bmarks)):
        fm.startitem()
        if not ui.quiet:
            fm.plain(" %s " % prefix, label=label)
        fm.write("bookmark", "%s", bmark, label=label)
        pad = " " * (25 - encoding.colwidth(bmark))
        if ui.plain():
            fm.condwrite(
                not ui.quiet,
                "rev node",
                pad + " %d:%s",
                repo.changelog.rev(n),
                hexfn(n),
                label=label,
            )
        else:
            fm.condwrite(not ui.quiet, "node", pad + " %s", hexfn(n), label=label)
        fm.data(active=(activebookmarklabel in label))
        fm.plain("\n")
    fm.end()


def printbookmarks(ui, repo, **opts):
    """print bookmarks to a formatter

    Provides a way for extensions to control how bookmarks are printed.
    """
    marks = repo._bookmarks
    bmarks = {}
    for bmark, n in sorted(pycompat.iteritems(marks)):
        active = repo._activebookmark
        if bmark == active:
            prefix, label = "*", activebookmarklabel
        else:
            prefix, label = " ", ""

        bmarks[bmark] = (n, prefix, label)
    _printbookmarks(ui, repo, bmarks, **opts)


def preparehookargs(name, old, new):
    if new is None:
        new = b""
    if old is None:
        old = b""
    return {"bookmark": name, "node": hex(new), "oldnode": hex(old)}


def reachablerevs(repo, bookmarks):
    """revisions reachable only from the given bookmarks

    Returns a revset matching all commits that are only reachable by the given
    bookmarks.
    """
    repobookmarks = repo._bookmarks
    if not bookmarks.issubset(repobookmarks):
        raise error.Abort(
            _("bookmark not found: %s")
            % ", ".join(
                "'%s'" % bookmark
                for bookmark in sorted(bookmarks - set(repobookmarks.keys()))
            )
        )

    nodes = [repobookmarks[bookmark] for bookmark in bookmarks]

    # Compute nodes that are pointed to by other bookmarks.  This is not the
    # same as 'bookmark() - nodes', as it includes nodes that are pointed to by
    # both bookmarks we are deleting and other bookmarks.
    othernodes = [
        node
        for bookmark, node in pycompat.iteritems(repobookmarks)
        if bookmark not in bookmarks
    ]

    return repo.revs("(%ln) %% (head() - (%ln) + (%ln))", nodes, nodes, othernodes)


class remotenames(dict):
    """This class encapsulates all the remotenames state. It also contains
    methods to access that state in convenient ways. Remotenames are lazy
    loaded. Whenever client code needs to ensure the freshest copy of
    remotenames, use the `clearnames` method to force an eventual load.
    """

    # Count of changes. Useful for cache invalidation.
    _changecount = 0

    def __init__(self, repo, *args):
        dict.__init__(self, *args)
        self._repo = repo
        self.clearnames()

    def clearnames(self):
        """Clear all remote names state"""
        self["bookmarks"] = lazyremotenamedict("bookmarks", self._repo)
        self._invalidatecache()
        self._loadednames = False
        self._changecount += 1

    def _invalidatecache(self):
        self._node2marks = None
        self._hoist2nodes = None
        self._node2hoists = None
        self._node2branch = None
        self._changecount += 1

    def applychanges(self, changes, override=True):
        # Only supported for bookmarks
        bmchanges = changes.get("bookmarks", {})
        remotepathbooks = {}
        for remotename, node in pycompat.iteritems(bmchanges):
            path, name = splitremotename(remotename)
            remotepathbooks.setdefault(path, {})[name] = node

        self._changecount += 1
        saveremotenames(self._repo, remotepathbooks, override)

    def mark2nodes(self):
        return self["bookmarks"]

    def node2marks(self):
        if not self._node2marks:
            mark2nodes = self.mark2nodes()
            self._node2marks = {}
            for name, node in mark2nodes.items():
                self._node2marks.setdefault(node[0], []).append(name)
        return self._node2marks

    def hoist2nodes(self, hoist):
        if not self._hoist2nodes:
            mark2nodes = self.mark2nodes()
            self._hoist2nodes = {}
            hoist += "/"
            for name, node in mark2nodes.items():
                if name.startswith(hoist):
                    name = name[len(hoist) :]
                    self._hoist2nodes[name] = node
        return self._hoist2nodes

    def node2hoists(self, hoist):
        if not self._node2hoists:
            mark2nodes = self.mark2nodes()
            self._node2hoists = {}
            hoist += "/"
            for name, node in mark2nodes.items():
                if name.startswith(hoist):
                    name = name[len(hoist) :]
                    self._node2hoists.setdefault(node[0], []).append(name)
        return self._node2hoists

    def get(self, name):
        """Resolve a remote bookmark. Return None if the bookmark does not exist"""
        nodes = self["bookmarks"].get(name)
        if nodes:
            return nodes[0]
        else:
            return None

    @property
    def changecount(self):
        return self._changecount


def saveremotenames(repo, remotebookmarks, override=True):
    """
    remotebookmarks has the format {name: {name: hexnode | None}}.
    For example: {"remote": {"master": "0" * 40}}

    If override is False, update existing entries.
    If override is True, bookmarks with the same "remote" name (ex. "remote")
    will be replaced.
    """
    if not remotebookmarks:
        return

    from . import extensions

    with repo.wlock(), repo.lock():
        if extensions.isenabled(repo.ui, "remotenames"):
            transition(repo, repo.ui)

        tr = repo.currenttransaction()
        if tr is not None:
            tr.addbackup("remotenames")

        # read in all data first before opening file to write
        olddata = set(readremotenames(repo))
        oldbooks = {}

        remotepaths = remotebookmarks.keys()
        newremotenamenodes = {}
        for hexnode, nametype, remote, rname in olddata:
            # Do not write 'default-push' names. See https://fburl.com/1rft34i8.
            if remote == "default-push":
                continue
            fullname = joinremotename(remote, rname)
            if remote not in remotepaths:
                newremotenamenodes[fullname] = bin(hexnode)
            elif nametype == "bookmarks":
                oldbooks[(remote, rname)] = hexnode
                if not override and rname not in remotebookmarks[remote]:
                    newremotenamenodes[fullname] = bin(hexnode)

        journal = []
        nm = repo.changelog.nodemap
        missingnode = False
        for remote, rmbookmarks in pycompat.iteritems(remotebookmarks):
            # Do not write 'default-push' names. See https://fburl.com/1rft34i8.
            if remote == "default-push":
                continue
            rmbookmarks = {} if rmbookmarks is None else rmbookmarks
            for name, hexnode in pycompat.iteritems(rmbookmarks):
                oldnode = oldbooks.get((remote, name), hex(nullid))
                newnode = hexnode
                if not bin(newnode) in nm:
                    # node is unknown locally, don't change the bookmark
                    missingnode = True
                    newnode = oldnode
                if newnode != hex(nullid):
                    fullname = joinremotename(remote, name)
                    newremotenamenodes[fullname] = bin(newnode)
                    if newnode != oldnode:
                        journal.append(
                            (joinremotename(remote, name), bin(oldnode), bin(newnode))
                        )
        repo.ui.log("remotenamesmissingnode", remotenamesmissingnode=str(missingnode))

        repo.svfs.write("remotenames", encoderemotenames(newremotenamenodes))

        _recordbookmarksupdate(repo, journal)

        # Old paths have been deleted, refresh remotenames
        if util.safehasattr(repo, "_remotenames"):
            repo._remotenames.clearnames()

        # If narrowheads is enabled, updating remotenames can affect phases
        # (and other revsets). Therefore invalidate them.
        if "narrowheads" in repo.storerequirements:
            repo.invalidatevolatilesets()


encoderemotenames = bindings.refencode.encoderemotenames
decoderemotenames = bindings.refencode.decoderemotenames


class lazyremotenamedict(pycompat.Mapping):
    """Read-only dict-like Class to lazily resolve remotename entries

    We are doing that because remotenames startup was slow.
    We lazily read the remotenames file once to figure out the potential entries
    and store them in self.potentialentries. Then when asked to resolve an
    entry, if it is not in self.potentialentries, then it isn't there, if it
    is in self.potentialentries we resolve it and store the result in
    self.cache. We cannot be lazy is when asked all the entries (keys).
    """

    def __init__(self, kind, repo):
        self.cache = {}
        self.potentialentries = {}
        assert kind == "bookmarks"  # only support bookmarks
        self._kind = kind
        self._repo = repo
        self.loaded = False

    def _load(self):
        """Read the remotenames file, store entries matching selected kind"""
        self.loaded = True
        repo = self._repo
        alias_default = repo.ui.configbool("remotenames", "alias.default")
        entries = list(readremotenames(repo))
        threshold = repo.ui.configint("remotenames", "autocleanupthreshold") or 0
        # Do not clean up refs for git, which might have a lot of refs.
        # Doing so causes surprises: tags, remote refs will be gone unexpectedly.
        if not git.isgitformat(repo) and threshold > 0 and len(entries) > threshold:
            repo.ui.status_err(
                _(
                    "attempt to clean up remote bookmarks since they exceed threshold %s\n"
                )
                % threshold
            )
            try:
                removednames = cleanupremotenames(repo)
                if removednames:
                    repo.ui.log(
                        "features",
                        fullargs=repr(pycompat.sysargv),
                        feature="auto-clean-remotenames",
                    )
                    entries = list(readremotenames(repo))
            except Exception as ex:
                # happens if metalog is not used, etc.
                # not fatal
                repo.ui.warn(_("failed to clean up remote bookmarks: %s\n") % ex)
        for node, nametype, remote, rname in entries:
            if nametype != self._kind:
                continue
            # handle alias_default here
            if remote != "default" and rname == "default" and alias_default:
                name = remote
            else:
                name = joinremotename(remote, rname)
            self.potentialentries[name] = (node, nametype, remote, rname)

    def _resolvedata(self, potentialentry):
        """Check that the node for potentialentry exists and return it"""
        if not potentialentry in self.potentialentries:
            return None
        hexnode, nametype, remote, rname = self.potentialentries[potentialentry]
        repo = self._repo
        binnode = bin(hexnode)
        try:
            repo.changelog.rev(binnode)
        except LookupError as e:
            if rname not in selectivepullinitbookmarknames(repo):
                # Not a critical bookmark.
                return None
            raise error.RepoLookupError(
                _("remotename entry %s (%s) cannot be found: %s")
                % (potentialentry, hexnode, e),
                hint=_("try '@prog@ doctor' to attempt to fix it"),
            )
        # Skip closed branches
        if nametype == "branches" and repo[binnode].closesbranch():
            return None
        return [binnode]

    def __getitem__(self, key):
        if not self.loaded:
            self._load()
        val = self._fetchandcache(key)
        if val is not None:
            return val
        else:
            raise KeyError()

    def _fetchandcache(self, key):
        if key in self.cache:
            return self.cache[key]
        val = self._resolvedata(key)
        if val is not None:
            self.cache[key] = val
            return val
        else:
            return None

    # pyre-fixme[15]: `keys` overrides method defined in `Mapping` inconsistently.
    def keys(self) -> typing.AbstractSet[str]:
        """Get a list of bookmark names"""
        if not self.loaded:
            self._load()
        return self.potentialentries.keys()

    def items(self):
        """Iterate over (name, node) tuples"""
        if not self.loaded:
            self._load()
        for k, vtup in pycompat.iteritems(self.potentialentries):
            yield (k, [bin(vtup[0])])

    def __iter__(self):
        for k, v in self.items():
            yield k

    def __len__(self):
        return len(list(self.keys()))


def transition(repo, ui):
    """
    Help with transitioning to using a remotenames workflow.

    Allows deleting matching local bookmarks defined in a config file:

    [remotenames]
    transitionbookmarks = master
        stable

    TODO: Remove this once remotenames is default on everywhere.
    """
    transmarks = ui.configlist("remotenames", "transitionbookmarks")
    localmarks = repo._bookmarks
    changes = []
    for mark in transmarks:
        if mark in localmarks:
            changes.append((mark, None))  # delete this bookmark
    if changes:
        with repo.lock(), repo.transaction("remotenames") as tr:
            localmarks.applychanges(repo, tr, changes)

        message = ui.config("remotenames", "transitionmessage")
        if message:
            ui.warn(message + "\n")


def readremotenames(repo=None, svfs=None):
    if repo is not None:
        svfs = repo.svfs
    else:
        assert svfs is not None, "either repo or svfs should be set"
    return _readremotenamesfrom(svfs, "remotenames")


def _readremotenamesfrom(vfs, filename):
    for fullname, node in decoderemotenames(vfs.tryread(filename)).items():
        remote, name = splitremotename(fullname)
        yield (hex(node), "bookmarks", remote, name)


def mainbookmark(repo):
    """Get the "main" bookmark that represents the main commit history."""
    names = repo.ui.configlist("remotenames", "selectivepulldefault")
    if not names:
        # Fallback
        return "main"
    else:
        return names[0]


def selectivepullinitbookmarknames(repo):
    """Returns set of initial remote bookmarks names"""
    if "emergencychangelog" in repo.storerequirements:
        # In emergencychangelog mode, only care about the main bookmark.
        # Checking other bookmarks is likely going to make the server
        # do more work, or trigger commit graph code paths on the server
        # that is likely broken or slow.
        return [mainbookmark(repo)]
    return repo.ui.configlist("remotenames", "selectivepulldefault")


def selectivepullinitbookmarkfullnames(repo):
    """Returns set of initial remote bookmarks full names"""
    return [
        "%s/%s" % (repo.ui.config("remotenames", "hoist"), name)
        for name in selectivepullinitbookmarknames(repo)
    ]


def selectivepullbookmarknames(repo, remote=None):
    """Returns the bookmark names that should be pulled during a pull."""
    initbooks = selectivepullinitbookmarknames(repo)
    if remote is not None and "emergencychangelog" not in repo.storerequirements:
        for node, nametype, remotepath, name in readremotenames(repo):
            if nametype == "bookmarks" and remotepath == remote:
                initbooks.append(name)
        initbooks = util.dedup(initbooks)
    if not initbooks:
        raise error.Abort(_("no bookmarks to subscribe specified for selectivepull"))
    return initbooks


def cleanupremotenames(repo):
    """Remove non-critical remotenames that do not have draft descendants

    Return a list of removed names.
    """
    metalog = repo.metalog()

    essentialnames = selectivepullinitbookmarkfullnames(repo)
    namenodes = decoderemotenames(metalog["remotenames"])
    essentialpublicheads = [namenodes[n] for n in essentialnames if n in namenodes]

    # referred by draft visible heads that are not remotenames themselves
    # i.e. if draft heads match the remotenames then they are still cleaned up.
    # remotenames with visible draft children will stay.
    referredheads = repo.changelog._visibleheads.heads
    referrednodes = repo.dageval(
        lambda: only(parents(referredheads), essentialpublicheads)
    )
    newnamenodes = {
        fullname: node
        for fullname, node in namenodes.items()
        if fullname in essentialnames or node in referrednodes
    }
    removednames = sorted(set(namenodes) - set(newnamenodes))
    if removednames:
        # Also update visibleheads so we don't end up with massive draft
        # commits.
        removednodes = set(namenodes.values()) - set(newnamenodes.values())
        metalog["visibleheads"] = visibility.encodeheads(
            [h for h in referredheads if h not in removednodes]
        )
        metalog["remotenames"] = encoderemotenames(newnamenodes)
        metalog.commit("cleanupremotenames")
        threshold = 10
        if len(removednames) > threshold:
            partialnames = ", ".join(removednames[:threshold])
            names = "%s and %d others" % (
                partialnames,
                len(removednames) - threshold,
            )
        else:
            names = ", ".join(removednames)
        repo.ui.status_err(
            _("removed %s non-essential remote bookmarks: %s\n")
            % (
                len(removednames),
                names,
            )
        )
        repo.invalidatevolatilesets()
    return removednames


def _recordbookmarksupdate(repo, changes):
    """writes remotebookmarks changes to the journal

    'changes' - is a list of tuples '(remotebookmark, oldnode, newnode)''"""
    if util.safehasattr(repo, "journal"):
        repo.journal.recordmany(journalremotebookmarktype, changes)


def joinremotename(remote, ref):
    if ref:
        remote += "/" + ref
    return remote


def splitremotename(remote):
    name = ""
    if "/" in remote:
        remote, name = remote.split("/", 1)
    return remote, name


def remotenameforurl(ui, url):
    """Convert an URL to a remote name"""
    return ui.paths.getname(url, forremotenames=True)
