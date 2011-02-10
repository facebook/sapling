# Mercurial extension to provide the 'hg bookmark' command
#
# Copyright 2008 David Soria Parra <dsp@php.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''track a line of development with movable markers

Bookmarks are local movable markers to changesets. Every bookmark
points to a changeset identified by its hash. If you commit a
changeset that is based on a changeset that has a bookmark on it, the
bookmark shifts to the new changeset.

It is possible to use bookmark names in every revision lookup (e.g.
:hg:`merge`, :hg:`update`).

By default, when several bookmarks point to the same changeset, they
will all move forward together. It is possible to obtain a more
git-like experience by adding the following configuration option to
your configuration file::

  [bookmarks]
  track.current = True

This will cause Mercurial to track the bookmark that you are currently
using, and only update it. This is similar to git's approach to
branching.
'''

from mercurial.i18n import _
from mercurial.node import nullid, nullrev, bin, hex, short
from mercurial import util, commands, repair, extensions, pushkey, hg, url
from mercurial import revset, encoding
from mercurial import bookmarks
import os

def bookmark(ui, repo, mark=None, rev=None, force=False, delete=False, rename=None):
    '''track a line of development with movable markers

    Bookmarks are pointers to certain commits that move when
    committing. Bookmarks are local. They can be renamed, copied and
    deleted. It is possible to use bookmark names in :hg:`merge` and
    :hg:`update` to merge and update respectively to a given bookmark.

    You can use :hg:`bookmark NAME` to set a bookmark on the working
    directory's parent revision with the given name. If you specify
    a revision using -r REV (where REV may be an existing bookmark),
    the bookmark is assigned to that revision.

    Bookmarks can be pushed and pulled between repositories (see :hg:`help
    push` and :hg:`help pull`). This requires the bookmark extension to be
    enabled for both the local and remote repositories.
    '''
    hexfn = ui.debugflag and hex or short
    marks = repo._bookmarks
    cur   = repo.changectx('.').node()

    if rename:
        if rename not in marks:
            raise util.Abort(_("a bookmark of this name does not exist"))
        if mark in marks and not force:
            raise util.Abort(_("a bookmark of the same name already exists"))
        if mark is None:
            raise util.Abort(_("new bookmark name required"))
        marks[mark] = marks[rename]
        del marks[rename]
        if repo._bookmarkcurrent == rename:
            bookmarks.setcurrent(repo, mark)
        bookmarks.write(repo)
        return

    if delete:
        if mark is None:
            raise util.Abort(_("bookmark name required"))
        if mark not in marks:
            raise util.Abort(_("a bookmark of this name does not exist"))
        if mark == repo._bookmarkcurrent:
            bookmarks.setcurrent(repo, None)
        del marks[mark]
        bookmarks.write(repo)
        return

    if mark is not None:
        if "\n" in mark:
            raise util.Abort(_("bookmark name cannot contain newlines"))
        mark = mark.strip()
        if not mark:
            raise util.Abort(_("bookmark names cannot consist entirely of "
                               "whitespace"))
        if mark in marks and not force:
            raise util.Abort(_("a bookmark of the same name already exists"))
        if ((mark in repo.branchtags() or mark == repo.dirstate.branch())
            and not force):
            raise util.Abort(
                _("a bookmark cannot have the name of an existing branch"))
        if rev:
            marks[mark] = repo.lookup(rev)
        else:
            marks[mark] = repo.changectx('.').node()
        bookmarks.setcurrent(repo, mark)
        bookmarks.write(repo)
        return

    if mark is None:
        if rev:
            raise util.Abort(_("bookmark name required"))
        if len(marks) == 0:
            ui.status(_("no bookmarks set\n"))
        else:
            for bmark, n in marks.iteritems():
                if ui.configbool('bookmarks', 'track.current'):
                    current = repo._bookmarkcurrent
                    if bmark == current and n == cur:
                        prefix, label = '*', 'bookmarks.current'
                    else:
                        prefix, label = ' ', ''
                else:
                    if n == cur:
                        prefix, label = '*', 'bookmarks.current'
                    else:
                        prefix, label = ' ', ''

                if ui.quiet:
                    ui.write("%s\n" % bmark, label=label)
                else:
                    ui.write(" %s %-25s %d:%s\n" % (
                        prefix, bmark, repo.changelog.rev(n), hexfn(n)),
                        label=label)
        return

def _revstostrip(changelog, node):
    srev = changelog.rev(node)
    tostrip = [srev]
    saveheads = []
    for r in xrange(srev, len(changelog)):
        parents = changelog.parentrevs(r)
        if parents[0] in tostrip or parents[1] in tostrip:
            tostrip.append(r)
            if parents[1] != nullrev:
                for p in parents:
                    if p not in tostrip and p > srev:
                        saveheads.append(p)
    return [r for r in tostrip if r not in saveheads]

def strip(oldstrip, ui, repo, node, backup="all"):
    """Strip bookmarks if revisions are stripped using
    the mercurial.strip method. This usually happens during
    qpush and qpop"""
    revisions = _revstostrip(repo.changelog, node)
    marks = repo._bookmarks
    update = []
    for mark, n in marks.iteritems():
        if repo.changelog.rev(n) in revisions:
            update.append(mark)
    oldstrip(ui, repo, node, backup)
    if len(update) > 0:
        for m in update:
            marks[m] = repo.changectx('.').node()
        bookmarks.write(repo)

def reposetup(ui, repo):
    if not repo.local():
        return

    class bookmark_repo(repo.__class__):
        @util.propertycache
        def _bookmarks(self):
            return bookmarks.read(self)

        @util.propertycache
        def _bookmarkcurrent(self):
            return bookmarks.readcurrent(self)

        def rollback(self, dryrun=False):
            if os.path.exists(self.join('undo.bookmarks')):
                if not dryrun:
                    util.rename(self.join('undo.bookmarks'), self.join('bookmarks'))
                elif not os.path.exists(self.sjoin("undo")):
                    # avoid "no rollback information available" message
                    return 0
            return super(bookmark_repo, self).rollback(dryrun)

        def lookup(self, key):
            if key in self._bookmarks:
                key = self._bookmarks[key]
            return super(bookmark_repo, self).lookup(key)

        def commitctx(self, ctx, error=False):
            """Add a revision to the repository and
            move the bookmark"""
            wlock = self.wlock() # do both commit and bookmark with lock held
            try:
                node  = super(bookmark_repo, self).commitctx(ctx, error)
                if node is None:
                    return None
                parents = self.changelog.parents(node)
                if parents[1] == nullid:
                    parents = (parents[0],)

                bookmarks.update(self, parents, node)
                return node
            finally:
                wlock.release()

        def pull(self, remote, heads=None, force=False):
            result = super(bookmark_repo, self).pull(remote, heads, force)

            self.ui.debug("checking for updated bookmarks\n")
            rb = remote.listkeys('bookmarks')
            changed = False
            for k in rb.keys():
                if k in self._bookmarks:
                    nr, nl = rb[k], self._bookmarks[k]
                    if nr in self:
                        cr = self[nr]
                        cl = self[nl]
                        if cl.rev() >= cr.rev():
                            continue
                        if cr in cl.descendants():
                            self._bookmarks[k] = cr.node()
                            changed = True
                            self.ui.status(_("updating bookmark %s\n") % k)
                        else:
                            self.ui.warn(_("not updating divergent"
                                           " bookmark %s\n") % k)
            if changed:
                bookmarks.write(repo)

            return result

        def push(self, remote, force=False, revs=None, newbranch=False):
            result = super(bookmark_repo, self).push(remote, force, revs,
                                                     newbranch)

            self.ui.debug("checking for updated bookmarks\n")
            rb = remote.listkeys('bookmarks')
            for k in rb.keys():
                if k in self._bookmarks:
                    nr, nl = rb[k], hex(self._bookmarks[k])
                    if nr in self:
                        cr = self[nr]
                        cl = self[nl]
                        if cl in cr.descendants():
                            r = remote.pushkey('bookmarks', k, nr, nl)
                            if r:
                                self.ui.status(_("updating bookmark %s\n") % k)
                            else:
                                self.ui.warn(_('updating bookmark %s'
                                               ' failed!\n') % k)

            return result

        def addchangegroup(self, *args, **kwargs):
            result = super(bookmark_repo, self).addchangegroup(*args, **kwargs)
            if result > 1:
                # We have more heads than before
                return result
            node = self.changelog.tip()
            parents = self.dirstate.parents()
            bookmarks.update(self, parents, node)
            return result

        def _findtags(self):
            """Merge bookmarks with normal tags"""
            (tags, tagtypes) = super(bookmark_repo, self)._findtags()
            tags.update(self._bookmarks)
            return (tags, tagtypes)

        if hasattr(repo, 'invalidate'):
            def invalidate(self):
                super(bookmark_repo, self).invalidate()
                for attr in ('_bookmarks', '_bookmarkcurrent'):
                    if attr in self.__dict__:
                        delattr(self, attr)

    repo.__class__ = bookmark_repo

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
        bookmarks.write(repo)
        return True
    finally:
        w.release()

def pull(oldpull, ui, repo, source="default", **opts):
    # translate bookmark args to rev args for actual pull
    if opts.get('bookmark'):
        # this is an unpleasant hack as pull will do this internally
        source, branches = hg.parseurl(ui.expandpath(source),
                                       opts.get('branch'))
        other = hg.repository(hg.remoteui(repo, opts), source)
        rb = other.listkeys('bookmarks')

        for b in opts['bookmark']:
            if b not in rb:
                raise util.Abort(_('remote bookmark %s not found!') % b)
            opts.setdefault('rev', []).append(b)

    result = oldpull(ui, repo, source, **opts)

    # update specified bookmarks
    if opts.get('bookmark'):
        for b in opts['bookmark']:
            # explicit pull overrides local bookmark if any
            ui.status(_("importing bookmark %s\n") % b)
            repo._bookmarks[b] = repo[rb[b]].node()
        bookmarks.write(repo)

    return result

def push(oldpush, ui, repo, dest=None, **opts):
    dopush = True
    if opts.get('bookmark'):
        dopush = False
        for b in opts['bookmark']:
            if b in repo._bookmarks:
                dopush = True
                opts.setdefault('rev', []).append(b)

    result = 0
    if dopush:
        result = oldpush(ui, repo, dest, **opts)

    if opts.get('bookmark'):
        # this is an unpleasant hack as push will do this internally
        dest = ui.expandpath(dest or 'default-push', dest or 'default')
        dest, branches = hg.parseurl(dest, opts.get('branch'))
        other = hg.repository(hg.remoteui(repo, opts), dest)
        rb = other.listkeys('bookmarks')
        for b in opts['bookmark']:
            # explicit push overrides remote bookmark if any
            if b in repo._bookmarks:
                ui.status(_("exporting bookmark %s\n") % b)
                new = repo[b].hex()
            elif b in rb:
                ui.status(_("deleting remote bookmark %s\n") % b)
                new = '' # delete
            else:
                ui.warn(_('bookmark %s does not exist on the local '
                          'or remote repository!\n') % b)
                return 2
            old = rb.get(b, '')
            r = other.pushkey('bookmarks', b, old, new)
            if not r:
                ui.warn(_('updating bookmark %s failed!\n') % b)
                if not result:
                    result = 2

    return result

def diffbookmarks(ui, repo, remote):
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

def incoming(oldincoming, ui, repo, source="default", **opts):
    if opts.get('bookmarks'):
        source, branches = hg.parseurl(ui.expandpath(source), opts.get('branch'))
        other = hg.repository(hg.remoteui(repo, opts), source)
        ui.status(_('comparing with %s\n') % url.hidepassword(source))
        return diffbookmarks(ui, repo, other)
    else:
        return oldincoming(ui, repo, source, **opts)

def outgoing(oldoutgoing, ui, repo, dest=None, **opts):
    if opts.get('bookmarks'):
        dest = ui.expandpath(dest or 'default-push', dest or 'default')
        dest, branches = hg.parseurl(dest, opts.get('branch'))
        other = hg.repository(hg.remoteui(repo, opts), dest)
        ui.status(_('comparing with %s\n') % url.hidepassword(dest))
        return diffbookmarks(ui, other, repo)
    else:
        return oldoutgoing(ui, repo, dest, **opts)

def uisetup(ui):
    extensions.wrapfunction(repair, "strip", strip)
    if ui.configbool('bookmarks', 'track.current'):
        extensions.wrapcommand(commands.table, 'update', updatecurbookmark)

    entry = extensions.wrapcommand(commands.table, 'pull', pull)
    entry[1].append(('B', 'bookmark', [],
                     _("bookmark to import"),
                     _('BOOKMARK')))
    entry = extensions.wrapcommand(commands.table, 'push', push)
    entry[1].append(('B', 'bookmark', [],
                     _("bookmark to export"),
                     _('BOOKMARK')))
    entry = extensions.wrapcommand(commands.table, 'incoming', incoming)
    entry[1].append(('B', 'bookmarks', False,
                     _("compare bookmark")))
    entry = extensions.wrapcommand(commands.table, 'outgoing', outgoing)
    entry[1].append(('B', 'bookmarks', False,
                     _("compare bookmark")))

    pushkey.register('bookmarks', pushbookmark, listbookmarks)

def updatecurbookmark(orig, ui, repo, *args, **opts):
    '''Set the current bookmark

    If the user updates to a bookmark we update the .hg/bookmarks.current
    file.
    '''
    res = orig(ui, repo, *args, **opts)
    rev = opts['rev']
    if not rev and len(args) > 0:
        rev = args[0]
    bookmarks.setcurrent(repo, rev)
    return res

def bmrevset(repo, subset, x):
    """``bookmark([name])``
    The named bookmark or all bookmarks.
    """
    # i18n: "bookmark" is a keyword
    args = revset.getargs(x, 0, 1, _('bookmark takes one or no arguments'))
    if args:
        bm = revset.getstring(args[0],
                              # i18n: "bookmark" is a keyword
                              _('the argument to bookmark must be a string'))
        bmrev = listbookmarks(repo).get(bm, None)
        if bmrev:
            bmrev = repo.changelog.rev(bin(bmrev))
        return [r for r in subset if r == bmrev]
    bms = set([repo.changelog.rev(bin(r)) for r in listbookmarks(repo).values()])
    return [r for r in subset if r in bms]

def extsetup(ui):
    revset.symbols['bookmark'] = bmrevset

cmdtable = {
    "bookmarks":
        (bookmark,
         [('f', 'force', False, _('force')),
          ('r', 'rev', '', _('revision'), _('REV')),
          ('d', 'delete', False, _('delete a given bookmark')),
          ('m', 'rename', '', _('rename a given bookmark'), _('NAME'))],
         _('hg bookmarks [-f] [-d] [-m NAME] [-r REV] [NAME]')),
}

colortable = {'bookmarks.current': 'green'}

# tell hggettext to extract docstrings from these functions:
i18nfunctions = [bmrevset]
