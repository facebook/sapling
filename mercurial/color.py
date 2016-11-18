# utility for color output for Mercurial commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

_styles = {'grep.match': 'red bold',
           'grep.linenumber': 'green',
           'grep.rev': 'green',
           'grep.change': 'green',
           'grep.sep': 'cyan',
           'grep.filename': 'magenta',
           'grep.user': 'magenta',
           'grep.date': 'magenta',
           'bookmarks.active': 'green',
           'branches.active': 'none',
           'branches.closed': 'black bold',
           'branches.current': 'green',
           'branches.inactive': 'none',
           'diff.changed': 'white',
           'diff.deleted': 'red',
           'diff.diffline': 'bold',
           'diff.extended': 'cyan bold',
           'diff.file_a': 'red bold',
           'diff.file_b': 'green bold',
           'diff.hunk': 'magenta',
           'diff.inserted': 'green',
           'diff.tab': '',
           'diff.trailingwhitespace': 'bold red_background',
           'changeset.public' : '',
           'changeset.draft' : '',
           'changeset.secret' : '',
           'diffstat.deleted': 'red',
           'diffstat.inserted': 'green',
           'histedit.remaining': 'red bold',
           'ui.prompt': 'yellow',
           'log.changeset': 'yellow',
           'patchbomb.finalsummary': '',
           'patchbomb.from': 'magenta',
           'patchbomb.to': 'cyan',
           'patchbomb.subject': 'green',
           'patchbomb.diffstats': '',
           'rebase.rebased': 'blue',
           'rebase.remaining': 'red bold',
           'resolve.resolved': 'green bold',
           'resolve.unresolved': 'red bold',
           'shelve.age': 'cyan',
           'shelve.newest': 'green bold',
           'shelve.name': 'blue bold',
           'status.added': 'green bold',
           'status.clean': 'none',
           'status.copied': 'none',
           'status.deleted': 'cyan bold underline',
           'status.ignored': 'black bold',
           'status.modified': 'blue bold',
           'status.removed': 'red bold',
           'status.unknown': 'magenta bold underline',
           'tags.normal': 'green',
           'tags.local': 'black bold'}

def loadcolortable(ui, extname, colortable):
    _styles.update(colortable)
