# extdiff.py - external diff program support for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# allow to use external programs to compare revisions, or revision
# with working dir. program is called with two arguments: paths to
# directories containing snapshots of files to compare.
#
# to enable:
#
#   [extensions]
#   hgext.extdiff =
#
# also allows to configure new diff commands, so you do not need to
# type "hg extdiff -p kdiff3" always.
#
#   [extdiff]
#   # add new command called vdiff, runs kdiff3
#   cmd.vdiff = kdiff3
#   # add new command called meld, runs meld (no need to name twice)
#   cmd.meld =
#   # add new command called vimdiff, runs gvimdiff with DirDiff plugin
#   #(see http://www.vim.org/scripts/script.php?script_id=102)
#   cmd.vimdiff = LC_ALL=C gvim -f '+bdel 1 2' '+ execute "DirDiff ".argv(0)." ".argv(1)'
#
# you can use -I/-X and list of file or directory names like normal
# "hg diff" command. extdiff makes snapshots of only needed files, so
# compare program will be fast.

from mercurial.demandload import demandload
from mercurial.i18n import gettext as _
from mercurial.node import *
demandload(globals(), 'mercurial:commands,cmdutil,util os shutil tempfile')

def dodiff(ui, repo, diffcmd, pats, opts):
    def snapshot_node(files, node):
        '''snapshot files as of some revision'''
        changes = repo.changelog.read(node)
        mf = repo.manifest.read(changes[0])
        dirname = '%s.%s' % (os.path.basename(repo.root), short(node))
        base = os.path.join(tmproot, dirname)
        os.mkdir(base)
        if not ui.quiet:
            ui.write_err(_('making snapshot of %d files from rev %s\n') %
                         (len(files), short(node)))
        for fn in files:
            wfn = util.pconvert(fn)
            ui.note('  %s\n' % wfn)
            dest = os.path.join(base, wfn)
            destdir = os.path.dirname(dest)
            if not os.path.isdir(destdir):
                os.makedirs(destdir)
            repo.wwrite(wfn, repo.file(fn).read(mf[fn]), open(dest, 'w'))
        return dirname

    def snapshot_wdir(files):
        '''snapshot files from working directory.
        if not using snapshot, -I/-X does not work and recursive diff
        in tools like kdiff3 and meld displays too many files.'''
        dirname = os.path.basename(repo.root)
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
            fp = open(dest, 'w')
            for chunk in util.filechunkiter(repo.wopener(wfn)):
                fp.write(chunk)
        return dirname

    node1, node2 = commands.revpair(ui, repo, opts['rev'])
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
        util.system('%s %s %s %s' %
                    (util.shellquote(diffcmd), ' '.join(opts['option']),
                     util.shellquote(dir1), util.shellquote(dir2)),
                    cwd=tmproot)
        return 1
    finally:
        ui.note(_('cleaning up temp directory\n'))
        shutil.rmtree(tmproot)

def extdiff(ui, repo, *pats, **opts):
    '''use external program to diff repository (or selected files)

    Show differences between revisions for the specified files, using
    an external program.  The default program used is "diff -Npru".
    To select a different program, use the -p option.  The program
    will be passed the names of two directories to compare.  To pass
    additional options to the program, use the -o option.  These will
    be passed before the names of the directories to compare.

    When two revision arguments are given, then changes are
    shown between those revisions. If only one revision is
    specified then that revision is compared to the working
    directory, and, when no revisions are specified, the
    working directory files are compared to its parent.'''
    return dodiff(ui, repo, opts['program'] or 'diff -Npru', pats, opts)

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
        def save(cmd, path):
            '''use closure to save diff command to use'''
            def mydiff(ui, repo, *pats, **opts):
                return dodiff(ui, repo, path, pats, opts)
            mydiff.__doc__ = '''use %s to diff repository (or selected files)

            Show differences between revisions for the specified
            files, using the %s program.

            When two revision arguments are given, then changes are
            shown between those revisions. If only one revision is
            specified then that revision is compared to the working
            directory, and, when no revisions are specified, the
            working directory files are compared to its parent.''' % (cmd, cmd)
            return mydiff
        cmdtable[cmd] = (save(cmd, path),
                         cmdtable['extdiff'][1][1:],
                         _('hg %s [OPT]... [FILE]...') % cmd)
