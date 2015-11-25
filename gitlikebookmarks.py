# gitlikebookmarks.py - add git like behavior for bookmarks
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""add git like behavior for bookmarks: only move the active bookmark

Add a -x specified to several commands. When the -x flag is specied, the
commands only allow the active bookmark to move.
"""

from mercurial import extensions
from mercurial import bookmarks
from mercurial import util
from mercurial.i18n import _

gitlikebookmarkscommands = (
        # module name, command name
        ('rebase', 'rebase'),
        ('histedit', 'histedit'),
)
def _bookmarkwrite(orig, bkmstoreinst, *args, **kwargs):
    repo = bkmstoreinst._repo
    activebook = None
    bookmarksbefore = None

    if util.safehasattr(bkmstoreinst, "oldactivebookmark"):
        activebook = bkmstoreinst.oldactivebookmark
        bookmarksbefore = bkmstoreinst.oldbookmarks

    if activebook != None:
        override = []
        for book in bkmstoreinst:
            if book == activebook:
                continue
            override.append(book)
        for k in override:
            if k in bookmarksbefore:
                bkmstoreinst[k] = bookmarksbefore[k]
        if repo._activebookmark != activebook:
            bookmarks.activate(repo, activebook)

        # XXX will be an issue with chg, -x will persist accross commands
        # Possible fixes:
        # 1) setting activebook to None here would only work
        # for commands with a single transaction (we have no guarantee of that)
        # 2) put all this code in _wrapfn and wrap the call to orig(...) in a
        # transaction but it will rollback if rebase/histedit has conflicts (not
        # intended behavior as we want to leave the conflicts in the workdir)
    return orig(bkmstoreinst, *args, **kwargs)

def _wrapfn(orig, ui, repo, *args, **kwargs):
    # Normal behavior unless -x is specified
    if not kwargs.get('stickybookmark'):
        return orig(ui, repo, *args, **kwargs)
    repo._bookmarks.oldactivebookmark = bookmarks.readactive(repo)
    repo._bookmarks.oldbookmarks = repo._bookmarks.copy()
    return orig(ui, repo, *args, **kwargs)

def extsetup(ui):
    extensions.wrapfunction(bookmarks.bmstore, '_write', _bookmarkwrite)
    for module, fn in gitlikebookmarkscommands:
        try:
            ext = extensions.find(module)
            if ext:
                entry = extensions.wrapcommand(ext.cmdtable, fn, _wrapfn)
                entry[1].append(('x','stickybookmark', None,
                                 _('only allow the active bookmark to move'
                                ' during the operation')))
        except KeyError:
            # Extension not present
            pass
