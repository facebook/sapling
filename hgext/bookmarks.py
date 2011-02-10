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
from mercurial import encoding
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

def uisetup(ui):
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

cmdtable = {
    "bookmarks":
        (bookmark,
         [('f', 'force', False, _('force')),
          ('r', 'rev', '', _('revision'), _('REV')),
          ('d', 'delete', False, _('delete a given bookmark')),
          ('m', 'rename', '', _('rename a given bookmark'), _('NAME'))],
         _('hg bookmarks [-f] [-d] [-m NAME] [-r REV] [NAME]')),
}
