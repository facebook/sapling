# extdiff.py - external diff program support for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

'''command to allow external programs to compare revisions

The extdiff Mercurial extension allows you to use external programs
to compare revisions, or revision with working directory. The external
diff programs are called with a configurable set of options and two
non-option arguments: paths to directories containing snapshots of
files to compare.

The extdiff extension also allows to configure new diff commands, so
you do not need to type "hg extdiff -p kdiff3" always. ::

  [extdiff]
  # add new command that runs GNU diff(1) in 'context diff' mode
  cdiff = gdiff -Nprc5
  ## or the old way:
  #cmd.cdiff = gdiff
  #opts.cdiff = -Nprc5

  # add new command called vdiff, runs kdiff3
  vdiff = kdiff3

  # add new command called meld, runs meld (no need to name twice)
  meld =

  # add new command called vimdiff, runs gvimdiff with DirDiff plugin
  # (see http://www.vim.org/scripts/script.php?script_id=102) Non
  # English user, be sure to put "let g:DirDiffDynamicDiffText = 1" in
  # your .vimrc
  vimdiff = gvim -f '+next' '+execute "DirDiff" argv(0) argv(1)'

You can use -I/-X and list of file or directory names like normal "hg
diff" command. The extdiff extension makes snapshots of only needed
files, so running the external diff program will actually be pretty
fast (at least faster than having to compare the entire tree).
'''

from mercurial.i18n import _
from mercurial.node import short
from mercurial import cmdutil, util, commands
import os, shlex, shutil, tempfile

def snapshot(ui, repo, files, node, tmproot):
    '''snapshot files as of some revision
    if not using snapshot, -I/-X does not work and recursive diff
    in tools like kdiff3 and meld displays too many files.'''
    dirname = os.path.basename(repo.root)
    if dirname == "":
        dirname = "root"
    if node is not None:
        dirname = '%s.%s' % (dirname, short(node))
    base = os.path.join(tmproot, dirname)
    os.mkdir(base)
    if node is not None:
        ui.note(_('making snapshot of %d files from rev %s\n') %
                (len(files), short(node)))
    else:
        ui.note(_('making snapshot of %d files from working directory\n') %
            (len(files)))
    wopener = util.opener(base)
    fns_and_mtime = []
    ctx = repo[node]
    for fn in files:
        wfn = util.pconvert(fn)
        if not wfn in ctx:
            # skipping new file after a merge ?
            continue
        ui.note('  %s\n' % wfn)
        dest = os.path.join(base, wfn)
        fctx = ctx[wfn]
        data = repo.wwritedata(wfn, fctx.data())
        if 'l' in fctx.flags():
            wopener.symlink(data, wfn)
        else:
            wopener(wfn, 'w').write(data)
            if 'x' in fctx.flags():
                util.set_flags(dest, False, True)
        if node is None:
            fns_and_mtime.append((dest, repo.wjoin(fn), os.path.getmtime(dest)))
    return dirname, fns_and_mtime

def dodiff(ui, repo, diffcmd, diffopts, pats, opts):
    '''Do the actuall diff:

    - copy to a temp structure if diffing 2 internal revisions
    - copy to a temp structure if diffing working revision with
      another one and more than 1 file is changed
    - just invoke the diff for a single file in the working dir
    '''

    revs = opts.get('rev')
    change = opts.get('change')

    if revs and change:
        msg = _('cannot specify --rev and --change at the same time')
        raise util.Abort(msg)
    elif change:
        node2 = repo.lookup(change)
        node1 = repo[node2].parents()[0].node()
    else:
        node1, node2 = cmdutil.revpair(repo, revs)

    matcher = cmdutil.match(repo, pats, opts)
    modified, added, removed = repo.status(node1, node2, matcher)[:3]
    if not (modified or added or removed):
        return 0

    tmproot = tempfile.mkdtemp(prefix='extdiff.')
    dir2root = ''
    try:
        # Always make a copy of node1
        dir1 = snapshot(ui, repo, modified + removed, node1, tmproot)[0]
        changes = len(modified) + len(removed) + len(added)

        # If node2 in not the wc or there is >1 change, copy it
        if node2 or changes > 1:
            dir2, fns_and_mtime = snapshot(ui, repo, modified + added, node2, tmproot)
        else:
            # This lets the diff tool open the changed file directly
            dir2 = ''
            dir2root = repo.root
            fns_and_mtime = []

        # If only one change, diff the files instead of the directories
        if changes == 1 :
            if len(modified):
                dir1 = os.path.join(dir1, util.localpath(modified[0]))
                dir2 = os.path.join(dir2root, dir2, util.localpath(modified[0]))
            elif len(removed) :
                dir1 = os.path.join(dir1, util.localpath(removed[0]))
                dir2 = os.devnull
            else:
                dir1 = os.devnull
                dir2 = os.path.join(dir2root, dir2, util.localpath(added[0]))

        cmdline = ('%s %s %s %s' %
                   (util.shellquote(diffcmd), ' '.join(diffopts),
                    util.shellquote(dir1), util.shellquote(dir2)))
        ui.debug(_('running %r in %s\n') % (cmdline, tmproot))
        util.system(cmdline, cwd=tmproot)

        for copy_fn, working_fn, mtime in fns_and_mtime:
            if os.path.getmtime(copy_fn) != mtime:
                ui.debug(_('file changed while diffing. '
                         'Overwriting: %s (src: %s)\n') % (working_fn, copy_fn))
                util.copyfile(copy_fn, working_fn)

        return 1
    finally:
        ui.note(_('cleaning up temp directory\n'))
        shutil.rmtree(tmproot)

def extdiff(ui, repo, *pats, **opts):
    '''use external program to diff repository (or selected files)

    Show differences between revisions for the specified files, using
    an external program. The default program used is diff, with
    default options "-Npru".

    To select a different program, use the -p/--program option. The
    program will be passed the names of two directories to compare. To
    pass additional options to the program, use -o/--option. These
    will be passed before the names of the directories to compare.

    When two revision arguments are given, then changes are shown
    between those revisions. If only one revision is specified then
    that revision is compared to the working directory, and, when no
    revisions are specified, the working directory files are compared
    to its parent.'''
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
      ('c', 'change', '', _('change made by revision')),
     ] + commands.walkopts,
     _('hg extdiff [OPT]... [FILE]...')),
    }

def uisetup(ui):
    for cmd, path in ui.configitems('extdiff'):
        if cmd.startswith('cmd.'):
            cmd = cmd[4:]
            if not path: path = cmd
            diffopts = ui.config('extdiff', 'opts.' + cmd, '')
            diffopts = diffopts and [diffopts] or []
        elif cmd.startswith('opts.'):
            continue
        else:
            # command = path opts
            if path:
                diffopts = shlex.split(path)
                path = diffopts.pop(0)
            else:
                path, diffopts = cmd, []
        def save(cmd, path, diffopts):
            '''use closure to save diff command to use'''
            def mydiff(ui, repo, *pats, **opts):
                return dodiff(ui, repo, path, diffopts, pats, opts)
            mydiff.__doc__ = _('''\
use %(path)s to diff repository (or selected files)

    Show differences between revisions for the specified files, using the
    %(path)s program.

    When two revision arguments are given, then changes are shown between
    those revisions. If only one revision is specified then that revision is
    compared to the working directory, and, when no revisions are specified,
    the working directory files are compared to its parent.\
''') % dict(path=util.uirepr(path))
            return mydiff
        cmdtable[cmd] = (save(cmd, path, diffopts),
                         cmdtable['extdiff'][1][1:],
                         _('hg %s [OPTION]... [FILE]...') % cmd)
