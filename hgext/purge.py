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

from mercurial import util, commands
from mercurial.i18n import _
import os

def dopurge(ui, repo, dirs=None, act=True, ignored=False,
            abort_on_err=False, eol='\n',
            force=False, include=None, exclude=None):
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

    if not force:
        _check_fs(ui, repo)

    directories = []
    files = []
    missing = []
    roots, match, anypats = util.cmdmatcher(repo.root, repo.getcwd(), dirs,
                                            include, exclude)
    for src, f, st in repo.dirstate.statwalk(files=roots, match=match,
                                             ignored=ignored, directories=True):
        if src == 'd':
            directories.append(f)
        elif src == 'm':
            missing.append(f)
        elif src == 'f' and f not in repo.dirstate:
            files.append(f)

    directories.sort()

    for f in files:
        if f not in repo.dirstate:
            ui.note(_('Removing file %s\n') % f)
            remove(os.remove, f)

    for f in directories[::-1]:
        if match(f) and not os.listdir(repo.wjoin(f)):
            ui.note(_('Removing directory %s\n') % f)
            remove(os.rmdir, f)

def _check_fs(ui, repo):
    """Abort if there is the chance of having problems with name-mangling fs

    In a name mangling filesystem (e.g. a case insensitive one)
    dirstate.walk() can yield filenames different from the ones
    stored in the dirstate. This already confuses the status and
    add commands, but with purge this may cause data loss.

    To prevent this, this function will abort if there are uncommitted
    changes.
    """

    # We can't use (files, match) to do a partial walk here - we wouldn't
    # notice a modified README file if the user ran "hg purge readme"
    modified, added, removed, deleted = repo.status()[:4]
    if modified or added or removed or deleted:
        if not util.checkfolding(repo.path) and not ui.quiet:
            ui.warn(_("Purging on name mangling filesystems is not "
                      "fully supported.\n"))
        raise util.Abort(_("outstanding uncommitted changes"))


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
    ignored = bool(opts['all'])
    abort_on_err = bool(opts['abort_on_err'])
    eol = opts['print0'] and '\0' or '\n'
    if eol == '\0':
        # --print0 implies --print
        act = False
    force = bool(opts['force'])
    include = opts['include']
    exclude = opts['exclude']
    dopurge(ui, repo, dirs, act, ignored, abort_on_err,
            eol, force, include, exclude)


cmdtable = {
    'purge|clean':
        (purge,
         [('a', 'abort-on-err', None, _('abort if an error occurs')),
          ('',  'all', None, _('purge ignored files too')),
          ('f', 'force', None, _('purge even when there are uncommitted changes')),
          ('p', 'print', None, _('print the file names instead of deleting them')),
          ('0', 'print0', None, _('end filenames with NUL, for use with xargs'
                                  ' (implies -p)')),
         ] + commands.walkopts,
         _('hg purge [OPTION]... [DIR]...'))
}
