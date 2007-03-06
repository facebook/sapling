# Copyright (C) 2006 - Marco Barisione <marco@barisione.org>
#
# This is a small extension for Mercurial (http://www.selenic.com/mercurial)
# that removes files not known to mercurial
#
# This program was inspired by the "cvspurge" script contained in CVS utilities
# (http://www.red-bean.com/cvsutils/).
#
# To enable the "purge" extension put these lines in your ~/.hgrc:
#  [extensions]
#  purge = /path/to/purge.py
#
# For help on the usage of "hg purge" use:
#  hg help purge
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA 02111-1307, USA.

from mercurial import hg, util
from mercurial.i18n import _
import os

def dopurge(ui, repo, dirs=None, act=True, abort_on_err=False, eol='\n'):
    def error(msg):
        if abort_on_err:
            raise util.Abort(msg)
        else:
            ui.warn(_('warning: %s\n') % msg)

    def remove(remove_func, name):
        if act:
            try:
                remove_func(os.path.join(repo.root, name))
            except OSError, e:
                error(_('%s cannot be removed') % name)
        else:
            ui.write('%s%s' % (name, eol))

    directories = []
    files = []
    roots, match, anypats = util.cmdmatcher(repo.root, repo.getcwd(), dirs)
    for src, f, st in repo.dirstate.statwalk(files=roots, match=match,
                                             ignored=True, directories=True):
        if src == 'd':
            directories.append(f)
        elif src == 'f' and f not in repo.dirstate:
            files.append(f)

    directories.sort()

    for f in files:
        if f not in repo.dirstate:
            ui.note(_('Removing file %s\n') % f)
            remove(os.remove, f)

    for f in directories[::-1]:
        if not os.listdir(repo.wjoin(f)):
            ui.note(_('Removing directory %s\n') % f)
            remove(os.rmdir, f)


def purge(ui, repo, *dirs, **opts):
    '''removes files not tracked by mercurial

    Delete files not known to mercurial, this is useful to test local and
    uncommitted changes in the otherwise clean source tree.

    This means that purge will delete:
     - Unknown files: files marked with "?" by "hg status"
     - Ignored files: files usually ignored by Mercurial because they match
       a pattern in a ".hgignore" file
     - Empty directories: in fact Mercurial ignores directories unless they
       contain files under source control managment
    But it will leave untouched:
     - Unmodified tracked files
     - Modified tracked files
     - New files added to the repository (with "hg add")

    If directories are given on the command line, only files in these
    directories are considered.

    Be careful with purge, you could irreversibly delete some files you
    forgot to add to the repository. If you only want to print the list of
    files that this program would delete use the --print option.
    '''
    act = not opts['print']
    abort_on_err = bool(opts['abort_on_err'])
    eol = opts['print0'] and '\0' or '\n'
    if eol == '\0':
        # --print0 implies --print
        act = False
    dopurge(ui, repo, dirs, act, abort_on_err, eol)


cmdtable = {
    'purge':
        (purge,
         [('a', 'abort-on-err', None, _('abort if an error occurs')),
          ('p', 'print', None, _('print the file names instead of deleting them')),
          ('0', 'print0', None, _('end filenames with NUL, for use with xargs'
                                  ' (implies -p)'))],
         _('hg purge [OPTION]... [DIR]...'))
}
