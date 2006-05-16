#!/usr/bin/env python
#
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
import os

class Purge(object):
    def __init__(self, act=True, abort_on_err=False):
        self._repo = None
        self._ui = None
        self._hg_root = None
        self._act = act
        self._abort_on_err = abort_on_err

    def purge(self, ui, repo, paths=None):
        self._repo = repo
        self._ui = ui
        self._hg_root = self._split_path(repo.root)

        if not paths:
            paths = [repo.root]

        for path in paths:
            path = os.path.abspath(path)
            for root, dirs, files in os.walk(path, topdown=False):
                if '.hg' in self._split_path(root):
                    # Skip files in the .hg directory.
                    # Note that if the repository is in a directory
                    # called .hg this command does not work.
                    continue
                for name in files:
                    self._remove_file(os.path.join(root, name))
                if not os.listdir(root):
                    # Remove this directory if it is empty.
                    self._remove_dir(root)

        self._repo = None
        self._ui = None
        self._hg_root = None

    def _error(self, msg):
        if self._abort_on_err:
            raise util.Abort(msg)
        else:
            ui.warn('warning: ' + msg + '\n')

    def _remove_file(self, name):
        relative_name = self._relative_name(name)
        # dirstate.state() requires a path relative to the root
        # directory.
        if self._repo.dirstate.state(relative_name) != '?':
            return
        self._ui.note(name + '\n')
        if self._act:
            try:
                os.remove(name)
            except OSError, e:
                error('"%s" cannot be removed' % name)

    def _remove_dir(self, name):
        self._ui.note(name + '\n')
        if self._act:
            try:
                os.rmdir(name)
            except OSError, e:
                error('"%s" cannot be removed' % name)

    def _relative_name(self, name):
        splitted_path = self._split_path(name)[len(self._hg_root):]
        return self._join_path(splitted_path)

    def _split_path(self, path):
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
        ret = ''
        for part in splitted_path:
            if ret:
                ret = os.path.join(ret, part)
            else:
                ret = part
        return ret


def purge(ui, repo, *paths, **opts):
    '''removes files not tracked by mercurial

    Delete files not known to mercurial, this is useful to test local and
    uncommitted changes in the otherwise clean source tree.

    This means that purge will delete:
     - Unknown files: files marked with "?" by "hg status"
     - Ignored files: files usually ignored by Mercurial because they match a
       pattern in a ".hgignore" file
     - Empty directories: infact Mercurial ignores directories unless they
       contain files under source control managment
    But it will leave untouched:
     - Unmodified tracked files
     - Modified tracked files
     - New files added to the repository (with "hg add")

    If names are given, only files matching the names are considered, else
    all files in the repository directory are considered.

    Be careful with purge, you could irreversibly delete some files you
    forgot to add to the repository. If you only want to print the list of
    files that this program would delete use the -vn options.
    '''
    act = bool(opts['nothing'])
    abort_on_err = bool(opts['abort_on_err'])
    p = Purge(act, abort_on_err)
    p.purge(ui, repo, paths)


cmdtable = {
    'purge':    (purge,
                 [('a', 'abort-on-err', None, 'abort if an error occurs'),
                  ('n', 'nothing',      None, 'do nothing on files, useful with --verbose'),
                 ],
                 'hg purge [OPTIONS] [NAME]')
}
