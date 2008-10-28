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
#  hgext.purge =
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

from mercurial import util, commands, cmdutil
from mercurial.i18n import _
import os

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
    eol = '\n'
    if opts['print0']:
        eol = '\0'
        act = False # --print0 implies --print

    def remove(remove_func, name):
        if act:
            try:
                remove_func(os.path.join(repo.root, name))
            except OSError:
                m = _('%s cannot be removed') % name
                if opts['abort_on_err']:
                    raise util.Abort(m)
                ui.warn(_('warning: %s\n') % m)
        else:
            ui.write('%s%s' % (name, eol))

    directories = []
    match = cmdutil.match(repo, dirs, opts)
    match.dir = directories.append
    status = repo.status(match=match, ignored=opts['all'], unknown=True)

    for f in util.sort(status[4] + status[5]):
        ui.note(_('Removing file %s\n') % f)
        remove(os.remove, f)

    for f in util.sort(directories)[::-1]:
        if match(f) and not os.listdir(repo.wjoin(f)):
            ui.note(_('Removing directory %s\n') % f)
            remove(os.rmdir, f)

cmdtable = {
    'purge|clean':
        (purge,
         [('a', 'abort-on-err', None, _('abort if an error occurs')),
          ('',  'all', None, _('purge ignored files too')),
          ('p', 'print', None, _('print the file names instead of deleting them')),
          ('0', 'print0', None, _('end filenames with NUL, for use with xargs'
                                  ' (implies -p)')),
         ] + commands.walkopts,
         _('hg purge [OPTION]... [DIR]...'))
}
