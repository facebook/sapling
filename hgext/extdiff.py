# extdiff.py - external diff program support for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

'''
The `extdiff' Mercurial extension allows you to use external programs
to compare revisions, or revision with working dir.  The external diff
programs are called with a configurable set of options and two
non-option arguments: paths to directories containing snapshots of
files to compare.

To enable this extension:

  [extensions]
  hgext.extdiff =

The `extdiff' extension also allows to configure new diff commands, so
you do not need to type "hg extdiff -p kdiff3" always.

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
  #(see http://www.vim.org/scripts/script.php?script_id=102)
  # Non english user, be sure to put "let g:DirDiffDynamicDiffText = 1" in
  # your .vimrc
  vimdiff = gvim -f '+next' '+execute "DirDiff" argv(0) argv(1)'

You can use -I/-X and list of file or directory names like normal
"hg diff" command.  The `extdiff' extension makes snapshots of only
needed files, so running the external diff program will actually be
pretty fast (at least faster than having to compare the entire tree).
'''

from mercurial.i18n import _
from mercurial.node import short
from mercurial import cmdutil, util, commands
import os, shlex, shutil, tempfile

def snapshot_node(ui, repo, files, node, tmproot):
    '''snapshot files as of some revision'''
    mf = repo.changectx(node).manifest()
    dirname = os.path.basename(repo.root)
    if dirname == "":
        dirname = "root"
    dirname = '%s.%s' % (dirname, short(node))
    base = os.path.join(tmproot, dirname)
    os.mkdir(base)
    ui.note(_('making snapshot of %d files from rev %s\n') %
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


def snapshot_wdir(ui, repo, files, tmproot):
    '''snapshot files from working directory.
    if not using snapshot, -I/-X does not work and recursive diff
    in tools like kdiff3 and meld displays too many files.'''
    repo_root = repo.root

    dirname = os.path.basename(repo_root)
    if dirname == "":
        dirname = "root"
    base = os.path.join(tmproot, dirname)
    os.mkdir(base)
    ui.note(_('making snapshot of %d files from working dir\n') %
            (len(files)))

    fns_and_mtime = []

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
        fp.close()

        fns_and_mtime.append((dest, os.path.join(repo_root, fn),
            os.path.getmtime(dest)))


    return dirname, fns_and_mtime


def dodiff(ui, repo, diffcmd, diffopts, pats, opts):
    '''Do the actuall diff:

    - copy to a temp structure if diffing 2 internal revisions
    - copy to a temp structure if diffing working revision with
      another one and more than 1 file is changed
    - just invoke the diff for a single file in the working dir
    '''
    node1, node2 = cmdutil.revpair(repo, opts['rev'])
    files, matchfn, anypats = cmdutil.matchpats(repo, pats, opts)
    modified, added, removed, deleted, unknown = repo.status(
        node1, node2, files, match=matchfn)[:5]
    if not (modified or added or removed):
        return 0

    tmproot = tempfile.mkdtemp(prefix='extdiff.')
    dir2root = ''
    try:
        # Always make a copy of node1
        dir1 = snapshot_node(ui, repo, modified + removed, node1, tmproot)
        changes = len(modified) + len(removed) + len(added)

        fns_and_mtime = []

        # If node2 in not the wc or there is >1 change, copy it
        if node2:
            dir2 = snapshot_node(ui, repo, modified + added, node2, tmproot)
        elif changes > 1:
            #we only actually need to get the files to copy back to the working
            #dir in this case (because the other cases are: diffing 2 revisions
            #or single file -- in which case the file is already directly passed
            #to the diff tool).
            dir2, fns_and_mtime = snapshot_wdir(ui, repo, modified + added, tmproot)
        else:
            # This lets the diff tool open the changed file directly
            dir2 = ''
            dir2root = repo.root

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
        ui.debug('running %r in %s\n' % (cmdline, tmproot))
        util.system(cmdline, cwd=tmproot)

        for copy_fn, working_fn, mtime in fns_and_mtime:
            if os.path.getmtime(copy_fn) != mtime:
                ui.debug('File changed while diffing. '
                         'Overwriting: %s (src: %s)\n' % (working_fn, copy_fn))
                util.copyfile(copy_fn, working_fn)

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
            mydiff.__doc__ = '''use %(path)s to diff repository (or selected files)

            Show differences between revisions for the specified
            files, using the %(path)s program.

            When two revision arguments are given, then changes are
            shown between those revisions. If only one revision is
            specified then that revision is compared to the working
            directory, and, when no revisions are specified, the
            working directory files are compared to its parent.''' % {
                'path': util.uirepr(path),
                }
            return mydiff
        cmdtable[cmd] = (save(cmd, path, diffopts),
                         cmdtable['extdiff'][1][1:],
                         _('hg %s [OPTION]... [FILE]...') % cmd)
