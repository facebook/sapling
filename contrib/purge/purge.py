# Copyright (C) 2006 - Marco Barisione <marco@barisione.org>
#
# This is a small extension for Mercurial (http://www.selenic.com/mercurial)
# that removes files not known to mercurial
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

class Purge(object):
    def __init__(self, act=True, abort_on_err=False, eol='\n'):
        self._repo = None
        self._ui = None
        self._hg_root = None
        self._act = act
        self._abort_on_err = abort_on_err
        self._eol = eol

    def purge(self, ui, repo, dirs=None):
        self._repo = repo
        self._ui = ui
        self._hg_root = self._split_path(repo.root)
        
        directories = []
        files = []
        for src, f, st in repo.dirstate.statwalk(files=dirs, ignored=True,
                                                 directories=True):
            if   src == 'd':
                directories.append(f)
            elif src == 'f' and f not in repo.dirstate:
                files.append(f)

        directories.sort()

        for f in files:
            self._remove_file(os.path.join(repo.root, f))

        for f in directories[::-1]:
            f = os.path.join(repo.root, f)
            if not os.listdir(f):
                self._remove_dir(f)

        self._repo = None
        self._ui = None
        self._hg_root = None

    def _error(self, msg):
        if self._abort_on_err:
            raise util.Abort(msg)
        else:
            self._ui.warn(_('warning: %s\n') % msg)

    def _remove_file(self, name):
        relative_name = self._relative_name(name)
        # dirstate.state() requires a path relative to the root
        # directory.
        if self._repo.dirstate.state(relative_name) != '?':
            return
        self._ui.note(_('Removing file %s\n') % relative_name)
        if self._act:
            try:
                os.remove(name)
            except OSError, e:
                self._error(_('%s cannot be removed') % relative_name)
        else:
            self._ui.write('%s%s' % (relative_name, self._eol))

    def _remove_dir(self, name):
        relative_name = self._relative_name(name)
        self._ui.note(_('Removing directory %s\n') % relative_name)
        if self._act:
            try:
                os.rmdir(name)
            except OSError, e:
                self._error(_('%s cannot be removed') % relative_name)
        else:
            self._ui.write('%s%s' % (relative_name, self._eol))

    def _relative_name(self, path):
        '''
        Returns "path" but relative to the root directory of the
        repository and with '\\' replaced with '/'.
        This is needed because this is the format required by
        self._repo.dirstate.state().
        '''
        splitted_path = self._split_path(path)[len(self._hg_root):]
        # Even on Windows self._repo.dirstate.state() wants '/'.
        return self._join_path(splitted_path).replace('\\', '/')

    def _split_path(self, path):
        '''
        Returns a list of the single files/directories in "path".
        For instance:
          '/home/user/test' -> ['/', 'home', 'user', 'test']
          'C:\\Mercurial'   -> ['C:\\', 'Mercurial']
        '''
        ret = []
        while True:
            head, tail = os.path.split(path)
            if tail:
                ret.append(tail)
            if head == path:
                ret.append(head)
                break
            path = head
        ret.reverse()
        return ret

    def _join_path(self, splitted_path):
        '''
        Joins a list returned by _split_path().
        '''
        ret = ''
        for part in splitted_path:
            if ret:
                ret = os.path.join(ret, part)
            else:
                ret = part
        return ret


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
    p = Purge(act, abort_on_err, eol)
    p.purge(ui, repo, dirs)


cmdtable = {
    'purge':
        (purge,
         [('a', 'abort-on-err', None, _('abort if an error occurs')),
          ('p', 'print', None, _('print the file names instead of deleting them')),
          ('0', 'print0', None, _('end filenames with NUL, for use with xargs'
                                  ' (implies -p)'))],
         _('hg purge [OPTION]... [DIR]...'))
}
