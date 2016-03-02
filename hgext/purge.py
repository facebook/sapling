# Copyright (C) 2006 - Marco Barisione <marco@barisione.org>
#
# This is a small extension for Mercurial (https://mercurial-scm.org/)
# that removes files not known to mercurial
#
# This program was inspired by the "cvspurge" script contained in CVS
# utilities (http://www.red-bean.com/cvsutils/).
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
# along with this program; if not, see <http://www.gnu.org/licenses/>.

'''command to delete untracked files from the working directory'''
from __future__ import absolute_import

import os

from mercurial import (
    cmdutil,
    commands,
    error,
    scmutil,
    util,
)
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

@command('purge|clean',
    [('a', 'abort-on-err', None, _('abort if an error occurs')),
    ('',  'all', None, _('purge ignored files too')),
    ('',  'dirs', None, _('purge empty directories')),
    ('',  'files', None, _('purge files')),
    ('p', 'print', None, _('print filenames instead of deleting them')),
    ('0', 'print0', None, _('end filenames with NUL, for use with xargs'
                            ' (implies -p/--print)')),
    ] + commands.walkopts,
    _('hg purge [OPTION]... [DIR]...'))
def purge(ui, repo, *dirs, **opts):
    '''removes files not tracked by Mercurial

    Delete files not known to Mercurial. This is useful to test local
    and uncommitted changes in an otherwise-clean source tree.

    This means that purge will delete the following by default:

    - Unknown files: files marked with "?" by :hg:`status`
    - Empty directories: in fact Mercurial ignores directories unless
      they contain files under source control management

    But it will leave untouched:

    - Modified and unmodified tracked files
    - Ignored files (unless --all is specified)
    - New files added to the repository (with :hg:`add`)

    The --files and --dirs options can be used to direct purge to delete
    only files, only directories, or both. If neither option is given,
    both will be deleted.

    If directories are given on the command line, only files in these
    directories are considered.

    Be careful with purge, as you could irreversibly delete some files
    you forgot to add to the repository. If you only want to print the
    list of files that this program would delete, use the --print
    option.
    '''
    act = not opts['print']
    eol = '\n'
    if opts['print0']:
        eol = '\0'
        act = False # --print0 implies --print
    removefiles = opts['files']
    removedirs = opts['dirs']
    if not removefiles and not removedirs:
        removefiles = True
        removedirs = True

    def remove(remove_func, name):
        if act:
            try:
                remove_func(repo.wjoin(name))
            except OSError:
                m = _('%s cannot be removed') % name
                if opts['abort_on_err']:
                    raise error.Abort(m)
                ui.warn(_('warning: %s\n') % m)
        else:
            ui.write('%s%s' % (name, eol))

    match = scmutil.match(repo[None], dirs, opts)
    if removedirs:
        directories = []
        match.explicitdir = match.traversedir = directories.append
    status = repo.status(match=match, ignored=opts['all'], unknown=True)

    if removefiles:
        for f in sorted(status.unknown + status.ignored):
            if act:
                ui.note(_('removing file %s\n') % f)
            remove(util.unlink, f)

    if removedirs:
        for f in sorted(directories, reverse=True):
            if match(f) and not os.listdir(repo.wjoin(f)):
                if act:
                    ui.note(_('removing directory %s\n') % f)
                remove(os.rmdir, f)
