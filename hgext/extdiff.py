# extdiff.py - external diff program support for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# The `extdiff' Mercurial extension allows you to use external programs
# to compare revisions, or revision with working dir.  The external diff
# programs are called with a configurable set of options and two
# non-option arguments: paths to directories containing snapshots of
# files to compare.
#
# To enable this extension:
#
#   [extensions]
#   hgext.extdiff =
#
# The `extdiff' extension also allows to configure new diff commands, so
# you do not need to type "hg extdiff -p kdiff3" always.
#
#   [extdiff]
#   # add new command that runs GNU diff(1) in 'context diff' mode
#   cmd.cdiff = gdiff
#   opts.cdiff = -Nprc5

#   # add new command called vdiff, runs kdiff3
#   cmd.vdiff = kdiff3

#   # add new command called meld, runs meld (no need to name twice)
#   cmd.meld =

#   # add new command called vimdiff, runs gvimdiff with DirDiff plugin
#   #(see http://www.vim.org/scripts/script.php?script_id=102)
#   # Non english user, be sure to put "let g:DirDiffDynamicDiffText = 1" in
#   # your .vimrc
#   cmd.vimdiff = gvim
#   opts.vimdiff = -f '+next' '+execute "DirDiff" argv(0) argv(1)'
#
# Each custom diff commands can have two parts: a `cmd' and an `opts'
# part.  The cmd.xxx option defines the name of an executable program
# that will be run, and opts.xxx defines a set of command-line options
# which will be inserted to the command between the program name and
# the files/directories to diff (i.e. the cdiff example above).
#
# You can use -I/-X and list of file or directory names like normal
# "hg diff" command.  The `extdiff' extension makes snapshots of only
# needed files, so running the external diff program will actually be
# pretty fast (at least faster than having to compare the entire tree).

from mercurial.i18n import _
from mercurial.node import *
from mercurial import cmdutil, util
import os, shutil, tempfile

def dodiff(ui, repo, diffcmd, diffopts, pats, opts):
    def snapshot_node(files, node):
        '''snapshot files as of some revision'''
        mf = repo.changectx(node).manifest()
        dirname = os.path.basename(repo.root)
        if dirname == "":
            dirname = "root"
        dirname = '%s.%s' % (dirname, short(node))
        base = os.path.join(tmproot, dirname)
        os.mkdir(base)
        if not ui.quiet:
            ui.write_err(_('making snapshot of %d files from rev %s\n') %
                         (len(files), short(node)))
        for fn in files:
            if not fn in mf:
                # skipping new file after a merge ?
                continue
            wfn = util.pconvert(fn)
            ui.note('  %s\n' % wfn)
            dest = os.path.join(base, wfn)
            destdir = os.path.dirname(dest)
            if not os.path.isdir(destdir):
                os.makedirs(destdir)
            data = repo.wwritedata(wfn, repo.file(wfn).read(mf[wfn]))
            open(dest, 'wb').write(data)
        return dirname

    def snapshot_wdir(files):
        '''snapshot files from working directory.
        if not using snapshot, -I/-X does not work and recursive diff
        in tools like kdiff3 and meld displays too many files.'''
        dirname = os.path.basename(repo.root)
        if dirname == "":
            dirname = "root"
        base = os.path.join(tmproot, dirname)
        os.mkdir(base)
        if not ui.quiet:
            ui.write_err(_('making snapshot of %d files from working dir\n') %
                         (len(files)))
        for fn in files:
            wfn = util.pconvert(fn)
            ui.note('  %s\n' % wfn)
            dest = os.path.join(base, wfn)
            destdir = os.path.dirname(dest)
            if not os.path.isdir(destdir):
                os.makedirs(destdir)
            fp = open(dest, 'wb')
            for chunk in util.filechunkiter(repo.wopener(wfn)):
                fp.write(chunk)
        return dirname

    node1, node2 = cmdutil.revpair(repo, opts['rev'])
    files, matchfn, anypats = cmdutil.matchpats(repo, pats, opts)
    modified, added, removed, deleted, unknown = repo.status(
        node1, node2, files, match=matchfn)[:5]
    if not (modified or added or removed):
        return 0

    tmproot = tempfile.mkdtemp(prefix='extdiff.')
    try:
        dir1 = snapshot_node(modified + removed, node1)
        if node2:
            dir2 = snapshot_node(modified + added, node2)
        else:
            dir2 = snapshot_wdir(modified + added)
        cmdline = ('%s %s %s %s' %
                   (util.shellquote(diffcmd), ' '.join(diffopts),
                    util.shellquote(dir1), util.shellquote(dir2)))
        ui.debug('running %r in %s\n' % (cmdline, tmproot))
        util.system(cmdline, cwd=tmproot)
        return 1
    finally:
        ui.note(_('cleaning up temp directory\n'))
        shutil.rmtree(tmproot)

def extdiff(ui, repo, *pats, **opts):
    '''use external program to diff repository (or selected files)

    Show differences between revisions for the specified files, using
    an external program.  The default program used is diff, with
    default options "-Npru".

    To select a different program, use the -p option.  The program
    will be passed the names of two directories to compare.  To pass
    additional options to the program, use the -o option.  These will
    be passed before the names of the directories to compare.

    When two revision arguments are given, then changes are
    shown between those revisions. If only one revision is
    specified then that revision is compared to the working
    directory, and, when no revisions are specified, the
    working directory files are compared to its parent.'''
    program = opts['program'] or 'diff'
    if opts['program']:
        option = opts['option']
    else:
        option = opts['option'] or ['-Npru']
    return dodiff(ui, repo, program, option, pats, opts)

cmdtable = {
    "extdiff":
    (extdiff,
     [('p', 'program', '', _('comparison program to run')),
      ('o', 'option', [], _('pass option to comparison program')),
      ('r', 'rev', [], _('revision')),
      ('I', 'include', [], _('include names matching the given patterns')),
      ('X', 'exclude', [], _('exclude names matching the given patterns'))],
     _('hg extdiff [OPT]... [FILE]...')),
    }

def uisetup(ui):
    for cmd, path in ui.configitems('extdiff'):
        if not cmd.startswith('cmd.'): continue
        cmd = cmd[4:]
        if not path: path = cmd
        diffopts = ui.config('extdiff', 'opts.' + cmd, '')
        diffopts = diffopts and [diffopts] or []
        def save(cmd, path, diffopts):
            '''use closure to save diff command to use'''
            def mydiff(ui, repo, *pats, **opts):
                return dodiff(ui, repo, path, diffopts, pats, opts)
            mydiff.__doc__ = '''use %(path)r to diff repository (or selected files)

            Show differences between revisions for the specified
            files, using the %(path)r program.

            When two revision arguments are given, then changes are
            shown between those revisions. If only one revision is
            specified then that revision is compared to the working
            directory, and, when no revisions are specified, the
            working directory files are compared to its parent.''' % {
                'path': path,
                }
            return mydiff
        cmdtable[cmd] = (save(cmd, path, diffopts),
                         cmdtable['extdiff'][1][1:],
                         _('hg %s [OPTION]... [FILE]...') % cmd)
