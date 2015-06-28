# commands.py - command processing for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import hex, bin, nullid, nullrev, short
from lock import release
from i18n import _
import os, re, difflib, time, tempfile, errno, shlex
import sys, socket
import hg, scmutil, util, revlog, copies, error, bookmarks
import patch, help, encoding, templatekw, discovery
import archival, changegroup, cmdutil, hbisect
import sshserver, hgweb, commandserver
import extensions
from hgweb import server as hgweb_server
import merge as mergemod
import minirst, revset, fileset
import dagparser, context, simplemerge, graphmod, copies
import random
import setdiscovery, treediscovery, dagutil, pvec, localrepo
import phases, obsolete, exchange, bundle2, repair
import ui as uimod

table = {}

command = cmdutil.command(table)

# Space delimited list of commands that don't require local repositories.
# This should be populated by passing norepo=True into the @command decorator.
norepo = ''
# Space delimited list of commands that optionally require local repositories.
# This should be populated by passing optionalrepo=True into the @command
# decorator.
optionalrepo = ''
# Space delimited list of commands that will examine arguments looking for
# a repository. This should be populated by passing inferrepo=True into the
# @command decorator.
inferrepo = ''

# label constants
# until 3.5, bookmarks.current was the advertised name, not
# bookmarks.active, so we must use both to avoid breaking old
# custom styles
activebookmarklabel = 'bookmarks.active bookmarks.current'

# common command options

globalopts = [
    ('R', 'repository', '',
     _('repository root directory or name of overlay bundle file'),
     _('REPO')),
    ('', 'cwd', '',
     _('change working directory'), _('DIR')),
    ('y', 'noninteractive', None,
     _('do not prompt, automatically pick the first choice for all prompts')),
    ('q', 'quiet', None, _('suppress output')),
    ('v', 'verbose', None, _('enable additional output')),
    ('', 'config', [],
     _('set/override config option (use \'section.name=value\')'),
     _('CONFIG')),
    ('', 'debug', None, _('enable debugging output')),
    ('', 'debugger', None, _('start debugger')),
    ('', 'encoding', encoding.encoding, _('set the charset encoding'),
     _('ENCODE')),
    ('', 'encodingmode', encoding.encodingmode,
     _('set the charset encoding mode'), _('MODE')),
    ('', 'traceback', None, _('always print a traceback on exception')),
    ('', 'time', None, _('time how long the command takes')),
    ('', 'profile', None, _('print command execution profile')),
    ('', 'version', None, _('output version information and exit')),
    ('h', 'help', None, _('display help and exit')),
    ('', 'hidden', False, _('consider hidden changesets')),
]

dryrunopts = [('n', 'dry-run', None,
               _('do not perform actions, just print output'))]

remoteopts = [
    ('e', 'ssh', '',
     _('specify ssh command to use'), _('CMD')),
    ('', 'remotecmd', '',
     _('specify hg command to run on the remote side'), _('CMD')),
    ('', 'insecure', None,
     _('do not verify server certificate (ignoring web.cacerts config)')),
]

walkopts = [
    ('I', 'include', [],
     _('include names matching the given patterns'), _('PATTERN')),
    ('X', 'exclude', [],
     _('exclude names matching the given patterns'), _('PATTERN')),
]

commitopts = [
    ('m', 'message', '',
     _('use text as commit message'), _('TEXT')),
    ('l', 'logfile', '',
     _('read commit message from file'), _('FILE')),
]

commitopts2 = [
    ('d', 'date', '',
     _('record the specified date as commit date'), _('DATE')),
    ('u', 'user', '',
     _('record the specified user as committer'), _('USER')),
]

# hidden for now
formatteropts = [
    ('T', 'template', '',
     _('display with template (EXPERIMENTAL)'), _('TEMPLATE')),
]

templateopts = [
    ('', 'style', '',
     _('display using template map file (DEPRECATED)'), _('STYLE')),
    ('T', 'template', '',
     _('display with template'), _('TEMPLATE')),
]

logopts = [
    ('p', 'patch', None, _('show patch')),
    ('g', 'git', None, _('use git extended diff format')),
    ('l', 'limit', '',
     _('limit number of changes displayed'), _('NUM')),
    ('M', 'no-merges', None, _('do not show merges')),
    ('', 'stat', None, _('output diffstat-style summary of changes')),
    ('G', 'graph', None, _("show the revision DAG")),
] + templateopts

diffopts = [
    ('a', 'text', None, _('treat all files as text')),
    ('g', 'git', None, _('use git extended diff format')),
    ('', 'nodates', None, _('omit dates from diff headers'))
]

diffwsopts = [
    ('w', 'ignore-all-space', None,
     _('ignore white space when comparing lines')),
    ('b', 'ignore-space-change', None,
     _('ignore changes in the amount of white space')),
    ('B', 'ignore-blank-lines', None,
     _('ignore changes whose lines are all blank')),
    ]

diffopts2 = [
    ('', 'noprefix', None, _('omit a/ and b/ prefixes from filenames')),
    ('p', 'show-function', None, _('show which function each change is in')),
    ('', 'reverse', None, _('produce a diff that undoes the changes')),
    ] + diffwsopts + [
    ('U', 'unified', '',
     _('number of lines of context to show'), _('NUM')),
    ('', 'stat', None, _('output diffstat-style summary of changes')),
    ('', 'root', '', _('produce diffs relative to subdirectory'), _('DIR')),
]

mergetoolopts = [
    ('t', 'tool', '', _('specify merge tool')),
]

similarityopts = [
    ('s', 'similarity', '',
     _('guess renamed files by similarity (0<=s<=100)'), _('SIMILARITY'))
]

subrepoopts = [
    ('S', 'subrepos', None,
     _('recurse into subrepositories'))
]

# Commands start here, listed alphabetically

@command('^add',
    walkopts + subrepoopts + dryrunopts,
    _('[OPTION]... [FILE]...'),
    inferrepo=True)
def add(ui, repo, *pats, **opts):
    """add the specified files on the next commit

    Schedule files to be version controlled and added to the
    repository.

    The files will be added to the repository at the next commit. To
    undo an add before that, see :hg:`forget`.

    If no names are given, add all files to the repository.

    .. container:: verbose

       An example showing how new (unknown) files are added
       automatically by :hg:`add`::

         $ ls
         foo.c
         $ hg status
         ? foo.c
         $ hg add
         adding foo.c
         $ hg status
         A foo.c

    Returns 0 if all files are successfully added.
    """

    m = scmutil.match(repo[None], pats, opts)
    rejected = cmdutil.add(ui, repo, m, "", False, **opts)
    return rejected and 1 or 0

@command('addremove',
    similarityopts + subrepoopts + walkopts + dryrunopts,
    _('[OPTION]... [FILE]...'),
    inferrepo=True)
def addremove(ui, repo, *pats, **opts):
    """add all new files, delete all missing files

    Add all new files and remove all missing files from the
    repository.

    New files are ignored if they match any of the patterns in
    ``.hgignore``. As with add, these changes take effect at the next
    commit.

    Use the -s/--similarity option to detect renamed files. This
    option takes a percentage between 0 (disabled) and 100 (files must
    be identical) as its parameter. With a parameter greater than 0,
    this compares every removed file with every added file and records
    those similar enough as renames. Detecting renamed files this way
    can be expensive. After using this option, :hg:`status -C` can be
    used to check which files were identified as moved or renamed. If
    not specified, -s/--similarity defaults to 100 and only renames of
    identical files are detected.

    Returns 0 if all files are successfully added.
    """
    try:
        sim = float(opts.get('similarity') or 100)
    except ValueError:
        raise util.Abort(_('similarity must be a number'))
    if sim < 0 or sim > 100:
        raise util.Abort(_('similarity must be between 0 and 100'))
    matcher = scmutil.match(repo[None], pats, opts)
    return scmutil.addremove(repo, matcher, "", opts, similarity=sim / 100.0)

@command('^annotate|blame',
    [('r', 'rev', '', _('annotate the specified revision'), _('REV')),
    ('', 'follow', None,
     _('follow copies/renames and list the filename (DEPRECATED)')),
    ('', 'no-follow', None, _("don't follow copies and renames")),
    ('a', 'text', None, _('treat all files as text')),
    ('u', 'user', None, _('list the author (long with -v)')),
    ('f', 'file', None, _('list the filename')),
    ('d', 'date', None, _('list the date (short with -q)')),
    ('n', 'number', None, _('list the revision number (default)')),
    ('c', 'changeset', None, _('list the changeset')),
    ('l', 'line-number', None, _('show line number at the first appearance'))
    ] + diffwsopts + walkopts + formatteropts,
    _('[-r REV] [-f] [-a] [-u] [-d] [-n] [-c] [-l] FILE...'),
    inferrepo=True)
def annotate(ui, repo, *pats, **opts):
    """show changeset information by line for each file

    List changes in files, showing the revision id responsible for
    each line

    This command is useful for discovering when a change was made and
    by whom.

    Without the -a/--text option, annotate will avoid processing files
    it detects as binary. With -a, annotate will annotate the file
    anyway, although the results will probably be neither useful
    nor desirable.

    Returns 0 on success.
    """
    if not pats:
        raise util.Abort(_('at least one filename or pattern is required'))

    if opts.get('follow'):
        # --follow is deprecated and now just an alias for -f/--file
        # to mimic the behavior of Mercurial before version 1.5
        opts['file'] = True

    ctx = scmutil.revsingle(repo, opts.get('rev'))

    fm = ui.formatter('annotate', opts)
    if ui.quiet:
        datefunc = util.shortdate
    else:
        datefunc = util.datestr
    if ctx.rev() is None:
        def hexfn(node):
            if node is None:
                return None
            else:
                return fm.hexfunc(node)
        if opts.get('changeset'):
            # omit "+" suffix which is appended to node hex
            def formatrev(rev):
                if rev is None:
                    return '%d' % ctx.p1().rev()
                else:
                    return '%d' % rev
        else:
            def formatrev(rev):
                if rev is None:
                    return '%d+' % ctx.p1().rev()
                else:
                    return '%d ' % rev
        def formathex(hex):
            if hex is None:
                return '%s+' % fm.hexfunc(ctx.p1().node())
            else:
                return '%s ' % hex
    else:
        hexfn = fm.hexfunc
        formatrev = formathex = str

    opmap = [('user', ' ', lambda x: x[0].user(), ui.shortuser),
             ('number', ' ', lambda x: x[0].rev(), formatrev),
             ('changeset', ' ', lambda x: hexfn(x[0].node()), formathex),
             ('date', ' ', lambda x: x[0].date(), util.cachefunc(datefunc)),
             ('file', ' ', lambda x: x[0].path(), str),
             ('line_number', ':', lambda x: x[1], str),
            ]
    fieldnamemap = {'number': 'rev', 'changeset': 'node'}

    if (not opts.get('user') and not opts.get('changeset')
        and not opts.get('date') and not opts.get('file')):
        opts['number'] = True

    linenumber = opts.get('line_number') is not None
    if linenumber and (not opts.get('changeset')) and (not opts.get('number')):
        raise util.Abort(_('at least one of -n/-c is required for -l'))

    if fm:
        def makefunc(get, fmt):
            return get
    else:
        def makefunc(get, fmt):
            return lambda x: fmt(get(x))
    funcmap = [(makefunc(get, fmt), sep) for op, sep, get, fmt in opmap
               if opts.get(op)]
    funcmap[0] = (funcmap[0][0], '') # no separator in front of first column
    fields = ' '.join(fieldnamemap.get(op, op) for op, sep, get, fmt in opmap
                      if opts.get(op))

    def bad(x, y):
        raise util.Abort("%s: %s" % (x, y))

    m = scmutil.match(ctx, pats, opts, badfn=bad)

    follow = not opts.get('no_follow')
    diffopts = patch.difffeatureopts(ui, opts, section='annotate',
                                     whitespace=True)
    for abs in ctx.walk(m):
        fctx = ctx[abs]
        if not opts.get('text') and util.binary(fctx.data()):
            fm.plain(_("%s: binary file\n") % ((pats and m.rel(abs)) or abs))
            continue

        lines = fctx.annotate(follow=follow, linenumber=linenumber,
                              diffopts=diffopts)
        formats = []
        pieces = []

        for f, sep in funcmap:
            l = [f(n) for n, dummy in lines]
            if l:
                if fm:
                    formats.append(['%s' for x in l])
                else:
                    sizes = [encoding.colwidth(x) for x in l]
                    ml = max(sizes)
                    formats.append([sep + ' ' * (ml - w) + '%s' for w in sizes])
                pieces.append(l)

        for f, p, l in zip(zip(*formats), zip(*pieces), lines):
            fm.startitem()
            fm.write(fields, "".join(f), *p)
            fm.write('line', ": %s", l[1])

        if lines and not lines[-1][1].endswith('\n'):
            fm.plain('\n')

    fm.end()

@command('archive',
    [('', 'no-decode', None, _('do not pass files through decoders')),
    ('p', 'prefix', '', _('directory prefix for files in archive'),
     _('PREFIX')),
    ('r', 'rev', '', _('revision to distribute'), _('REV')),
    ('t', 'type', '', _('type of distribution to create'), _('TYPE')),
    ] + subrepoopts + walkopts,
    _('[OPTION]... DEST'))
def archive(ui, repo, dest, **opts):
    '''create an unversioned archive of a repository revision

    By default, the revision used is the parent of the working
    directory; use -r/--rev to specify a different revision.

    The archive type is automatically detected based on file
    extension (or override using -t/--type).

    .. container:: verbose

      Examples:

      - create a zip file containing the 1.0 release::

          hg archive -r 1.0 project-1.0.zip

      - create a tarball excluding .hg files::

          hg archive project.tar.gz -X ".hg*"

    Valid types are:

    :``files``: a directory full of files (default)
    :``tar``:   tar archive, uncompressed
    :``tbz2``:  tar archive, compressed using bzip2
    :``tgz``:   tar archive, compressed using gzip
    :``uzip``:  zip archive, uncompressed
    :``zip``:   zip archive, compressed using deflate

    The exact name of the destination archive or directory is given
    using a format string; see :hg:`help export` for details.

    Each member added to an archive file has a directory prefix
    prepended. Use -p/--prefix to specify a format string for the
    prefix. The default is the basename of the archive, with suffixes
    removed.

    Returns 0 on success.
    '''

    ctx = scmutil.revsingle(repo, opts.get('rev'))
    if not ctx:
        raise util.Abort(_('no working directory: please specify a revision'))
    node = ctx.node()
    dest = cmdutil.makefilename(repo, dest, node)
    if os.path.realpath(dest) == repo.root:
        raise util.Abort(_('repository root cannot be destination'))

    kind = opts.get('type') or archival.guesskind(dest) or 'files'
    prefix = opts.get('prefix')

    if dest == '-':
        if kind == 'files':
            raise util.Abort(_('cannot archive plain files to stdout'))
        dest = cmdutil.makefileobj(repo, dest)
        if not prefix:
            prefix = os.path.basename(repo.root) + '-%h'

    prefix = cmdutil.makefilename(repo, prefix, node)
    matchfn = scmutil.match(ctx, [], opts)
    archival.archive(repo, dest, node, kind, not opts.get('no_decode'),
                     matchfn, prefix, subrepos=opts.get('subrepos'))

@command('backout',
    [('', 'merge', None, _('merge with old dirstate parent after backout')),
    ('', 'commit', None, _('commit if no conflicts were encountered')),
    ('', 'parent', '',
     _('parent to choose when backing out merge (DEPRECATED)'), _('REV')),
    ('r', 'rev', '', _('revision to backout'), _('REV')),
    ('e', 'edit', False, _('invoke editor on commit messages')),
    ] + mergetoolopts + walkopts + commitopts + commitopts2,
    _('[OPTION]... [-r] REV'))
def backout(ui, repo, node=None, rev=None, commit=False, **opts):
    '''reverse effect of earlier changeset

    Prepare a new changeset with the effect of REV undone in the
    current working directory.

    If REV is the parent of the working directory, then this new changeset
    is committed automatically. Otherwise, hg needs to merge the
    changes and the merged result is left uncommitted.

    .. note::

      backout cannot be used to fix either an unwanted or
      incorrect merge.

    .. container:: verbose

      By default, the pending changeset will have one parent,
      maintaining a linear history. With --merge, the pending
      changeset will instead have two parents: the old parent of the
      working directory and a new child of REV that simply undoes REV.

      Before version 1.7, the behavior without --merge was equivalent
      to specifying --merge followed by :hg:`update --clean .` to
      cancel the merge and leave the child of REV as a head to be
      merged separately.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    Returns 0 on success, 1 if nothing to backout or there are unresolved
    files.
    '''
    if rev and node:
        raise util.Abort(_("please specify just one revision"))

    if not rev:
        rev = node

    if not rev:
        raise util.Abort(_("please specify a revision to backout"))

    date = opts.get('date')
    if date:
        opts['date'] = util.parsedate(date)

    cmdutil.checkunfinished(repo)
    cmdutil.bailifchanged(repo)
    node = scmutil.revsingle(repo, rev).node()

    op1, op2 = repo.dirstate.parents()
    if not repo.changelog.isancestor(node, op1):
        raise util.Abort(_('cannot backout change that is not an ancestor'))

    p1, p2 = repo.changelog.parents(node)
    if p1 == nullid:
        raise util.Abort(_('cannot backout a change with no parents'))
    if p2 != nullid:
        if not opts.get('parent'):
            raise util.Abort(_('cannot backout a merge changeset'))
        p = repo.lookup(opts['parent'])
        if p not in (p1, p2):
            raise util.Abort(_('%s is not a parent of %s') %
                             (short(p), short(node)))
        parent = p
    else:
        if opts.get('parent'):
            raise util.Abort(_('cannot use --parent on non-merge changeset'))
        parent = p1

    # the backout should appear on the same branch
    wlock = repo.wlock()
    try:
        branch = repo.dirstate.branch()
        bheads = repo.branchheads(branch)
        rctx = scmutil.revsingle(repo, hex(parent))
        if not opts.get('merge') and op1 != node:
            try:
                ui.setconfig('ui', 'forcemerge', opts.get('tool', ''),
                             'backout')
                repo.dirstate.beginparentchange()
                stats = mergemod.update(repo, parent, True, True, False,
                                        node, False)
                repo.setparents(op1, op2)
                repo.dirstate.endparentchange()
                hg._showstats(repo, stats)
                if stats[3]:
                    repo.ui.status(_("use 'hg resolve' to retry unresolved "
                                     "file merges\n"))
                    return 1
                elif not commit:
                    msg = _("changeset %s backed out, "
                            "don't forget to commit.\n")
                    ui.status(msg % short(node))
                    return 0
            finally:
                ui.setconfig('ui', 'forcemerge', '', '')
        else:
            hg.clean(repo, node, show_stats=False)
            repo.dirstate.setbranch(branch)
            cmdutil.revert(ui, repo, rctx, repo.dirstate.parents())


        def commitfunc(ui, repo, message, match, opts):
            editform = 'backout'
            e = cmdutil.getcommiteditor(editform=editform, **opts)
            if not message:
                # we don't translate commit messages
                message = "Backed out changeset %s" % short(node)
                e = cmdutil.getcommiteditor(edit=True, editform=editform)
            return repo.commit(message, opts.get('user'), opts.get('date'),
                               match, editor=e)
        newnode = cmdutil.commit(ui, repo, commitfunc, [], opts)
        if not newnode:
            ui.status(_("nothing changed\n"))
            return 1
        cmdutil.commitstatus(repo, newnode, branch, bheads)

        def nice(node):
            return '%d:%s' % (repo.changelog.rev(node), short(node))
        ui.status(_('changeset %s backs out changeset %s\n') %
                  (nice(repo.changelog.tip()), nice(node)))
        if opts.get('merge') and op1 != node:
            hg.clean(repo, op1, show_stats=False)
            ui.status(_('merging with changeset %s\n')
                      % nice(repo.changelog.tip()))
            try:
                ui.setconfig('ui', 'forcemerge', opts.get('tool', ''),
                             'backout')
                return hg.merge(repo, hex(repo.changelog.tip()))
            finally:
                ui.setconfig('ui', 'forcemerge', '', '')
    finally:
        wlock.release()
    return 0

@command('bisect',
    [('r', 'reset', False, _('reset bisect state')),
    ('g', 'good', False, _('mark changeset good')),
    ('b', 'bad', False, _('mark changeset bad')),
    ('s', 'skip', False, _('skip testing changeset')),
    ('e', 'extend', False, _('extend the bisect range')),
    ('c', 'command', '', _('use command to check changeset state'), _('CMD')),
    ('U', 'noupdate', False, _('do not update to target'))],
    _("[-gbsr] [-U] [-c CMD] [REV]"))
def bisect(ui, repo, rev=None, extra=None, command=None,
               reset=None, good=None, bad=None, skip=None, extend=None,
               noupdate=None):
    """subdivision search of changesets

    This command helps to find changesets which introduce problems. To
    use, mark the earliest changeset you know exhibits the problem as
    bad, then mark the latest changeset which is free from the problem
    as good. Bisect will update your working directory to a revision
    for testing (unless the -U/--noupdate option is specified). Once
    you have performed tests, mark the working directory as good or
    bad, and bisect will either update to another candidate changeset
    or announce that it has found the bad revision.

    As a shortcut, you can also use the revision argument to mark a
    revision as good or bad without checking it out first.

    If you supply a command, it will be used for automatic bisection.
    The environment variable HG_NODE will contain the ID of the
    changeset being tested. The exit status of the command will be
    used to mark revisions as good or bad: status 0 means good, 125
    means to skip the revision, 127 (command not found) will abort the
    bisection, and any other non-zero exit status means the revision
    is bad.

    .. container:: verbose

      Some examples:

      - start a bisection with known bad revision 34, and good revision 12::

          hg bisect --bad 34
          hg bisect --good 12

      - advance the current bisection by marking current revision as good or
        bad::

          hg bisect --good
          hg bisect --bad

      - mark the current revision, or a known revision, to be skipped (e.g. if
        that revision is not usable because of another issue)::

          hg bisect --skip
          hg bisect --skip 23

      - skip all revisions that do not touch directories ``foo`` or ``bar``::

          hg bisect --skip "!( file('path:foo') & file('path:bar') )"

      - forget the current bisection::

          hg bisect --reset

      - use 'make && make tests' to automatically find the first broken
        revision::

          hg bisect --reset
          hg bisect --bad 34
          hg bisect --good 12
          hg bisect --command "make && make tests"

      - see all changesets whose states are already known in the current
        bisection::

          hg log -r "bisect(pruned)"

      - see the changeset currently being bisected (especially useful
        if running with -U/--noupdate)::

          hg log -r "bisect(current)"

      - see all changesets that took part in the current bisection::

          hg log -r "bisect(range)"

      - you can even get a nice graph::

          hg log --graph -r "bisect(range)"

      See :hg:`help revsets` for more about the `bisect()` keyword.

    Returns 0 on success.
    """
    def extendbisectrange(nodes, good):
        # bisect is incomplete when it ends on a merge node and
        # one of the parent was not checked.
        parents = repo[nodes[0]].parents()
        if len(parents) > 1:
            if good:
                side = state['bad']
            else:
                side = state['good']
            num = len(set(i.node() for i in parents) & set(side))
            if num == 1:
                return parents[0].ancestor(parents[1])
        return None

    def print_result(nodes, good):
        displayer = cmdutil.show_changeset(ui, repo, {})
        if len(nodes) == 1:
            # narrowed it down to a single revision
            if good:
                ui.write(_("The first good revision is:\n"))
            else:
                ui.write(_("The first bad revision is:\n"))
            displayer.show(repo[nodes[0]])
            extendnode = extendbisectrange(nodes, good)
            if extendnode is not None:
                ui.write(_('Not all ancestors of this changeset have been'
                           ' checked.\nUse bisect --extend to continue the '
                           'bisection from\nthe common ancestor, %s.\n')
                         % extendnode)
        else:
            # multiple possible revisions
            if good:
                ui.write(_("Due to skipped revisions, the first "
                        "good revision could be any of:\n"))
            else:
                ui.write(_("Due to skipped revisions, the first "
                        "bad revision could be any of:\n"))
            for n in nodes:
                displayer.show(repo[n])
        displayer.close()

    def check_state(state, interactive=True):
        if not state['good'] or not state['bad']:
            if (good or bad or skip or reset) and interactive:
                return
            if not state['good']:
                raise util.Abort(_('cannot bisect (no known good revisions)'))
            else:
                raise util.Abort(_('cannot bisect (no known bad revisions)'))
        return True

    # backward compatibility
    if rev in "good bad reset init".split():
        ui.warn(_("(use of 'hg bisect <cmd>' is deprecated)\n"))
        cmd, rev, extra = rev, extra, None
        if cmd == "good":
            good = True
        elif cmd == "bad":
            bad = True
        else:
            reset = True
    elif extra or good + bad + skip + reset + extend + bool(command) > 1:
        raise util.Abort(_('incompatible arguments'))

    cmdutil.checkunfinished(repo)

    if reset:
        p = repo.join("bisect.state")
        if os.path.exists(p):
            os.unlink(p)
        return

    state = hbisect.load_state(repo)

    if command:
        changesets = 1
        if noupdate:
            try:
                node = state['current'][0]
            except LookupError:
                raise util.Abort(_('current bisect revision is unknown - '
                                   'start a new bisect to fix'))
        else:
            node, p2 = repo.dirstate.parents()
            if p2 != nullid:
                raise util.Abort(_('current bisect revision is a merge'))
        try:
            while changesets:
                # update state
                state['current'] = [node]
                hbisect.save_state(repo, state)
                status = ui.system(command, environ={'HG_NODE': hex(node)})
                if status == 125:
                    transition = "skip"
                elif status == 0:
                    transition = "good"
                # status < 0 means process was killed
                elif status == 127:
                    raise util.Abort(_("failed to execute %s") % command)
                elif status < 0:
                    raise util.Abort(_("%s killed") % command)
                else:
                    transition = "bad"
                ctx = scmutil.revsingle(repo, rev, node)
                rev = None # clear for future iterations
                state[transition].append(ctx.node())
                ui.status(_('changeset %d:%s: %s\n') % (ctx, ctx, transition))
                check_state(state, interactive=False)
                # bisect
                nodes, changesets, bgood = hbisect.bisect(repo.changelog, state)
                # update to next check
                node = nodes[0]
                if not noupdate:
                    cmdutil.bailifchanged(repo)
                    hg.clean(repo, node, show_stats=False)
        finally:
            state['current'] = [node]
            hbisect.save_state(repo, state)
        print_result(nodes, bgood)
        return

    # update state

    if rev:
        nodes = [repo.lookup(i) for i in scmutil.revrange(repo, [rev])]
    else:
        nodes = [repo.lookup('.')]

    if good or bad or skip:
        if good:
            state['good'] += nodes
        elif bad:
            state['bad'] += nodes
        elif skip:
            state['skip'] += nodes
        hbisect.save_state(repo, state)

    if not check_state(state):
        return

    # actually bisect
    nodes, changesets, good = hbisect.bisect(repo.changelog, state)
    if extend:
        if not changesets:
            extendnode = extendbisectrange(nodes, good)
            if extendnode is not None:
                ui.write(_("Extending search to changeset %d:%s\n")
                         % (extendnode.rev(), extendnode))
                state['current'] = [extendnode.node()]
                hbisect.save_state(repo, state)
                if noupdate:
                    return
                cmdutil.bailifchanged(repo)
                return hg.clean(repo, extendnode.node())
        raise util.Abort(_("nothing to extend"))

    if changesets == 0:
        print_result(nodes, good)
    else:
        assert len(nodes) == 1 # only a single node can be tested next
        node = nodes[0]
        # compute the approximate number of remaining tests
        tests, size = 0, 2
        while size <= changesets:
            tests, size = tests + 1, size * 2
        rev = repo.changelog.rev(node)
        ui.write(_("Testing changeset %d:%s "
                   "(%d changesets remaining, ~%d tests)\n")
                 % (rev, short(node), changesets, tests))
        state['current'] = [node]
        hbisect.save_state(repo, state)
        if not noupdate:
            cmdutil.bailifchanged(repo)
            return hg.clean(repo, node)

@command('bookmarks|bookmark',
    [('f', 'force', False, _('force')),
    ('r', 'rev', '', _('revision'), _('REV')),
    ('d', 'delete', False, _('delete a given bookmark')),
    ('m', 'rename', '', _('rename a given bookmark'), _('NAME')),
    ('i', 'inactive', False, _('mark a bookmark inactive')),
    ] + formatteropts,
    _('hg bookmarks [OPTIONS]... [NAME]...'))
def bookmark(ui, repo, *names, **opts):
    '''create a new bookmark or list existing bookmarks

    Bookmarks are labels on changesets to help track lines of development.
    Bookmarks are unversioned and can be moved, renamed and deleted.
    Deleting or moving a bookmark has no effect on the associated changesets.

    Creating or updating to a bookmark causes it to be marked as 'active'.
    The active bookmark is indicated with a '*'.
    When a commit is made, the active bookmark will advance to the new commit.
    A plain :hg:`update` will also advance an active bookmark, if possible.
    Updating away from a bookmark will cause it to be deactivated.

    Bookmarks can be pushed and pulled between repositories (see
    :hg:`help push` and :hg:`help pull`). If a shared bookmark has
    diverged, a new 'divergent bookmark' of the form 'name@path' will
    be created. Using :hg:`merge` will resolve the divergence.

    A bookmark named '@' has the special property that :hg:`clone` will
    check it out by default if it exists.

    .. container:: verbose

      Examples:

      - create an active bookmark for a new line of development::

          hg book new-feature

      - create an inactive bookmark as a place marker::

          hg book -i reviewed

      - create an inactive bookmark on another changeset::

          hg book -r .^ tested

      - move the '@' bookmark from another branch::

          hg book -f @
    '''
    force = opts.get('force')
    rev = opts.get('rev')
    delete = opts.get('delete')
    rename = opts.get('rename')
    inactive = opts.get('inactive')

    def checkformat(mark):
        mark = mark.strip()
        if not mark:
            raise util.Abort(_("bookmark names cannot consist entirely of "
                               "whitespace"))
        scmutil.checknewlabel(repo, mark, 'bookmark')
        return mark

    def checkconflict(repo, mark, cur, force=False, target=None):
        if mark in marks and not force:
            if target:
                if marks[mark] == target and target == cur:
                    # re-activating a bookmark
                    return
                anc = repo.changelog.ancestors([repo[target].rev()])
                bmctx = repo[marks[mark]]
                divs = [repo[b].node() for b in marks
                        if b.split('@', 1)[0] == mark.split('@', 1)[0]]

                # allow resolving a single divergent bookmark even if moving
                # the bookmark across branches when a revision is specified
                # that contains a divergent bookmark
                if bmctx.rev() not in anc and target in divs:
                    bookmarks.deletedivergent(repo, [target], mark)
                    return

                deletefrom = [b for b in divs
                              if repo[b].rev() in anc or b == target]
                bookmarks.deletedivergent(repo, deletefrom, mark)
                if bookmarks.validdest(repo, bmctx, repo[target]):
                    ui.status(_("moving bookmark '%s' forward from %s\n") %
                              (mark, short(bmctx.node())))
                    return
            raise util.Abort(_("bookmark '%s' already exists "
                               "(use -f to force)") % mark)
        if ((mark in repo.branchmap() or mark == repo.dirstate.branch())
            and not force):
            raise util.Abort(
                _("a bookmark cannot have the name of an existing branch"))

    if delete and rename:
        raise util.Abort(_("--delete and --rename are incompatible"))
    if delete and rev:
        raise util.Abort(_("--rev is incompatible with --delete"))
    if rename and rev:
        raise util.Abort(_("--rev is incompatible with --rename"))
    if not names and (delete or rev):
        raise util.Abort(_("bookmark name required"))

    if delete or rename or names or inactive:
        wlock = repo.wlock()
        try:
            cur = repo.changectx('.').node()
            marks = repo._bookmarks
            if delete:
                for mark in names:
                    if mark not in marks:
                        raise util.Abort(_("bookmark '%s' does not exist") %
                                         mark)
                    if mark == repo._activebookmark:
                        bookmarks.deactivate(repo)
                    del marks[mark]
                marks.write()

            elif rename:
                if not names:
                    raise util.Abort(_("new bookmark name required"))
                elif len(names) > 1:
                    raise util.Abort(_("only one new bookmark name allowed"))
                mark = checkformat(names[0])
                if rename not in marks:
                    raise util.Abort(_("bookmark '%s' does not exist") % rename)
                checkconflict(repo, mark, cur, force)
                marks[mark] = marks[rename]
                if repo._activebookmark == rename and not inactive:
                    bookmarks.activate(repo, mark)
                del marks[rename]
                marks.write()

            elif names:
                newact = None
                for mark in names:
                    mark = checkformat(mark)
                    if newact is None:
                        newact = mark
                    if inactive and mark == repo._activebookmark:
                        bookmarks.deactivate(repo)
                        return
                    tgt = cur
                    if rev:
                        tgt = scmutil.revsingle(repo, rev).node()
                    checkconflict(repo, mark, cur, force, tgt)
                    marks[mark] = tgt
                if not inactive and cur == marks[newact] and not rev:
                    bookmarks.activate(repo, newact)
                elif cur != tgt and newact == repo._activebookmark:
                    bookmarks.deactivate(repo)
                marks.write()

            elif inactive:
                if len(marks) == 0:
                    ui.status(_("no bookmarks set\n"))
                elif not repo._activebookmark:
                    ui.status(_("no active bookmark\n"))
                else:
                    bookmarks.deactivate(repo)
        finally:
            wlock.release()
    else: # show bookmarks
        fm = ui.formatter('bookmarks', opts)
        hexfn = fm.hexfunc
        marks = repo._bookmarks
        if len(marks) == 0 and not fm:
            ui.status(_("no bookmarks set\n"))
        for bmark, n in sorted(marks.iteritems()):
            active = repo._activebookmark
            if bmark == active:
                prefix, label = '*', activebookmarklabel
            else:
                prefix, label = ' ', ''

            fm.startitem()
            if not ui.quiet:
                fm.plain(' %s ' % prefix, label=label)
            fm.write('bookmark', '%s', bmark, label=label)
            pad = " " * (25 - encoding.colwidth(bmark))
            fm.condwrite(not ui.quiet, 'rev node', pad + ' %d:%s',
                         repo.changelog.rev(n), hexfn(n), label=label)
            fm.data(active=(bmark == active))
            fm.plain('\n')
        fm.end()

@command('branch',
    [('f', 'force', None,
     _('set branch name even if it shadows an existing branch')),
    ('C', 'clean', None, _('reset branch name to parent branch name'))],
    _('[-fC] [NAME]'))
def branch(ui, repo, label=None, **opts):
    """set or show the current branch name

    .. note::

       Branch names are permanent and global. Use :hg:`bookmark` to create a
       light-weight bookmark instead. See :hg:`help glossary` for more
       information about named branches and bookmarks.

    With no argument, show the current branch name. With one argument,
    set the working directory branch name (the branch will not exist
    in the repository until the next commit). Standard practice
    recommends that primary development take place on the 'default'
    branch.

    Unless -f/--force is specified, branch will not let you set a
    branch name that already exists.

    Use -C/--clean to reset the working directory branch to that of
    the parent of the working directory, negating a previous branch
    change.

    Use the command :hg:`update` to switch to an existing branch. Use
    :hg:`commit --close-branch` to mark this branch head as closed.
    When all heads of the branch are closed, the branch will be
    considered closed.

    Returns 0 on success.
    """
    if label:
        label = label.strip()

    if not opts.get('clean') and not label:
        ui.write("%s\n" % repo.dirstate.branch())
        return

    wlock = repo.wlock()
    try:
        if opts.get('clean'):
            label = repo[None].p1().branch()
            repo.dirstate.setbranch(label)
            ui.status(_('reset working directory to branch %s\n') % label)
        elif label:
            if not opts.get('force') and label in repo.branchmap():
                if label not in [p.branch() for p in repo.parents()]:
                    raise util.Abort(_('a branch of the same name already'
                                       ' exists'),
                                     # i18n: "it" refers to an existing branch
                                     hint=_("use 'hg update' to switch to it"))
            scmutil.checknewlabel(repo, label, 'branch')
            repo.dirstate.setbranch(label)
            ui.status(_('marked working directory as branch %s\n') % label)

            # find any open named branches aside from default
            others = [n for n, h, t, c in repo.branchmap().iterbranches()
                      if n != "default" and not c]
            if not others:
                ui.status(_('(branches are permanent and global, '
                            'did you want a bookmark?)\n'))
    finally:
        wlock.release()

@command('branches',
    [('a', 'active', False,
      _('show only branches that have unmerged heads (DEPRECATED)')),
     ('c', 'closed', False, _('show normal and closed branches')),
    ] + formatteropts,
    _('[-ac]'))
def branches(ui, repo, active=False, closed=False, **opts):
    """list repository named branches

    List the repository's named branches, indicating which ones are
    inactive. If -c/--closed is specified, also list branches which have
    been marked closed (see :hg:`commit --close-branch`).

    Use the command :hg:`update` to switch to an existing branch.

    Returns 0.
    """

    fm = ui.formatter('branches', opts)
    hexfunc = fm.hexfunc

    allheads = set(repo.heads())
    branches = []
    for tag, heads, tip, isclosed in repo.branchmap().iterbranches():
        isactive = not isclosed and bool(set(heads) & allheads)
        branches.append((tag, repo[tip], isactive, not isclosed))
    branches.sort(key=lambda i: (i[2], i[1].rev(), i[0], i[3]),
                  reverse=True)

    for tag, ctx, isactive, isopen in branches:
        if active and not isactive:
            continue
        if isactive:
            label = 'branches.active'
            notice = ''
        elif not isopen:
            if not closed:
                continue
            label = 'branches.closed'
            notice = _(' (closed)')
        else:
            label = 'branches.inactive'
            notice = _(' (inactive)')
        current = (tag == repo.dirstate.branch())
        if current:
            label = 'branches.current'

        fm.startitem()
        fm.write('branch', '%s', tag, label=label)
        rev = ctx.rev()
        padsize = max(31 - len(str(rev)) - encoding.colwidth(tag), 0)
        fmt = ' ' * padsize + ' %d:%s'
        fm.condwrite(not ui.quiet, 'rev node', fmt, rev, hexfunc(ctx.node()),
                     label='log.changeset changeset.%s' % ctx.phasestr())
        fm.data(active=isactive, closed=not isopen, current=current)
        if not ui.quiet:
            fm.plain(notice)
        fm.plain('\n')
    fm.end()

@command('bundle',
    [('f', 'force', None, _('run even when the destination is unrelated')),
    ('r', 'rev', [], _('a changeset intended to be added to the destination'),
     _('REV')),
    ('b', 'branch', [], _('a specific branch you would like to bundle'),
     _('BRANCH')),
    ('', 'base', [],
     _('a base changeset assumed to be available at the destination'),
     _('REV')),
    ('a', 'all', None, _('bundle all changesets in the repository')),
    ('t', 'type', 'bzip2', _('bundle compression type to use'), _('TYPE')),
    ] + remoteopts,
    _('[-f] [-t TYPE] [-a] [-r REV]... [--base REV]... FILE [DEST]'))
def bundle(ui, repo, fname, dest=None, **opts):
    """create a changegroup file

    Generate a compressed changegroup file collecting changesets not
    known to be in another repository.

    If you omit the destination repository, then hg assumes the
    destination will have all the nodes you specify with --base
    parameters. To create a bundle containing all changesets, use
    -a/--all (or --base null).

    You can change compression method with the -t/--type option.
    The available compression methods are: none, bzip2, and
    gzip (by default, bundles are compressed using bzip2).

    The bundle file can then be transferred using conventional means
    and applied to another repository with the unbundle or pull
    command. This is useful when direct push and pull are not
    available or when exporting an entire repository is undesirable.

    Applying bundles preserves all changeset contents including
    permissions, copy/rename information, and revision history.

    Returns 0 on success, 1 if no changes found.
    """
    revs = None
    if 'rev' in opts:
        revs = scmutil.revrange(repo, opts['rev'])

    bundletype = opts.get('type', 'bzip2').lower()
    btypes = {'none': 'HG10UN',
              'bzip2': 'HG10BZ',
              'gzip': 'HG10GZ',
              'bundle2': 'HG20'}
    bundletype = btypes.get(bundletype)
    if bundletype not in changegroup.bundletypes:
        raise util.Abort(_('unknown bundle type specified with --type'))

    if opts.get('all'):
        base = ['null']
    else:
        base = scmutil.revrange(repo, opts.get('base'))
    # TODO: get desired bundlecaps from command line.
    bundlecaps = None
    if base:
        if dest:
            raise util.Abort(_("--base is incompatible with specifying "
                               "a destination"))
        common = [repo.lookup(rev) for rev in base]
        heads = revs and map(repo.lookup, revs) or revs
        cg = changegroup.getchangegroup(repo, 'bundle', heads=heads,
                                         common=common, bundlecaps=bundlecaps)
        outgoing = None
    else:
        dest = ui.expandpath(dest or 'default-push', dest or 'default')
        dest, branches = hg.parseurl(dest, opts.get('branch'))
        other = hg.peer(repo, opts, dest)
        revs, checkout = hg.addbranchrevs(repo, repo, branches, revs)
        heads = revs and map(repo.lookup, revs) or revs
        outgoing = discovery.findcommonoutgoing(repo, other,
                                                onlyheads=heads,
                                                force=opts.get('force'),
                                                portable=True)
        cg = changegroup.getlocalchangegroup(repo, 'bundle', outgoing,
                                             bundlecaps)
    if not cg:
        scmutil.nochangesfound(ui, repo, outgoing and outgoing.excluded)
        return 1

    changegroup.writebundle(ui, cg, fname, bundletype)

@command('cat',
    [('o', 'output', '',
     _('print output to file with formatted name'), _('FORMAT')),
    ('r', 'rev', '', _('print the given revision'), _('REV')),
    ('', 'decode', None, _('apply any matching decode filter')),
    ] + walkopts,
    _('[OPTION]... FILE...'),
    inferrepo=True)
def cat(ui, repo, file1, *pats, **opts):
    """output the current or given revision of files

    Print the specified files as they were at the given revision. If
    no revision is given, the parent of the working directory is used.

    Output may be to a file, in which case the name of the file is
    given using a format string. The formatting rules as follows:

    :``%%``: literal "%" character
    :``%s``: basename of file being printed
    :``%d``: dirname of file being printed, or '.' if in repository root
    :``%p``: root-relative path name of file being printed
    :``%H``: changeset hash (40 hexadecimal digits)
    :``%R``: changeset revision number
    :``%h``: short-form changeset hash (12 hexadecimal digits)
    :``%r``: zero-padded changeset revision number
    :``%b``: basename of the exporting repository

    Returns 0 on success.
    """
    ctx = scmutil.revsingle(repo, opts.get('rev'))
    m = scmutil.match(ctx, (file1,) + pats, opts)

    return cmdutil.cat(ui, repo, ctx, m, '', **opts)

@command('^clone',
    [('U', 'noupdate', None, _('the clone will include an empty working '
                               'directory (only a repository)')),
    ('u', 'updaterev', '', _('revision, tag or branch to check out'), _('REV')),
    ('r', 'rev', [], _('include the specified changeset'), _('REV')),
    ('b', 'branch', [], _('clone only the specified branch'), _('BRANCH')),
    ('', 'pull', None, _('use pull protocol to copy metadata')),
    ('', 'uncompressed', None, _('use uncompressed transfer (fast over LAN)')),
    ] + remoteopts,
    _('[OPTION]... SOURCE [DEST]'),
    norepo=True)
def clone(ui, source, dest=None, **opts):
    """make a copy of an existing repository

    Create a copy of an existing repository in a new directory.

    If no destination directory name is specified, it defaults to the
    basename of the source.

    The location of the source is added to the new repository's
    ``.hg/hgrc`` file, as the default to be used for future pulls.

    Only local paths and ``ssh://`` URLs are supported as
    destinations. For ``ssh://`` destinations, no working directory or
    ``.hg/hgrc`` will be created on the remote side.

    To pull only a subset of changesets, specify one or more revisions
    identifiers with -r/--rev or branches with -b/--branch. The
    resulting clone will contain only the specified changesets and
    their ancestors. These options (or 'clone src#rev dest') imply
    --pull, even for local source repositories. Note that specifying a
    tag will include the tagged changeset but not the changeset
    containing the tag.

    If the source repository has a bookmark called '@' set, that
    revision will be checked out in the new repository by default.

    To check out a particular version, use -u/--update, or
    -U/--noupdate to create a clone with no working directory.

    .. container:: verbose

      For efficiency, hardlinks are used for cloning whenever the
      source and destination are on the same filesystem (note this
      applies only to the repository data, not to the working
      directory). Some filesystems, such as AFS, implement hardlinking
      incorrectly, but do not report errors. In these cases, use the
      --pull option to avoid hardlinking.

      In some cases, you can clone repositories and the working
      directory using full hardlinks with ::

        $ cp -al REPO REPOCLONE

      This is the fastest way to clone, but it is not always safe. The
      operation is not atomic (making sure REPO is not modified during
      the operation is up to you) and you have to make sure your
      editor breaks hardlinks (Emacs and most Linux Kernel tools do
      so). Also, this is not compatible with certain extensions that
      place their metadata under the .hg directory, such as mq.

      Mercurial will update the working directory to the first applicable
      revision from this list:

      a) null if -U or the source repository has no changesets
      b) if -u . and the source repository is local, the first parent of
         the source repository's working directory
      c) the changeset specified with -u (if a branch name, this means the
         latest head of that branch)
      d) the changeset specified with -r
      e) the tipmost head specified with -b
      f) the tipmost head specified with the url#branch source syntax
      g) the revision marked with the '@' bookmark, if present
      h) the tipmost head of the default branch
      i) tip

      Examples:

      - clone a remote repository to a new directory named hg/::

          hg clone http://selenic.com/hg

      - create a lightweight local clone::

          hg clone project/ project-feature/

      - clone from an absolute path on an ssh server (note double-slash)::

          hg clone ssh://user@server//home/projects/alpha/

      - do a high-speed clone over a LAN while checking out a
        specified version::

          hg clone --uncompressed http://server/repo -u 1.5

      - create a repository without changesets after a particular revision::

          hg clone -r 04e544 experimental/ good/

      - clone (and track) a particular named branch::

          hg clone http://selenic.com/hg#stable

    See :hg:`help urls` for details on specifying URLs.

    Returns 0 on success.
    """
    if opts.get('noupdate') and opts.get('updaterev'):
        raise util.Abort(_("cannot specify both --noupdate and --updaterev"))

    r = hg.clone(ui, opts, source, dest,
                 pull=opts.get('pull'),
                 stream=opts.get('uncompressed'),
                 rev=opts.get('rev'),
                 update=opts.get('updaterev') or not opts.get('noupdate'),
                 branch=opts.get('branch'))

    return r is None

@command('^commit|ci',
    [('A', 'addremove', None,
     _('mark new/missing files as added/removed before committing')),
    ('', 'close-branch', None,
     _('mark a branch head as closed')),
    ('', 'amend', None, _('amend the parent of the working directory')),
    ('s', 'secret', None, _('use the secret phase for committing')),
    ('e', 'edit', None, _('invoke editor on commit messages')),
    ('i', 'interactive', None, _('use interactive mode')),
    ] + walkopts + commitopts + commitopts2 + subrepoopts,
    _('[OPTION]... [FILE]...'),
    inferrepo=True)
def commit(ui, repo, *pats, **opts):
    """commit the specified files or all outstanding changes

    Commit changes to the given files into the repository. Unlike a
    centralized SCM, this operation is a local operation. See
    :hg:`push` for a way to actively distribute your changes.

    If a list of files is omitted, all changes reported by :hg:`status`
    will be committed.

    If you are committing the result of a merge, do not provide any
    filenames or -I/-X filters.

    If no commit message is specified, Mercurial starts your
    configured editor where you can enter a message. In case your
    commit fails, you will find a backup of your message in
    ``.hg/last-message.txt``.

    The --close-branch flag can be used to mark the current branch
    head closed. When all heads of a branch are closed, the branch
    will be considered closed and no longer listed.

    The --amend flag can be used to amend the parent of the
    working directory with a new commit that contains the changes
    in the parent in addition to those currently reported by :hg:`status`,
    if there are any. The old commit is stored in a backup bundle in
    ``.hg/strip-backup`` (see :hg:`help bundle` and :hg:`help unbundle`
    on how to restore it).

    Message, user and date are taken from the amended commit unless
    specified. When a message isn't specified on the command line,
    the editor will open with the message of the amended commit.

    It is not possible to amend public changesets (see :hg:`help phases`)
    or changesets that have children.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    Returns 0 on success, 1 if nothing changed.
    """
    if opts.get('interactive'):
        opts.pop('interactive')
        cmdutil.dorecord(ui, repo, commit, 'commit', False,
                        cmdutil.recordfilter, *pats, **opts)
        return

    if opts.get('subrepos'):
        if opts.get('amend'):
            raise util.Abort(_('cannot amend with --subrepos'))
        # Let --subrepos on the command line override config setting.
        ui.setconfig('ui', 'commitsubrepos', True, 'commit')

    cmdutil.checkunfinished(repo, commit=True)

    branch = repo[None].branch()
    bheads = repo.branchheads(branch)

    extra = {}
    if opts.get('close_branch'):
        extra['close'] = 1

        if not bheads:
            raise util.Abort(_('can only close branch heads'))
        elif opts.get('amend'):
            if repo.parents()[0].p1().branch() != branch and \
                    repo.parents()[0].p2().branch() != branch:
                raise util.Abort(_('can only close branch heads'))

    if opts.get('amend'):
        if ui.configbool('ui', 'commitsubrepos'):
            raise util.Abort(_('cannot amend with ui.commitsubrepos enabled'))

        old = repo['.']
        if not old.mutable():
            raise util.Abort(_('cannot amend public changesets'))
        if len(repo[None].parents()) > 1:
            raise util.Abort(_('cannot amend while merging'))
        allowunstable = obsolete.isenabled(repo, obsolete.allowunstableopt)
        if not allowunstable and old.children():
            raise util.Abort(_('cannot amend changeset with children'))

        # commitfunc is used only for temporary amend commit by cmdutil.amend
        def commitfunc(ui, repo, message, match, opts):
            return repo.commit(message,
                               opts.get('user') or old.user(),
                               opts.get('date') or old.date(),
                               match,
                               extra=extra)

        active = repo._activebookmark
        marks = old.bookmarks()
        node = cmdutil.amend(ui, repo, commitfunc, old, extra, pats, opts)
        if node == old.node():
            ui.status(_("nothing changed\n"))
            return 1
        elif marks:
            ui.debug('moving bookmarks %r from %s to %s\n' %
                     (marks, old.hex(), hex(node)))
            newmarks = repo._bookmarks
            for bm in marks:
                newmarks[bm] = node
                if bm == active:
                    bookmarks.activate(repo, bm)
            newmarks.write()
    else:
        def commitfunc(ui, repo, message, match, opts):
            backup = ui.backupconfig('phases', 'new-commit')
            baseui = repo.baseui
            basebackup = baseui.backupconfig('phases', 'new-commit')
            try:
                if opts.get('secret'):
                    ui.setconfig('phases', 'new-commit', 'secret', 'commit')
                    # Propagate to subrepos
                    baseui.setconfig('phases', 'new-commit', 'secret', 'commit')

                editform = cmdutil.mergeeditform(repo[None], 'commit.normal')
                editor = cmdutil.getcommiteditor(editform=editform, **opts)
                return repo.commit(message, opts.get('user'), opts.get('date'),
                                   match,
                                   editor=editor,
                                   extra=extra)
            finally:
                ui.restoreconfig(backup)
                repo.baseui.restoreconfig(basebackup)


        node = cmdutil.commit(ui, repo, commitfunc, pats, opts)

        if not node:
            stat = repo.status(match=scmutil.match(repo[None], pats, opts))
            if stat[3]:
                ui.status(_("nothing changed (%d missing files, see "
                            "'hg status')\n") % len(stat[3]))
            else:
                ui.status(_("nothing changed\n"))
            return 1

    cmdutil.commitstatus(repo, node, branch, bheads, opts)

@command('config|showconfig|debugconfig',
    [('u', 'untrusted', None, _('show untrusted configuration options')),
     ('e', 'edit', None, _('edit user config')),
     ('l', 'local', None, _('edit repository config')),
     ('g', 'global', None, _('edit global config'))],
    _('[-u] [NAME]...'),
    optionalrepo=True)
def config(ui, repo, *values, **opts):
    """show combined config settings from all hgrc files

    With no arguments, print names and values of all config items.

    With one argument of the form section.name, print just the value
    of that config item.

    With multiple arguments, print names and values of all config
    items with matching section names.

    With --edit, start an editor on the user-level config file. With
    --global, edit the system-wide config file. With --local, edit the
    repository-level config file.

    With --debug, the source (filename and line number) is printed
    for each config item.

    See :hg:`help config` for more information about config files.

    Returns 0 on success, 1 if NAME does not exist.

    """

    if opts.get('edit') or opts.get('local') or opts.get('global'):
        if opts.get('local') and opts.get('global'):
            raise util.Abort(_("can't use --local and --global together"))

        if opts.get('local'):
            if not repo:
                raise util.Abort(_("can't use --local outside a repository"))
            paths = [repo.join('hgrc')]
        elif opts.get('global'):
            paths = scmutil.systemrcpath()
        else:
            paths = scmutil.userrcpath()

        for f in paths:
            if os.path.exists(f):
                break
        else:
            if opts.get('global'):
                samplehgrc = uimod.samplehgrcs['global']
            elif opts.get('local'):
                samplehgrc = uimod.samplehgrcs['local']
            else:
                samplehgrc = uimod.samplehgrcs['user']

            f = paths[0]
            fp = open(f, "w")
            fp.write(samplehgrc)
            fp.close()

        editor = ui.geteditor()
        ui.system("%s \"%s\"" % (editor, f),
                  onerr=util.Abort, errprefix=_("edit failed"))
        return

    for f in scmutil.rcpath():
        ui.debug('read config from: %s\n' % f)
    untrusted = bool(opts.get('untrusted'))
    if values:
        sections = [v for v in values if '.' not in v]
        items = [v for v in values if '.' in v]
        if len(items) > 1 or items and sections:
            raise util.Abort(_('only one config item permitted'))
    matched = False
    for section, name, value in ui.walkconfig(untrusted=untrusted):
        value = str(value).replace('\n', '\\n')
        sectname = section + '.' + name
        if values:
            for v in values:
                if v == section:
                    ui.debug('%s: ' %
                             ui.configsource(section, name, untrusted))
                    ui.write('%s=%s\n' % (sectname, value))
                    matched = True
                elif v == sectname:
                    ui.debug('%s: ' %
                             ui.configsource(section, name, untrusted))
                    ui.write(value, '\n')
                    matched = True
        else:
            ui.debug('%s: ' %
                     ui.configsource(section, name, untrusted))
            ui.write('%s=%s\n' % (sectname, value))
            matched = True
    if matched:
        return 0
    return 1

@command('copy|cp',
    [('A', 'after', None, _('record a copy that has already occurred')),
    ('f', 'force', None, _('forcibly copy over an existing managed file')),
    ] + walkopts + dryrunopts,
    _('[OPTION]... [SOURCE]... DEST'))
def copy(ui, repo, *pats, **opts):
    """mark files as copied for the next commit

    Mark dest as having copies of source files. If dest is a
    directory, copies are put in that directory. If dest is a file,
    the source must be a single file.

    By default, this command copies the contents of files as they
    exist in the working directory. If invoked with -A/--after, the
    operation is recorded, but no copying is performed.

    This command takes effect with the next commit. To undo a copy
    before that, see :hg:`revert`.

    Returns 0 on success, 1 if errors are encountered.
    """
    wlock = repo.wlock(False)
    try:
        return cmdutil.copy(ui, repo, pats, opts)
    finally:
        wlock.release()

@command('debugancestor', [], _('[INDEX] REV1 REV2'), optionalrepo=True)
def debugancestor(ui, repo, *args):
    """find the ancestor revision of two revisions in a given index"""
    if len(args) == 3:
        index, rev1, rev2 = args
        r = revlog.revlog(scmutil.opener(os.getcwd(), audit=False), index)
        lookup = r.lookup
    elif len(args) == 2:
        if not repo:
            raise util.Abort(_("there is no Mercurial repository here "
                               "(.hg not found)"))
        rev1, rev2 = args
        r = repo.changelog
        lookup = repo.lookup
    else:
        raise util.Abort(_('either two or three arguments required'))
    a = r.ancestor(lookup(rev1), lookup(rev2))
    ui.write("%d:%s\n" % (r.rev(a), hex(a)))

@command('debugbuilddag',
    [('m', 'mergeable-file', None, _('add single file mergeable changes')),
    ('o', 'overwritten-file', None, _('add single file all revs overwrite')),
    ('n', 'new-file', None, _('add new file at each rev'))],
    _('[OPTION]... [TEXT]'))
def debugbuilddag(ui, repo, text=None,
                  mergeable_file=False,
                  overwritten_file=False,
                  new_file=False):
    """builds a repo with a given DAG from scratch in the current empty repo

    The description of the DAG is read from stdin if not given on the
    command line.

    Elements:

     - "+n" is a linear run of n nodes based on the current default parent
     - "." is a single node based on the current default parent
     - "$" resets the default parent to null (implied at the start);
           otherwise the default parent is always the last node created
     - "<p" sets the default parent to the backref p
     - "*p" is a fork at parent p, which is a backref
     - "*p1/p2" is a merge of parents p1 and p2, which are backrefs
     - "/p2" is a merge of the preceding node and p2
     - ":tag" defines a local tag for the preceding node
     - "@branch" sets the named branch for subsequent nodes
     - "#...\\n" is a comment up to the end of the line

    Whitespace between the above elements is ignored.

    A backref is either

     - a number n, which references the node curr-n, where curr is the current
       node, or
     - the name of a local tag you placed earlier using ":tag", or
     - empty to denote the default parent.

    All string valued-elements are either strictly alphanumeric, or must
    be enclosed in double quotes ("..."), with "\\" as escape character.
    """

    if text is None:
        ui.status(_("reading DAG from stdin\n"))
        text = ui.fin.read()

    cl = repo.changelog
    if len(cl) > 0:
        raise util.Abort(_('repository is not empty'))

    # determine number of revs in DAG
    total = 0
    for type, data in dagparser.parsedag(text):
        if type == 'n':
            total += 1

    if mergeable_file:
        linesperrev = 2
        # make a file with k lines per rev
        initialmergedlines = [str(i) for i in xrange(0, total * linesperrev)]
        initialmergedlines.append("")

    tags = []

    lock = tr = None
    try:
        lock = repo.lock()
        tr = repo.transaction("builddag")

        at = -1
        atbranch = 'default'
        nodeids = []
        id = 0
        ui.progress(_('building'), id, unit=_('revisions'), total=total)
        for type, data in dagparser.parsedag(text):
            if type == 'n':
                ui.note(('node %s\n' % str(data)))
                id, ps = data

                files = []
                fctxs = {}

                p2 = None
                if mergeable_file:
                    fn = "mf"
                    p1 = repo[ps[0]]
                    if len(ps) > 1:
                        p2 = repo[ps[1]]
                        pa = p1.ancestor(p2)
                        base, local, other = [x[fn].data() for x in (pa, p1,
                                                                     p2)]
                        m3 = simplemerge.Merge3Text(base, local, other)
                        ml = [l.strip() for l in m3.merge_lines()]
                        ml.append("")
                    elif at > 0:
                        ml = p1[fn].data().split("\n")
                    else:
                        ml = initialmergedlines
                    ml[id * linesperrev] += " r%i" % id
                    mergedtext = "\n".join(ml)
                    files.append(fn)
                    fctxs[fn] = context.memfilectx(repo, fn, mergedtext)

                if overwritten_file:
                    fn = "of"
                    files.append(fn)
                    fctxs[fn] = context.memfilectx(repo, fn, "r%i\n" % id)

                if new_file:
                    fn = "nf%i" % id
                    files.append(fn)
                    fctxs[fn] = context.memfilectx(repo, fn, "r%i\n" % id)
                    if len(ps) > 1:
                        if not p2:
                            p2 = repo[ps[1]]
                        for fn in p2:
                            if fn.startswith("nf"):
                                files.append(fn)
                                fctxs[fn] = p2[fn]

                def fctxfn(repo, cx, path):
                    return fctxs.get(path)

                if len(ps) == 0 or ps[0] < 0:
                    pars = [None, None]
                elif len(ps) == 1:
                    pars = [nodeids[ps[0]], None]
                else:
                    pars = [nodeids[p] for p in ps]
                cx = context.memctx(repo, pars, "r%i" % id, files, fctxfn,
                                    date=(id, 0),
                                    user="debugbuilddag",
                                    extra={'branch': atbranch})
                nodeid = repo.commitctx(cx)
                nodeids.append(nodeid)
                at = id
            elif type == 'l':
                id, name = data
                ui.note(('tag %s\n' % name))
                tags.append("%s %s\n" % (hex(repo.changelog.node(id)), name))
            elif type == 'a':
                ui.note(('branch %s\n' % data))
                atbranch = data
            ui.progress(_('building'), id, unit=_('revisions'), total=total)
        tr.close()

        if tags:
            repo.vfs.write("localtags", "".join(tags))
    finally:
        ui.progress(_('building'), None)
        release(tr, lock)

@command('debugbundle',
        [('a', 'all', None, _('show all details'))],
        _('FILE'),
        norepo=True)
def debugbundle(ui, bundlepath, all=None, **opts):
    """lists the contents of a bundle"""
    f = hg.openpath(ui, bundlepath)
    try:
        gen = exchange.readbundle(ui, f, bundlepath)
        if isinstance(gen, bundle2.unbundle20):
            return _debugbundle2(ui, gen, all=all, **opts)
        if all:
            ui.write(("format: id, p1, p2, cset, delta base, len(delta)\n"))

            def showchunks(named):
                ui.write("\n%s\n" % named)
                chain = None
                while True:
                    chunkdata = gen.deltachunk(chain)
                    if not chunkdata:
                        break
                    node = chunkdata['node']
                    p1 = chunkdata['p1']
                    p2 = chunkdata['p2']
                    cs = chunkdata['cs']
                    deltabase = chunkdata['deltabase']
                    delta = chunkdata['delta']
                    ui.write("%s %s %s %s %s %s\n" %
                             (hex(node), hex(p1), hex(p2),
                              hex(cs), hex(deltabase), len(delta)))
                    chain = node

            chunkdata = gen.changelogheader()
            showchunks("changelog")
            chunkdata = gen.manifestheader()
            showchunks("manifest")
            while True:
                chunkdata = gen.filelogheader()
                if not chunkdata:
                    break
                fname = chunkdata['filename']
                showchunks(fname)
        else:
            if isinstance(gen, bundle2.unbundle20):
                raise util.Abort(_('use debugbundle2 for this file'))
            chunkdata = gen.changelogheader()
            chain = None
            while True:
                chunkdata = gen.deltachunk(chain)
                if not chunkdata:
                    break
                node = chunkdata['node']
                ui.write("%s\n" % hex(node))
                chain = node
    finally:
        f.close()

def _debugbundle2(ui, gen, **opts):
    """lists the contents of a bundle2"""
    if not isinstance(gen, bundle2.unbundle20):
        raise util.Abort(_('not a bundle2 file'))
    ui.write(('Stream params: %s\n' % repr(gen.params)))
    for part in gen.iterparts():
        ui.write('%s -- %r\n' % (part.type, repr(part.params)))
        if part.type == 'changegroup':
            version = part.params.get('version', '01')
            cg = changegroup.packermap[version][1](part, 'UN')
            chunkdata = cg.changelogheader()
            chain = None
            while True:
                chunkdata = cg.deltachunk(chain)
                if not chunkdata:
                    break
                node = chunkdata['node']
                ui.write("    %s\n" % hex(node))
                chain = node

@command('debugcheckstate', [], '')
def debugcheckstate(ui, repo):
    """validate the correctness of the current dirstate"""
    parent1, parent2 = repo.dirstate.parents()
    m1 = repo[parent1].manifest()
    m2 = repo[parent2].manifest()
    errors = 0
    for f in repo.dirstate:
        state = repo.dirstate[f]
        if state in "nr" and f not in m1:
            ui.warn(_("%s in state %s, but not in manifest1\n") % (f, state))
            errors += 1
        if state in "a" and f in m1:
            ui.warn(_("%s in state %s, but also in manifest1\n") % (f, state))
            errors += 1
        if state in "m" and f not in m1 and f not in m2:
            ui.warn(_("%s in state %s, but not in either manifest\n") %
                    (f, state))
            errors += 1
    for f in m1:
        state = repo.dirstate[f]
        if state not in "nrm":
            ui.warn(_("%s in manifest1, but listed as state %s") % (f, state))
            errors += 1
    if errors:
        error = _(".hg/dirstate inconsistent with current parent's manifest")
        raise util.Abort(error)

@command('debugcommands', [], _('[COMMAND]'), norepo=True)
def debugcommands(ui, cmd='', *args):
    """list all available commands and options"""
    for cmd, vals in sorted(table.iteritems()):
        cmd = cmd.split('|')[0].strip('^')
        opts = ', '.join([i[1] for i in vals[1]])
        ui.write('%s: %s\n' % (cmd, opts))

@command('debugcomplete',
    [('o', 'options', None, _('show the command options'))],
    _('[-o] CMD'),
    norepo=True)
def debugcomplete(ui, cmd='', **opts):
    """returns the completion list associated with the given command"""

    if opts.get('options'):
        options = []
        otables = [globalopts]
        if cmd:
            aliases, entry = cmdutil.findcmd(cmd, table, False)
            otables.append(entry[1])
        for t in otables:
            for o in t:
                if "(DEPRECATED)" in o[3]:
                    continue
                if o[0]:
                    options.append('-%s' % o[0])
                options.append('--%s' % o[1])
        ui.write("%s\n" % "\n".join(options))
        return

    cmdlist, unused_allcmds = cmdutil.findpossible(cmd, table)
    if ui.verbose:
        cmdlist = [' '.join(c[0]) for c in cmdlist.values()]
    ui.write("%s\n" % "\n".join(sorted(cmdlist)))

@command('debugdag',
    [('t', 'tags', None, _('use tags as labels')),
    ('b', 'branches', None, _('annotate with branch names')),
    ('', 'dots', None, _('use dots for runs')),
    ('s', 'spaces', None, _('separate elements by spaces'))],
    _('[OPTION]... [FILE [REV]...]'),
    optionalrepo=True)
def debugdag(ui, repo, file_=None, *revs, **opts):
    """format the changelog or an index DAG as a concise textual description

    If you pass a revlog index, the revlog's DAG is emitted. If you list
    revision numbers, they get labeled in the output as rN.

    Otherwise, the changelog DAG of the current repo is emitted.
    """
    spaces = opts.get('spaces')
    dots = opts.get('dots')
    if file_:
        rlog = revlog.revlog(scmutil.opener(os.getcwd(), audit=False), file_)
        revs = set((int(r) for r in revs))
        def events():
            for r in rlog:
                yield 'n', (r, list(p for p in rlog.parentrevs(r)
                                        if p != -1))
                if r in revs:
                    yield 'l', (r, "r%i" % r)
    elif repo:
        cl = repo.changelog
        tags = opts.get('tags')
        branches = opts.get('branches')
        if tags:
            labels = {}
            for l, n in repo.tags().items():
                labels.setdefault(cl.rev(n), []).append(l)
        def events():
            b = "default"
            for r in cl:
                if branches:
                    newb = cl.read(cl.node(r))[5]['branch']
                    if newb != b:
                        yield 'a', newb
                        b = newb
                yield 'n', (r, list(p for p in cl.parentrevs(r)
                                        if p != -1))
                if tags:
                    ls = labels.get(r)
                    if ls:
                        for l in ls:
                            yield 'l', (r, l)
    else:
        raise util.Abort(_('need repo for changelog dag'))

    for line in dagparser.dagtextlines(events(),
                                       addspaces=spaces,
                                       wraplabels=True,
                                       wrapannotations=True,
                                       wrapnonlinear=dots,
                                       usedots=dots,
                                       maxlinewidth=70):
        ui.write(line)
        ui.write("\n")

@command('debugdata',
    [('c', 'changelog', False, _('open changelog')),
     ('m', 'manifest', False, _('open manifest')),
     ('', 'dir', False, _('open directory manifest'))],
    _('-c|-m|FILE REV'))
def debugdata(ui, repo, file_, rev=None, **opts):
    """dump the contents of a data file revision"""
    if opts.get('changelog') or opts.get('manifest'):
        file_, rev = None, file_
    elif rev is None:
        raise error.CommandError('debugdata', _('invalid arguments'))
    r = cmdutil.openrevlog(repo, 'debugdata', file_, opts)
    try:
        ui.write(r.revision(r.lookup(rev)))
    except KeyError:
        raise util.Abort(_('invalid revision identifier %s') % rev)

@command('debugdate',
    [('e', 'extended', None, _('try extended date formats'))],
    _('[-e] DATE [RANGE]'),
    norepo=True, optionalrepo=True)
def debugdate(ui, date, range=None, **opts):
    """parse and display a date"""
    if opts["extended"]:
        d = util.parsedate(date, util.extendeddateformats)
    else:
        d = util.parsedate(date)
    ui.write(("internal: %s %s\n") % d)
    ui.write(("standard: %s\n") % util.datestr(d))
    if range:
        m = util.matchdate(range)
        ui.write(("match: %s\n") % m(d[0]))

@command('debugdiscovery',
    [('', 'old', None, _('use old-style discovery')),
    ('', 'nonheads', None,
     _('use old-style discovery with non-heads included')),
    ] + remoteopts,
    _('[-l REV] [-r REV] [-b BRANCH]... [OTHER]'))
def debugdiscovery(ui, repo, remoteurl="default", **opts):
    """runs the changeset discovery protocol in isolation"""
    remoteurl, branches = hg.parseurl(ui.expandpath(remoteurl),
                                      opts.get('branch'))
    remote = hg.peer(repo, opts, remoteurl)
    ui.status(_('comparing with %s\n') % util.hidepassword(remoteurl))

    # make sure tests are repeatable
    random.seed(12323)

    def doit(localheads, remoteheads, remote=remote):
        if opts.get('old'):
            if localheads:
                raise util.Abort('cannot use localheads with old style '
                                 'discovery')
            if not util.safehasattr(remote, 'branches'):
                # enable in-client legacy support
                remote = localrepo.locallegacypeer(remote.local())
            common, _in, hds = treediscovery.findcommonincoming(repo, remote,
                                                                force=True)
            common = set(common)
            if not opts.get('nonheads'):
                ui.write(("unpruned common: %s\n") %
                         " ".join(sorted(short(n) for n in common)))
                dag = dagutil.revlogdag(repo.changelog)
                all = dag.ancestorset(dag.internalizeall(common))
                common = dag.externalizeall(dag.headsetofconnecteds(all))
        else:
            common, any, hds = setdiscovery.findcommonheads(ui, repo, remote)
        common = set(common)
        rheads = set(hds)
        lheads = set(repo.heads())
        ui.write(("common heads: %s\n") %
                 " ".join(sorted(short(n) for n in common)))
        if lheads <= common:
            ui.write(("local is subset\n"))
        elif rheads <= common:
            ui.write(("remote is subset\n"))

    serverlogs = opts.get('serverlog')
    if serverlogs:
        for filename in serverlogs:
            logfile = open(filename, 'r')
            try:
                line = logfile.readline()
                while line:
                    parts = line.strip().split(';')
                    op = parts[1]
                    if op == 'cg':
                        pass
                    elif op == 'cgss':
                        doit(parts[2].split(' '), parts[3].split(' '))
                    elif op == 'unb':
                        doit(parts[3].split(' '), parts[2].split(' '))
                    line = logfile.readline()
            finally:
                logfile.close()

    else:
        remoterevs, _checkout = hg.addbranchrevs(repo, remote, branches,
                                                 opts.get('remote_head'))
        localrevs = opts.get('local_head')
        doit(localrevs, remoterevs)

@command('debugfileset',
    [('r', 'rev', '', _('apply the filespec on this revision'), _('REV'))],
    _('[-r REV] FILESPEC'))
def debugfileset(ui, repo, expr, **opts):
    '''parse and apply a fileset specification'''
    ctx = scmutil.revsingle(repo, opts.get('rev'), None)
    if ui.verbose:
        tree = fileset.parse(expr)
        ui.note(fileset.prettyformat(tree), "\n")

    for f in ctx.getfileset(expr):
        ui.write("%s\n" % f)

@command('debugfsinfo', [], _('[PATH]'), norepo=True)
def debugfsinfo(ui, path="."):
    """show information detected about current filesystem"""
    util.writefile('.debugfsinfo', '')
    ui.write(('exec: %s\n') % (util.checkexec(path) and 'yes' or 'no'))
    ui.write(('symlink: %s\n') % (util.checklink(path) and 'yes' or 'no'))
    ui.write(('hardlink: %s\n') % (util.checknlink(path) and 'yes' or 'no'))
    ui.write(('case-sensitive: %s\n') % (util.checkcase('.debugfsinfo')
                                and 'yes' or 'no'))
    os.unlink('.debugfsinfo')

@command('debuggetbundle',
    [('H', 'head', [], _('id of head node'), _('ID')),
    ('C', 'common', [], _('id of common node'), _('ID')),
    ('t', 'type', 'bzip2', _('bundle compression type to use'), _('TYPE'))],
    _('REPO FILE [-H|-C ID]...'),
    norepo=True)
def debuggetbundle(ui, repopath, bundlepath, head=None, common=None, **opts):
    """retrieves a bundle from a repo

    Every ID must be a full-length hex node id string. Saves the bundle to the
    given file.
    """
    repo = hg.peer(ui, opts, repopath)
    if not repo.capable('getbundle'):
        raise util.Abort("getbundle() not supported by target repository")
    args = {}
    if common:
        args['common'] = [bin(s) for s in common]
    if head:
        args['heads'] = [bin(s) for s in head]
    # TODO: get desired bundlecaps from command line.
    args['bundlecaps'] = None
    bundle = repo.getbundle('debug', **args)

    bundletype = opts.get('type', 'bzip2').lower()
    btypes = {'none': 'HG10UN',
              'bzip2': 'HG10BZ',
              'gzip': 'HG10GZ',
              'bundle2': 'HG20'}
    bundletype = btypes.get(bundletype)
    if bundletype not in changegroup.bundletypes:
        raise util.Abort(_('unknown bundle type specified with --type'))
    changegroup.writebundle(ui, bundle, bundlepath, bundletype)

@command('debugignore', [], '')
def debugignore(ui, repo, *values, **opts):
    """display the combined ignore pattern"""
    ignore = repo.dirstate._ignore
    includepat = getattr(ignore, 'includepat', None)
    if includepat is not None:
        ui.write("%s\n" % includepat)
    else:
        raise util.Abort(_("no ignore patterns found"))

@command('debugindex',
    [('c', 'changelog', False, _('open changelog')),
     ('m', 'manifest', False, _('open manifest')),
     ('', 'dir', False, _('open directory manifest')),
     ('f', 'format', 0, _('revlog format'), _('FORMAT'))],
    _('[-f FORMAT] -c|-m|FILE'),
    optionalrepo=True)
def debugindex(ui, repo, file_=None, **opts):
    """dump the contents of an index file"""
    r = cmdutil.openrevlog(repo, 'debugindex', file_, opts)
    format = opts.get('format', 0)
    if format not in (0, 1):
        raise util.Abort(_("unknown format %d") % format)

    generaldelta = r.version & revlog.REVLOGGENERALDELTA
    if generaldelta:
        basehdr = ' delta'
    else:
        basehdr = '  base'

    if ui.debugflag:
        shortfn = hex
    else:
        shortfn = short

    # There might not be anything in r, so have a sane default
    idlen = 12
    for i in r:
        idlen = len(shortfn(r.node(i)))
        break

    if format == 0:
        ui.write("   rev    offset  length " + basehdr + " linkrev"
                 " %s %s p2\n" % ("nodeid".ljust(idlen), "p1".ljust(idlen)))
    elif format == 1:
        ui.write("   rev flag   offset   length"
                 "     size " + basehdr + "   link     p1     p2"
                 " %s\n" % "nodeid".rjust(idlen))

    for i in r:
        node = r.node(i)
        if generaldelta:
            base = r.deltaparent(i)
        else:
            base = r.chainbase(i)
        if format == 0:
            try:
                pp = r.parents(node)
            except Exception:
                pp = [nullid, nullid]
            ui.write("% 6d % 9d % 7d % 6d % 7d %s %s %s\n" % (
                    i, r.start(i), r.length(i), base, r.linkrev(i),
                    shortfn(node), shortfn(pp[0]), shortfn(pp[1])))
        elif format == 1:
            pr = r.parentrevs(i)
            ui.write("% 6d %04x % 8d % 8d % 8d % 6d % 6d % 6d % 6d %s\n" % (
                    i, r.flags(i), r.start(i), r.length(i), r.rawsize(i),
                    base, r.linkrev(i), pr[0], pr[1], shortfn(node)))

@command('debugindexdot', [], _('FILE'), optionalrepo=True)
def debugindexdot(ui, repo, file_):
    """dump an index DAG as a graphviz dot file"""
    r = None
    if repo:
        filelog = repo.file(file_)
        if len(filelog):
            r = filelog
    if not r:
        r = revlog.revlog(scmutil.opener(os.getcwd(), audit=False), file_)
    ui.write(("digraph G {\n"))
    for i in r:
        node = r.node(i)
        pp = r.parents(node)
        ui.write("\t%d -> %d\n" % (r.rev(pp[0]), i))
        if pp[1] != nullid:
            ui.write("\t%d -> %d\n" % (r.rev(pp[1]), i))
    ui.write("}\n")

@command('debuginstall', [], '', norepo=True)
def debuginstall(ui):
    '''test Mercurial installation

    Returns 0 on success.
    '''

    def writetemp(contents):
        (fd, name) = tempfile.mkstemp(prefix="hg-debuginstall-")
        f = os.fdopen(fd, "wb")
        f.write(contents)
        f.close()
        return name

    problems = 0

    # encoding
    ui.status(_("checking encoding (%s)...\n") % encoding.encoding)
    try:
        encoding.fromlocal("test")
    except util.Abort as inst:
        ui.write(" %s\n" % inst)
        ui.write(_(" (check that your locale is properly set)\n"))
        problems += 1

    # Python
    ui.status(_("checking Python executable (%s)\n") % sys.executable)
    ui.status(_("checking Python version (%s)\n")
              % ("%s.%s.%s" % sys.version_info[:3]))
    ui.status(_("checking Python lib (%s)...\n")
              % os.path.dirname(os.__file__))

    # compiled modules
    ui.status(_("checking installed modules (%s)...\n")
              % os.path.dirname(__file__))
    try:
        import bdiff, mpatch, base85, osutil
        dir(bdiff), dir(mpatch), dir(base85), dir(osutil) # quiet pyflakes
    except Exception as inst:
        ui.write(" %s\n" % inst)
        ui.write(_(" One or more extensions could not be found"))
        ui.write(_(" (check that you compiled the extensions)\n"))
        problems += 1

    # templates
    import templater
    p = templater.templatepaths()
    ui.status(_("checking templates (%s)...\n") % ' '.join(p))
    if p:
        m = templater.templatepath("map-cmdline.default")
        if m:
            # template found, check if it is working
            try:
                templater.templater(m)
            except Exception as inst:
                ui.write(" %s\n" % inst)
                p = None
        else:
            ui.write(_(" template 'default' not found\n"))
            p = None
    else:
        ui.write(_(" no template directories found\n"))
    if not p:
        ui.write(_(" (templates seem to have been installed incorrectly)\n"))
        problems += 1

    # editor
    ui.status(_("checking commit editor...\n"))
    editor = ui.geteditor()
    editor = util.expandpath(editor)
    cmdpath = util.findexe(shlex.split(editor)[0])
    if not cmdpath:
        if editor == 'vi':
            ui.write(_(" No commit editor set and can't find vi in PATH\n"))
            ui.write(_(" (specify a commit editor in your configuration"
                       " file)\n"))
        else:
            ui.write(_(" Can't find editor '%s' in PATH\n") % editor)
            ui.write(_(" (specify a commit editor in your configuration"
                       " file)\n"))
            problems += 1

    # check username
    ui.status(_("checking username...\n"))
    try:
        ui.username()
    except util.Abort as e:
        ui.write(" %s\n" % e)
        ui.write(_(" (specify a username in your configuration file)\n"))
        problems += 1

    if not problems:
        ui.status(_("no problems detected\n"))
    else:
        ui.write(_("%s problems detected,"
                   " please check your install!\n") % problems)

    return problems

@command('debugknown', [], _('REPO ID...'), norepo=True)
def debugknown(ui, repopath, *ids, **opts):
    """test whether node ids are known to a repo

    Every ID must be a full-length hex node id string. Returns a list of 0s
    and 1s indicating unknown/known.
    """
    repo = hg.peer(ui, opts, repopath)
    if not repo.capable('known'):
        raise util.Abort("known() not supported by target repository")
    flags = repo.known([bin(s) for s in ids])
    ui.write("%s\n" % ("".join([f and "1" or "0" for f in flags])))

@command('debuglabelcomplete', [], _('LABEL...'))
def debuglabelcomplete(ui, repo, *args):
    '''backwards compatibility with old bash completion scripts (DEPRECATED)'''
    debugnamecomplete(ui, repo, *args)

@command('debugnamecomplete', [], _('NAME...'))
def debugnamecomplete(ui, repo, *args):
    '''complete "names" - tags, open branch names, bookmark names'''

    names = set()
    # since we previously only listed open branches, we will handle that
    # specially (after this for loop)
    for name, ns in repo.names.iteritems():
        if name != 'branches':
            names.update(ns.listnames(repo))
    names.update(tag for (tag, heads, tip, closed)
                 in repo.branchmap().iterbranches() if not closed)
    completions = set()
    if not args:
        args = ['']
    for a in args:
        completions.update(n for n in names if n.startswith(a))
    ui.write('\n'.join(sorted(completions)))
    ui.write('\n')

@command('debuglocks',
         [('L', 'force-lock', None, _('free the store lock (DANGEROUS)')),
          ('W', 'force-wlock', None,
           _('free the working state lock (DANGEROUS)'))],
         _('[OPTION]...'))
def debuglocks(ui, repo, **opts):
    """show or modify state of locks

    By default, this command will show which locks are held. This
    includes the user and process holding the lock, the amount of time
    the lock has been held, and the machine name where the process is
    running if it's not local.

    Locks protect the integrity of Mercurial's data, so should be
    treated with care. System crashes or other interruptions may cause
    locks to not be properly released, though Mercurial will usually
    detect and remove such stale locks automatically.

    However, detecting stale locks may not always be possible (for
    instance, on a shared filesystem). Removing locks may also be
    blocked by filesystem permissions.

    Returns 0 if no locks are held.

    """

    if opts.get('force_lock'):
        repo.svfs.unlink('lock')
    if opts.get('force_wlock'):
        repo.vfs.unlink('wlock')
    if opts.get('force_lock') or opts.get('force_lock'):
        return 0

    now = time.time()
    held = 0

    def report(vfs, name, method):
        # this causes stale locks to get reaped for more accurate reporting
        try:
            l = method(False)
        except error.LockHeld:
            l = None

        if l:
            l.release()
        else:
            try:
                stat = vfs.lstat(name)
                age = now - stat.st_mtime
                user = util.username(stat.st_uid)
                locker = vfs.readlock(name)
                if ":" in locker:
                    host, pid = locker.split(':')
                    if host == socket.gethostname():
                        locker = 'user %s, process %s' % (user, pid)
                    else:
                        locker = 'user %s, process %s, host %s' \
                                 % (user, pid, host)
                ui.write("%-6s %s (%ds)\n" % (name + ":", locker, age))
                return 1
            except OSError as e:
                if e.errno != errno.ENOENT:
                    raise

        ui.write("%-6s free\n" % (name + ":"))
        return 0

    held += report(repo.svfs, "lock", repo.lock)
    held += report(repo.vfs, "wlock", repo.wlock)

    return held

@command('debugobsolete',
        [('', 'flags', 0, _('markers flag')),
         ('', 'record-parents', False,
          _('record parent information for the precursor')),
         ('r', 'rev', [], _('display markers relevant to REV')),
        ] + commitopts2,
         _('[OBSOLETED [REPLACEMENT] [REPL... ]'))
def debugobsolete(ui, repo, precursor=None, *successors, **opts):
    """create arbitrary obsolete marker

    With no arguments, displays the list of obsolescence markers."""

    def parsenodeid(s):
        try:
            # We do not use revsingle/revrange functions here to accept
            # arbitrary node identifiers, possibly not present in the
            # local repository.
            n = bin(s)
            if len(n) != len(nullid):
                raise TypeError()
            return n
        except TypeError:
            raise util.Abort('changeset references must be full hexadecimal '
                             'node identifiers')

    if precursor is not None:
        if opts['rev']:
            raise util.Abort('cannot select revision when creating marker')
        metadata = {}
        metadata['user'] = opts['user'] or ui.username()
        succs = tuple(parsenodeid(succ) for succ in successors)
        l = repo.lock()
        try:
            tr = repo.transaction('debugobsolete')
            try:
                date = opts.get('date')
                if date:
                    date = util.parsedate(date)
                else:
                    date = None
                prec = parsenodeid(precursor)
                parents = None
                if opts['record_parents']:
                    if prec not in repo.unfiltered():
                        raise util.Abort('cannot used --record-parents on '
                                         'unknown changesets')
                    parents = repo.unfiltered()[prec].parents()
                    parents = tuple(p.node() for p in parents)
                repo.obsstore.create(tr, prec, succs, opts['flags'],
                                     parents=parents, date=date,
                                     metadata=metadata)
                tr.close()
            except ValueError as exc:
                raise util.Abort(_('bad obsmarker input: %s') % exc)
            finally:
                tr.release()
        finally:
            l.release()
    else:
        if opts['rev']:
            revs = scmutil.revrange(repo, opts['rev'])
            nodes = [repo[r].node() for r in revs]
            markers = list(obsolete.getmarkers(repo, nodes=nodes))
            markers.sort(key=lambda x: x._data)
        else:
            markers = obsolete.getmarkers(repo)

        for m in markers:
            cmdutil.showmarker(ui, m)

@command('debugpathcomplete',
         [('f', 'full', None, _('complete an entire path')),
          ('n', 'normal', None, _('show only normal files')),
          ('a', 'added', None, _('show only added files')),
          ('r', 'removed', None, _('show only removed files'))],
         _('FILESPEC...'))
def debugpathcomplete(ui, repo, *specs, **opts):
    '''complete part or all of a tracked path

    This command supports shells that offer path name completion. It
    currently completes only files already known to the dirstate.

    Completion extends only to the next path segment unless
    --full is specified, in which case entire paths are used.'''

    def complete(path, acceptable):
        dirstate = repo.dirstate
        spec = os.path.normpath(os.path.join(os.getcwd(), path))
        rootdir = repo.root + os.sep
        if spec != repo.root and not spec.startswith(rootdir):
            return [], []
        if os.path.isdir(spec):
            spec += '/'
        spec = spec[len(rootdir):]
        fixpaths = os.sep != '/'
        if fixpaths:
            spec = spec.replace(os.sep, '/')
        speclen = len(spec)
        fullpaths = opts['full']
        files, dirs = set(), set()
        adddir, addfile = dirs.add, files.add
        for f, st in dirstate.iteritems():
            if f.startswith(spec) and st[0] in acceptable:
                if fixpaths:
                    f = f.replace('/', os.sep)
                if fullpaths:
                    addfile(f)
                    continue
                s = f.find(os.sep, speclen)
                if s >= 0:
                    adddir(f[:s])
                else:
                    addfile(f)
        return files, dirs

    acceptable = ''
    if opts['normal']:
        acceptable += 'nm'
    if opts['added']:
        acceptable += 'a'
    if opts['removed']:
        acceptable += 'r'
    cwd = repo.getcwd()
    if not specs:
        specs = ['.']

    files, dirs = set(), set()
    for spec in specs:
        f, d = complete(spec, acceptable or 'nmar')
        files.update(f)
        dirs.update(d)
    files.update(dirs)
    ui.write('\n'.join(repo.pathto(p, cwd) for p in sorted(files)))
    ui.write('\n')

@command('debugpushkey', [], _('REPO NAMESPACE [KEY OLD NEW]'), norepo=True)
def debugpushkey(ui, repopath, namespace, *keyinfo, **opts):
    '''access the pushkey key/value protocol

    With two args, list the keys in the given namespace.

    With five args, set a key to new if it currently is set to old.
    Reports success or failure.
    '''

    target = hg.peer(ui, {}, repopath)
    if keyinfo:
        key, old, new = keyinfo
        r = target.pushkey(namespace, key, old, new)
        ui.status(str(r) + '\n')
        return not r
    else:
        for k, v in sorted(target.listkeys(namespace).iteritems()):
            ui.write("%s\t%s\n" % (k.encode('string-escape'),
                                   v.encode('string-escape')))

@command('debugpvec', [], _('A B'))
def debugpvec(ui, repo, a, b=None):
    ca = scmutil.revsingle(repo, a)
    cb = scmutil.revsingle(repo, b)
    pa = pvec.ctxpvec(ca)
    pb = pvec.ctxpvec(cb)
    if pa == pb:
        rel = "="
    elif pa > pb:
        rel = ">"
    elif pa < pb:
        rel = "<"
    elif pa | pb:
        rel = "|"
    ui.write(_("a: %s\n") % pa)
    ui.write(_("b: %s\n") % pb)
    ui.write(_("depth(a): %d depth(b): %d\n") % (pa._depth, pb._depth))
    ui.write(_("delta: %d hdist: %d distance: %d relation: %s\n") %
             (abs(pa._depth - pb._depth), pvec._hamming(pa._vec, pb._vec),
              pa.distance(pb), rel))

@command('debugrebuilddirstate|debugrebuildstate',
    [('r', 'rev', '', _('revision to rebuild to'), _('REV'))],
    _('[-r REV]'))
def debugrebuilddirstate(ui, repo, rev):
    """rebuild the dirstate as it would look like for the given revision

    If no revision is specified the first current parent will be used.

    The dirstate will be set to the files of the given revision.
    The actual working directory content or existing dirstate
    information such as adds or removes is not considered.

    One use of this command is to make the next :hg:`status` invocation
    check the actual file content.
    """
    ctx = scmutil.revsingle(repo, rev)
    wlock = repo.wlock()
    try:
        repo.dirstate.rebuild(ctx.node(), ctx.manifest())
    finally:
        wlock.release()

@command('debugrebuildfncache', [], '')
def debugrebuildfncache(ui, repo):
    """rebuild the fncache file"""
    repair.rebuildfncache(ui, repo)

@command('debugrename',
    [('r', 'rev', '', _('revision to debug'), _('REV'))],
    _('[-r REV] FILE'))
def debugrename(ui, repo, file1, *pats, **opts):
    """dump rename information"""

    ctx = scmutil.revsingle(repo, opts.get('rev'))
    m = scmutil.match(ctx, (file1,) + pats, opts)
    for abs in ctx.walk(m):
        fctx = ctx[abs]
        o = fctx.filelog().renamed(fctx.filenode())
        rel = m.rel(abs)
        if o:
            ui.write(_("%s renamed from %s:%s\n") % (rel, o[0], hex(o[1])))
        else:
            ui.write(_("%s not renamed\n") % rel)

@command('debugrevlog',
    [('c', 'changelog', False, _('open changelog')),
     ('m', 'manifest', False, _('open manifest')),
     ('', 'dir', False, _('open directory manifest')),
     ('d', 'dump', False, _('dump index data'))],
    _('-c|-m|FILE'),
    optionalrepo=True)
def debugrevlog(ui, repo, file_=None, **opts):
    """show data and statistics about a revlog"""
    r = cmdutil.openrevlog(repo, 'debugrevlog', file_, opts)

    if opts.get("dump"):
        numrevs = len(r)
        ui.write("# rev p1rev p2rev start   end deltastart base   p1   p2"
                 " rawsize totalsize compression heads chainlen\n")
        ts = 0
        heads = set()

        for rev in xrange(numrevs):
            dbase = r.deltaparent(rev)
            if dbase == -1:
                dbase = rev
            cbase = r.chainbase(rev)
            clen = r.chainlen(rev)
            p1, p2 = r.parentrevs(rev)
            rs = r.rawsize(rev)
            ts = ts + rs
            heads -= set(r.parentrevs(rev))
            heads.add(rev)
            ui.write("%5d %5d %5d %5d %5d %10d %4d %4d %4d %7d %9d "
                     "%11d %5d %8d\n" %
                     (rev, p1, p2, r.start(rev), r.end(rev),
                      r.start(dbase), r.start(cbase),
                      r.start(p1), r.start(p2),
                      rs, ts, ts / r.end(rev), len(heads), clen))
        return 0

    v = r.version
    format = v & 0xFFFF
    flags = []
    gdelta = False
    if v & revlog.REVLOGNGINLINEDATA:
        flags.append('inline')
    if v & revlog.REVLOGGENERALDELTA:
        gdelta = True
        flags.append('generaldelta')
    if not flags:
        flags = ['(none)']

    nummerges = 0
    numfull = 0
    numprev = 0
    nump1 = 0
    nump2 = 0
    numother = 0
    nump1prev = 0
    nump2prev = 0
    chainlengths = []

    datasize = [None, 0, 0L]
    fullsize = [None, 0, 0L]
    deltasize = [None, 0, 0L]

    def addsize(size, l):
        if l[0] is None or size < l[0]:
            l[0] = size
        if size > l[1]:
            l[1] = size
        l[2] += size

    numrevs = len(r)
    for rev in xrange(numrevs):
        p1, p2 = r.parentrevs(rev)
        delta = r.deltaparent(rev)
        if format > 0:
            addsize(r.rawsize(rev), datasize)
        if p2 != nullrev:
            nummerges += 1
        size = r.length(rev)
        if delta == nullrev:
            chainlengths.append(0)
            numfull += 1
            addsize(size, fullsize)
        else:
            chainlengths.append(chainlengths[delta] + 1)
            addsize(size, deltasize)
            if delta == rev - 1:
                numprev += 1
                if delta == p1:
                    nump1prev += 1
                elif delta == p2:
                    nump2prev += 1
            elif delta == p1:
                nump1 += 1
            elif delta == p2:
                nump2 += 1
            elif delta != nullrev:
                numother += 1

    # Adjust size min value for empty cases
    for size in (datasize, fullsize, deltasize):
        if size[0] is None:
            size[0] = 0

    numdeltas = numrevs - numfull
    numoprev = numprev - nump1prev - nump2prev
    totalrawsize = datasize[2]
    datasize[2] /= numrevs
    fulltotal = fullsize[2]
    fullsize[2] /= numfull
    deltatotal = deltasize[2]
    if numrevs - numfull > 0:
        deltasize[2] /= numrevs - numfull
    totalsize = fulltotal + deltatotal
    avgchainlen = sum(chainlengths) / numrevs
    maxchainlen = max(chainlengths)
    compratio = totalrawsize / totalsize

    basedfmtstr = '%%%dd\n'
    basepcfmtstr = '%%%dd %s(%%5.2f%%%%)\n'

    def dfmtstr(max):
        return basedfmtstr % len(str(max))
    def pcfmtstr(max, padding=0):
        return basepcfmtstr % (len(str(max)), ' ' * padding)

    def pcfmt(value, total):
        return (value, 100 * float(value) / total)

    ui.write(('format : %d\n') % format)
    ui.write(('flags  : %s\n') % ', '.join(flags))

    ui.write('\n')
    fmt = pcfmtstr(totalsize)
    fmt2 = dfmtstr(totalsize)
    ui.write(('revisions     : ') + fmt2 % numrevs)
    ui.write(('    merges    : ') + fmt % pcfmt(nummerges, numrevs))
    ui.write(('    normal    : ') + fmt % pcfmt(numrevs - nummerges, numrevs))
    ui.write(('revisions     : ') + fmt2 % numrevs)
    ui.write(('    full      : ') + fmt % pcfmt(numfull, numrevs))
    ui.write(('    deltas    : ') + fmt % pcfmt(numdeltas, numrevs))
    ui.write(('revision size : ') + fmt2 % totalsize)
    ui.write(('    full      : ') + fmt % pcfmt(fulltotal, totalsize))
    ui.write(('    deltas    : ') + fmt % pcfmt(deltatotal, totalsize))

    ui.write('\n')
    fmt = dfmtstr(max(avgchainlen, compratio))
    ui.write(('avg chain length  : ') + fmt % avgchainlen)
    ui.write(('max chain length  : ') + fmt % maxchainlen)
    ui.write(('compression ratio : ') + fmt % compratio)

    if format > 0:
        ui.write('\n')
        ui.write(('uncompressed data size (min/max/avg) : %d / %d / %d\n')
                 % tuple(datasize))
    ui.write(('full revision size (min/max/avg)     : %d / %d / %d\n')
             % tuple(fullsize))
    ui.write(('delta size (min/max/avg)             : %d / %d / %d\n')
             % tuple(deltasize))

    if numdeltas > 0:
        ui.write('\n')
        fmt = pcfmtstr(numdeltas)
        fmt2 = pcfmtstr(numdeltas, 4)
        ui.write(('deltas against prev  : ') + fmt % pcfmt(numprev, numdeltas))
        if numprev > 0:
            ui.write(('    where prev = p1  : ') + fmt2 % pcfmt(nump1prev,
                                                              numprev))
            ui.write(('    where prev = p2  : ') + fmt2 % pcfmt(nump2prev,
                                                              numprev))
            ui.write(('    other            : ') + fmt2 % pcfmt(numoprev,
                                                              numprev))
        if gdelta:
            ui.write(('deltas against p1    : ')
                     + fmt % pcfmt(nump1, numdeltas))
            ui.write(('deltas against p2    : ')
                     + fmt % pcfmt(nump2, numdeltas))
            ui.write(('deltas against other : ') + fmt % pcfmt(numother,
                                                             numdeltas))

@command('debugrevspec',
    [('', 'optimize', None, _('print parsed tree after optimizing'))],
    ('REVSPEC'))
def debugrevspec(ui, repo, expr, **opts):
    """parse and apply a revision specification

    Use --verbose to print the parsed tree before and after aliases
    expansion.
    """
    if ui.verbose:
        tree = revset.parse(expr)
        ui.note(revset.prettyformat(tree), "\n")
        newtree = revset.findaliases(ui, tree)
        if newtree != tree:
            ui.note(revset.prettyformat(newtree), "\n")
        tree = newtree
        newtree = revset.foldconcat(tree)
        if newtree != tree:
            ui.note(revset.prettyformat(newtree), "\n")
        if opts["optimize"]:
            weight, optimizedtree = revset.optimize(newtree, True)
            ui.note("* optimized:\n", revset.prettyformat(optimizedtree), "\n")
    func = revset.match(ui, expr)
    revs = func(repo)
    if ui.verbose:
        ui.note("* set:\n", revset.prettyformatset(revs), "\n")
    for c in revs:
        ui.write("%s\n" % c)

@command('debugsetparents', [], _('REV1 [REV2]'))
def debugsetparents(ui, repo, rev1, rev2=None):
    """manually set the parents of the current working directory

    This is useful for writing repository conversion tools, but should
    be used with care. For example, neither the working directory nor the
    dirstate is updated, so file status may be incorrect after running this
    command.

    Returns 0 on success.
    """

    r1 = scmutil.revsingle(repo, rev1).node()
    r2 = scmutil.revsingle(repo, rev2, 'null').node()

    wlock = repo.wlock()
    try:
        repo.dirstate.beginparentchange()
        repo.setparents(r1, r2)
        repo.dirstate.endparentchange()
    finally:
        wlock.release()

@command('debugdirstate|debugstate',
    [('', 'nodates', None, _('do not display the saved mtime')),
    ('', 'datesort', None, _('sort by saved mtime'))],
    _('[OPTION]...'))
def debugstate(ui, repo, nodates=None, datesort=None):
    """show the contents of the current dirstate"""
    timestr = ""
    if datesort:
        keyfunc = lambda x: (x[1][3], x[0]) # sort by mtime, then by filename
    else:
        keyfunc = None # sort by filename
    for file_, ent in sorted(repo.dirstate._map.iteritems(), key=keyfunc):
        if ent[3] == -1:
            timestr = 'unset               '
        elif nodates:
            timestr = 'set                 '
        else:
            timestr = time.strftime("%Y-%m-%d %H:%M:%S ",
                                    time.localtime(ent[3]))
        if ent[1] & 0o20000:
            mode = 'lnk'
        else:
            mode = '%3o' % (ent[1] & 0o777 & ~util.umask)
        ui.write("%c %s %10d %s%s\n" % (ent[0], mode, ent[2], timestr, file_))
    for f in repo.dirstate.copies():
        ui.write(_("copy: %s -> %s\n") % (repo.dirstate.copied(f), f))

@command('debugsub',
    [('r', 'rev', '',
     _('revision to check'), _('REV'))],
    _('[-r REV] [REV]'))
def debugsub(ui, repo, rev=None):
    ctx = scmutil.revsingle(repo, rev, None)
    for k, v in sorted(ctx.substate.items()):
        ui.write(('path %s\n') % k)
        ui.write((' source   %s\n') % v[0])
        ui.write((' revision %s\n') % v[1])

@command('debugsuccessorssets',
    [],
    _('[REV]'))
def debugsuccessorssets(ui, repo, *revs):
    """show set of successors for revision

    A successors set of changeset A is a consistent group of revisions that
    succeed A. It contains non-obsolete changesets only.

    In most cases a changeset A has a single successors set containing a single
    successor (changeset A replaced by A').

    A changeset that is made obsolete with no successors are called "pruned".
    Such changesets have no successors sets at all.

    A changeset that has been "split" will have a successors set containing
    more than one successor.

    A changeset that has been rewritten in multiple different ways is called
    "divergent". Such changesets have multiple successor sets (each of which
    may also be split, i.e. have multiple successors).

    Results are displayed as follows::

        <rev1>
            <successors-1A>
        <rev2>
            <successors-2A>
            <successors-2B1> <successors-2B2> <successors-2B3>

    Here rev2 has two possible (i.e. divergent) successors sets. The first
    holds one element, whereas the second holds three (i.e. the changeset has
    been split).
    """
    # passed to successorssets caching computation from one call to another
    cache = {}
    ctx2str = str
    node2str = short
    if ui.debug():
        def ctx2str(ctx):
            return ctx.hex()
        node2str = hex
    for rev in scmutil.revrange(repo, revs):
        ctx = repo[rev]
        ui.write('%s\n'% ctx2str(ctx))
        for succsset in obsolete.successorssets(repo, ctx.node(), cache):
            if succsset:
                ui.write('    ')
                ui.write(node2str(succsset[0]))
                for node in succsset[1:]:
                    ui.write(' ')
                    ui.write(node2str(node))
            ui.write('\n')

@command('debugwalk', walkopts, _('[OPTION]... [FILE]...'), inferrepo=True)
def debugwalk(ui, repo, *pats, **opts):
    """show how files match on given patterns"""
    m = scmutil.match(repo[None], pats, opts)
    items = list(repo.walk(m))
    if not items:
        return
    f = lambda fn: fn
    if ui.configbool('ui', 'slash') and os.sep != '/':
        f = lambda fn: util.normpath(fn)
    fmt = 'f  %%-%ds  %%-%ds  %%s' % (
        max([len(abs) for abs in items]),
        max([len(m.rel(abs)) for abs in items]))
    for abs in items:
        line = fmt % (abs, f(m.rel(abs)), m.exact(abs) and 'exact' or '')
        ui.write("%s\n" % line.rstrip())

@command('debugwireargs',
    [('', 'three', '', 'three'),
    ('', 'four', '', 'four'),
    ('', 'five', '', 'five'),
    ] + remoteopts,
    _('REPO [OPTIONS]... [ONE [TWO]]'),
    norepo=True)
def debugwireargs(ui, repopath, *vals, **opts):
    repo = hg.peer(ui, opts, repopath)
    for opt in remoteopts:
        del opts[opt[1]]
    args = {}
    for k, v in opts.iteritems():
        if v:
            args[k] = v
    # run twice to check that we don't mess up the stream for the next command
    res1 = repo.debugwireargs(*vals, **args)
    res2 = repo.debugwireargs(*vals, **args)
    ui.write("%s\n" % res1)
    if res1 != res2:
        ui.warn("%s\n" % res2)

@command('^diff',
    [('r', 'rev', [], _('revision'), _('REV')),
    ('c', 'change', '', _('change made by revision'), _('REV'))
    ] + diffopts + diffopts2 + walkopts + subrepoopts,
    _('[OPTION]... ([-c REV] | [-r REV1 [-r REV2]]) [FILE]...'),
    inferrepo=True)
def diff(ui, repo, *pats, **opts):
    """diff repository (or selected files)

    Show differences between revisions for the specified files.

    Differences between files are shown using the unified diff format.

    .. note::

       diff may generate unexpected results for merges, as it will
       default to comparing against the working directory's first
       parent changeset if no revisions are specified.

    When two revision arguments are given, then changes are shown
    between those revisions. If only one revision is specified then
    that revision is compared to the working directory, and, when no
    revisions are specified, the working directory files are compared
    to its parent.

    Alternatively you can specify -c/--change with a revision to see
    the changes in that changeset relative to its first parent.

    Without the -a/--text option, diff will avoid generating diffs of
    files it detects as binary. With -a, diff will generate a diff
    anyway, probably with undesirable results.

    Use the -g/--git option to generate diffs in the git extended diff
    format. For more information, read :hg:`help diffs`.

    .. container:: verbose

      Examples:

      - compare a file in the current working directory to its parent::

          hg diff foo.c

      - compare two historical versions of a directory, with rename info::

          hg diff --git -r 1.0:1.2 lib/

      - get change stats relative to the last change on some date::

          hg diff --stat -r "date('may 2')"

      - diff all newly-added files that contain a keyword::

          hg diff "set:added() and grep(GNU)"

      - compare a revision and its parents::

          hg diff -c 9353         # compare against first parent
          hg diff -r 9353^:9353   # same using revset syntax
          hg diff -r 9353^2:9353  # compare against the second parent

    Returns 0 on success.
    """

    revs = opts.get('rev')
    change = opts.get('change')
    stat = opts.get('stat')
    reverse = opts.get('reverse')

    if revs and change:
        msg = _('cannot specify --rev and --change at the same time')
        raise util.Abort(msg)
    elif change:
        node2 = scmutil.revsingle(repo, change, None).node()
        node1 = repo[node2].p1().node()
    else:
        node1, node2 = scmutil.revpair(repo, revs)

    if reverse:
        node1, node2 = node2, node1

    diffopts = patch.diffallopts(ui, opts)
    m = scmutil.match(repo[node2], pats, opts)
    cmdutil.diffordiffstat(ui, repo, diffopts, node1, node2, m, stat=stat,
                           listsubrepos=opts.get('subrepos'),
                           root=opts.get('root'))

@command('^export',
    [('o', 'output', '',
     _('print output to file with formatted name'), _('FORMAT')),
    ('', 'switch-parent', None, _('diff against the second parent')),
    ('r', 'rev', [], _('revisions to export'), _('REV')),
    ] + diffopts,
    _('[OPTION]... [-o OUTFILESPEC] [-r] [REV]...'))
def export(ui, repo, *changesets, **opts):
    """dump the header and diffs for one or more changesets

    Print the changeset header and diffs for one or more revisions.
    If no revision is given, the parent of the working directory is used.

    The information shown in the changeset header is: author, date,
    branch name (if non-default), changeset hash, parent(s) and commit
    comment.

    .. note::

       export may generate unexpected diff output for merge
       changesets, as it will compare the merge changeset against its
       first parent only.

    Output may be to a file, in which case the name of the file is
    given using a format string. The formatting rules are as follows:

    :``%%``: literal "%" character
    :``%H``: changeset hash (40 hexadecimal digits)
    :``%N``: number of patches being generated
    :``%R``: changeset revision number
    :``%b``: basename of the exporting repository
    :``%h``: short-form changeset hash (12 hexadecimal digits)
    :``%m``: first line of the commit message (only alphanumeric characters)
    :``%n``: zero-padded sequence number, starting at 1
    :``%r``: zero-padded changeset revision number

    Without the -a/--text option, export will avoid generating diffs
    of files it detects as binary. With -a, export will generate a
    diff anyway, probably with undesirable results.

    Use the -g/--git option to generate diffs in the git extended diff
    format. See :hg:`help diffs` for more information.

    With the --switch-parent option, the diff will be against the
    second parent. It can be useful to review a merge.

    .. container:: verbose

      Examples:

      - use export and import to transplant a bugfix to the current
        branch::

          hg export -r 9353 | hg import -

      - export all the changesets between two revisions to a file with
        rename information::

          hg export --git -r 123:150 > changes.txt

      - split outgoing changes into a series of patches with
        descriptive names::

          hg export -r "outgoing()" -o "%n-%m.patch"

    Returns 0 on success.
    """
    changesets += tuple(opts.get('rev', []))
    if not changesets:
        changesets = ['.']
    revs = scmutil.revrange(repo, changesets)
    if not revs:
        raise util.Abort(_("export requires at least one changeset"))
    if len(revs) > 1:
        ui.note(_('exporting patches:\n'))
    else:
        ui.note(_('exporting patch:\n'))
    cmdutil.export(repo, revs, template=opts.get('output'),
                 switch_parent=opts.get('switch_parent'),
                 opts=patch.diffallopts(ui, opts))

@command('files',
    [('r', 'rev', '', _('search the repository as it is in REV'), _('REV')),
     ('0', 'print0', None, _('end filenames with NUL, for use with xargs')),
    ] + walkopts + formatteropts + subrepoopts,
    _('[OPTION]... [PATTERN]...'))
def files(ui, repo, *pats, **opts):
    """list tracked files

    Print files under Mercurial control in the working directory or
    specified revision whose names match the given patterns (excluding
    removed files).

    If no patterns are given to match, this command prints the names
    of all files under Mercurial control in the working directory.

    .. container:: verbose

      Examples:

      - list all files under the current directory::

          hg files .

      - shows sizes and flags for current revision::

          hg files -vr .

      - list all files named README::

          hg files -I "**/README"

      - list all binary files::

          hg files "set:binary()"

      - find files containing a regular expression::

          hg files "set:grep('bob')"

      - search tracked file contents with xargs and grep::

          hg files -0 | xargs -0 grep foo

    See :hg:`help patterns` and :hg:`help filesets` for more information
    on specifying file patterns.

    Returns 0 if a match is found, 1 otherwise.

    """
    ctx = scmutil.revsingle(repo, opts.get('rev'), None)

    end = '\n'
    if opts.get('print0'):
        end = '\0'
    fm = ui.formatter('files', opts)
    fmt = '%s' + end

    m = scmutil.match(ctx, pats, opts)
    ret = cmdutil.files(ui, ctx, m, fm, fmt, opts.get('subrepos'))

    fm.end()

    return ret

@command('^forget', walkopts, _('[OPTION]... FILE...'), inferrepo=True)
def forget(ui, repo, *pats, **opts):
    """forget the specified files on the next commit

    Mark the specified files so they will no longer be tracked
    after the next commit.

    This only removes files from the current branch, not from the
    entire project history, and it does not delete them from the
    working directory.

    To undo a forget before the next commit, see :hg:`add`.

    .. container:: verbose

      Examples:

      - forget newly-added binary files::

          hg forget "set:added() and binary()"

      - forget files that would be excluded by .hgignore::

          hg forget "set:hgignore()"

    Returns 0 on success.
    """

    if not pats:
        raise util.Abort(_('no files specified'))

    m = scmutil.match(repo[None], pats, opts)
    rejected = cmdutil.forget(ui, repo, m, prefix="", explicitonly=False)[0]
    return rejected and 1 or 0

@command(
    'graft',
    [('r', 'rev', [], _('revisions to graft'), _('REV')),
     ('c', 'continue', False, _('resume interrupted graft')),
     ('e', 'edit', False, _('invoke editor on commit messages')),
     ('', 'log', None, _('append graft info to log message')),
     ('f', 'force', False, _('force graft')),
     ('D', 'currentdate', False,
      _('record the current date as commit date')),
     ('U', 'currentuser', False,
      _('record the current user as committer'), _('DATE'))]
    + commitopts2 + mergetoolopts  + dryrunopts,
    _('[OPTION]... [-r] REV...'))
def graft(ui, repo, *revs, **opts):
    '''copy changes from other branches onto the current branch

    This command uses Mercurial's merge logic to copy individual
    changes from other branches without merging branches in the
    history graph. This is sometimes known as 'backporting' or
    'cherry-picking'. By default, graft will copy user, date, and
    description from the source changesets.

    Changesets that are ancestors of the current revision, that have
    already been grafted, or that are merges will be skipped.

    If --log is specified, log messages will have a comment appended
    of the form::

      (grafted from CHANGESETHASH)

    If --force is specified, revisions will be grafted even if they
    are already ancestors of or have been grafted to the destination.
    This is useful when the revisions have since been backed out.

    If a graft merge results in conflicts, the graft process is
    interrupted so that the current merge can be manually resolved.
    Once all conflicts are addressed, the graft process can be
    continued with the -c/--continue option.

    .. note::

      The -c/--continue option does not reapply earlier options, except
      for --force.

    .. container:: verbose

      Examples:

      - copy a single change to the stable branch and edit its description::

          hg update stable
          hg graft --edit 9393

      - graft a range of changesets with one exception, updating dates::

          hg graft -D "2085::2093 and not 2091"

      - continue a graft after resolving conflicts::

          hg graft -c

      - show the source of a grafted changeset::

          hg log --debug -r .

    See :hg:`help revisions` and :hg:`help revsets` for more about
    specifying revisions.

    Returns 0 on successful completion.
    '''

    revs = list(revs)
    revs.extend(opts['rev'])

    if not opts.get('user') and opts.get('currentuser'):
        opts['user'] = ui.username()
    if not opts.get('date') and opts.get('currentdate'):
        opts['date'] = "%d %d" % util.makedate()

    editor = cmdutil.getcommiteditor(editform='graft', **opts)

    cont = False
    if opts['continue']:
        cont = True
        if revs:
            raise util.Abort(_("can't specify --continue and revisions"))
        # read in unfinished revisions
        try:
            nodes = repo.vfs.read('graftstate').splitlines()
            revs = [repo[node].rev() for node in nodes]
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            raise util.Abort(_("no graft state found, can't continue"))
    else:
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)
        if not revs:
            raise util.Abort(_('no revisions specified'))
        revs = scmutil.revrange(repo, revs)

    skipped = set()
    # check for merges
    for rev in repo.revs('%ld and merge()', revs):
        ui.warn(_('skipping ungraftable merge revision %s\n') % rev)
        skipped.add(rev)
    revs = [r for r in revs if r not in skipped]
    if not revs:
        return -1

    # Don't check in the --continue case, in effect retaining --force across
    # --continues. That's because without --force, any revisions we decided to
    # skip would have been filtered out here, so they wouldn't have made their
    # way to the graftstate. With --force, any revisions we would have otherwise
    # skipped would not have been filtered out, and if they hadn't been applied
    # already, they'd have been in the graftstate.
    if not (cont or opts.get('force')):
        # check for ancestors of dest branch
        crev = repo['.'].rev()
        ancestors = repo.changelog.ancestors([crev], inclusive=True)
        # Cannot use x.remove(y) on smart set, this has to be a list.
        # XXX make this lazy in the future
        revs = list(revs)
        # don't mutate while iterating, create a copy
        for rev in list(revs):
            if rev in ancestors:
                ui.warn(_('skipping ancestor revision %d:%s\n') %
                        (rev, repo[rev]))
                # XXX remove on list is slow
                revs.remove(rev)
        if not revs:
            return -1

        # analyze revs for earlier grafts
        ids = {}
        for ctx in repo.set("%ld", revs):
            ids[ctx.hex()] = ctx.rev()
            n = ctx.extra().get('source')
            if n:
                ids[n] = ctx.rev()

        # check ancestors for earlier grafts
        ui.debug('scanning for duplicate grafts\n')

        for rev in repo.changelog.findmissingrevs(revs, [crev]):
            ctx = repo[rev]
            n = ctx.extra().get('source')
            if n in ids:
                try:
                    r = repo[n].rev()
                except error.RepoLookupError:
                    r = None
                if r in revs:
                    ui.warn(_('skipping revision %d:%s '
                              '(already grafted to %d:%s)\n')
                            % (r, repo[r], rev, ctx))
                    revs.remove(r)
                elif ids[n] in revs:
                    if r is None:
                        ui.warn(_('skipping already grafted revision %d:%s '
                                  '(%d:%s also has unknown origin %s)\n')
                                % (ids[n], repo[ids[n]], rev, ctx, n[:12]))
                    else:
                        ui.warn(_('skipping already grafted revision %d:%s '
                                  '(%d:%s also has origin %d:%s)\n')
                                % (ids[n], repo[ids[n]], rev, ctx, r, n[:12]))
                    revs.remove(ids[n])
            elif ctx.hex() in ids:
                r = ids[ctx.hex()]
                ui.warn(_('skipping already grafted revision %d:%s '
                          '(was grafted from %d:%s)\n') %
                        (r, repo[r], rev, ctx))
                revs.remove(r)
        if not revs:
            return -1

    wlock = repo.wlock()
    try:
        for pos, ctx in enumerate(repo.set("%ld", revs)):
            desc = '%d:%s "%s"' % (ctx.rev(), ctx,
                                   ctx.description().split('\n', 1)[0])
            names = repo.nodetags(ctx.node()) + repo.nodebookmarks(ctx.node())
            if names:
                desc += ' (%s)' % ' '.join(names)
            ui.status(_('grafting %s\n') % desc)
            if opts.get('dry_run'):
                continue

            source = ctx.extra().get('source')
            extra = {}
            if source:
                extra['source'] = source
                extra['intermediate-source'] = ctx.hex()
            else:
                extra['source'] = ctx.hex()
            user = ctx.user()
            if opts.get('user'):
                user = opts['user']
            date = ctx.date()
            if opts.get('date'):
                date = opts['date']
            message = ctx.description()
            if opts.get('log'):
                message += '\n(grafted from %s)' % ctx.hex()

            # we don't merge the first commit when continuing
            if not cont:
                # perform the graft merge with p1(rev) as 'ancestor'
                try:
                    # ui.forcemerge is an internal variable, do not document
                    repo.ui.setconfig('ui', 'forcemerge', opts.get('tool', ''),
                                      'graft')
                    stats = mergemod.graft(repo, ctx, ctx.p1(),
                                           ['local', 'graft'])
                finally:
                    repo.ui.setconfig('ui', 'forcemerge', '', 'graft')
                # report any conflicts
                if stats and stats[3] > 0:
                    # write out state for --continue
                    nodelines = [repo[rev].hex() + "\n" for rev in revs[pos:]]
                    repo.vfs.write('graftstate', ''.join(nodelines))
                    raise util.Abort(
                        _("unresolved conflicts, can't continue"),
                        hint=_('use hg resolve and hg graft --continue'))
            else:
                cont = False

            # commit
            node = repo.commit(text=message, user=user,
                        date=date, extra=extra, editor=editor)
            if node is None:
                ui.warn(
                    _('note: graft of %d:%s created no changes to commit\n') %
                    (ctx.rev(), ctx))
    finally:
        wlock.release()

    # remove state when we complete successfully
    if not opts.get('dry_run'):
        util.unlinkpath(repo.join('graftstate'), ignoremissing=True)

    return 0

@command('grep',
    [('0', 'print0', None, _('end fields with NUL')),
    ('', 'all', None, _('print all revisions that match')),
    ('a', 'text', None, _('treat all files as text')),
    ('f', 'follow', None,
     _('follow changeset history,'
       ' or file history across copies and renames')),
    ('i', 'ignore-case', None, _('ignore case when matching')),
    ('l', 'files-with-matches', None,
     _('print only filenames and revisions that match')),
    ('n', 'line-number', None, _('print matching line numbers')),
    ('r', 'rev', [],
     _('only search files changed within revision range'), _('REV')),
    ('u', 'user', None, _('list the author (long with -v)')),
    ('d', 'date', None, _('list the date (short with -q)')),
    ] + walkopts,
    _('[OPTION]... PATTERN [FILE]...'),
    inferrepo=True)
def grep(ui, repo, pattern, *pats, **opts):
    """search for a pattern in specified files and revisions

    Search revisions of files for a regular expression.

    This command behaves differently than Unix grep. It only accepts
    Python/Perl regexps. It searches repository history, not the
    working directory. It always prints the revision number in which a
    match appears.

    By default, grep only prints output for the first revision of a
    file in which it finds a match. To get it to print every revision
    that contains a change in match status ("-" for a match that
    becomes a non-match, or "+" for a non-match that becomes a match),
    use the --all flag.

    Returns 0 if a match is found, 1 otherwise.
    """
    reflags = re.M
    if opts.get('ignore_case'):
        reflags |= re.I
    try:
        regexp = util.re.compile(pattern, reflags)
    except re.error as inst:
        ui.warn(_("grep: invalid match pattern: %s\n") % inst)
        return 1
    sep, eol = ':', '\n'
    if opts.get('print0'):
        sep = eol = '\0'

    getfile = util.lrucachefunc(repo.file)

    def matchlines(body):
        begin = 0
        linenum = 0
        while begin < len(body):
            match = regexp.search(body, begin)
            if not match:
                break
            mstart, mend = match.span()
            linenum += body.count('\n', begin, mstart) + 1
            lstart = body.rfind('\n', begin, mstart) + 1 or begin
            begin = body.find('\n', mend) + 1 or len(body) + 1
            lend = begin - 1
            yield linenum, mstart - lstart, mend - lstart, body[lstart:lend]

    class linestate(object):
        def __init__(self, line, linenum, colstart, colend):
            self.line = line
            self.linenum = linenum
            self.colstart = colstart
            self.colend = colend

        def __hash__(self):
            return hash((self.linenum, self.line))

        def __eq__(self, other):
            return self.line == other.line

        def __iter__(self):
            yield (self.line[:self.colstart], '')
            yield (self.line[self.colstart:self.colend], 'grep.match')
            rest = self.line[self.colend:]
            while rest != '':
                match = regexp.search(rest)
                if not match:
                    yield (rest, '')
                    break
                mstart, mend = match.span()
                yield (rest[:mstart], '')
                yield (rest[mstart:mend], 'grep.match')
                rest = rest[mend:]

    matches = {}
    copies = {}
    def grepbody(fn, rev, body):
        matches[rev].setdefault(fn, [])
        m = matches[rev][fn]
        for lnum, cstart, cend, line in matchlines(body):
            s = linestate(line, lnum, cstart, cend)
            m.append(s)

    def difflinestates(a, b):
        sm = difflib.SequenceMatcher(None, a, b)
        for tag, alo, ahi, blo, bhi in sm.get_opcodes():
            if tag == 'insert':
                for i in xrange(blo, bhi):
                    yield ('+', b[i])
            elif tag == 'delete':
                for i in xrange(alo, ahi):
                    yield ('-', a[i])
            elif tag == 'replace':
                for i in xrange(alo, ahi):
                    yield ('-', a[i])
                for i in xrange(blo, bhi):
                    yield ('+', b[i])

    def display(fn, ctx, pstates, states):
        rev = ctx.rev()
        if ui.quiet:
            datefunc = util.shortdate
        else:
            datefunc = util.datestr
        found = False
        @util.cachefunc
        def binary():
            flog = getfile(fn)
            return util.binary(flog.read(ctx.filenode(fn)))

        if opts.get('all'):
            iter = difflinestates(pstates, states)
        else:
            iter = [('', l) for l in states]
        for change, l in iter:
            cols = [(fn, 'grep.filename'), (str(rev), 'grep.rev')]

            if opts.get('line_number'):
                cols.append((str(l.linenum), 'grep.linenumber'))
            if opts.get('all'):
                cols.append((change, 'grep.change'))
            if opts.get('user'):
                cols.append((ui.shortuser(ctx.user()), 'grep.user'))
            if opts.get('date'):
                cols.append((datefunc(ctx.date()), 'grep.date'))
            for col, label in cols[:-1]:
                ui.write(col, label=label)
                ui.write(sep, label='grep.sep')
            ui.write(cols[-1][0], label=cols[-1][1])
            if not opts.get('files_with_matches'):
                ui.write(sep, label='grep.sep')
                if not opts.get('text') and binary():
                    ui.write(" Binary file matches")
                else:
                    for s, label in l:
                        ui.write(s, label=label)
            ui.write(eol)
            found = True
            if opts.get('files_with_matches'):
                break
        return found

    skip = {}
    revfiles = {}
    matchfn = scmutil.match(repo[None], pats, opts)
    found = False
    follow = opts.get('follow')

    def prep(ctx, fns):
        rev = ctx.rev()
        pctx = ctx.p1()
        parent = pctx.rev()
        matches.setdefault(rev, {})
        matches.setdefault(parent, {})
        files = revfiles.setdefault(rev, [])
        for fn in fns:
            flog = getfile(fn)
            try:
                fnode = ctx.filenode(fn)
            except error.LookupError:
                continue

            copied = flog.renamed(fnode)
            copy = follow and copied and copied[0]
            if copy:
                copies.setdefault(rev, {})[fn] = copy
            if fn in skip:
                if copy:
                    skip[copy] = True
                continue
            files.append(fn)

            if fn not in matches[rev]:
                grepbody(fn, rev, flog.read(fnode))

            pfn = copy or fn
            if pfn not in matches[parent]:
                try:
                    fnode = pctx.filenode(pfn)
                    grepbody(pfn, parent, flog.read(fnode))
                except error.LookupError:
                    pass

    for ctx in cmdutil.walkchangerevs(repo, matchfn, opts, prep):
        rev = ctx.rev()
        parent = ctx.p1().rev()
        for fn in sorted(revfiles.get(rev, [])):
            states = matches[rev][fn]
            copy = copies.get(rev, {}).get(fn)
            if fn in skip:
                if copy:
                    skip[copy] = True
                continue
            pstates = matches.get(parent, {}).get(copy or fn, [])
            if pstates or states:
                r = display(fn, ctx, pstates, states)
                found = found or r
                if r and not opts.get('all'):
                    skip[fn] = True
                    if copy:
                        skip[copy] = True
        del matches[rev]
        del revfiles[rev]

    return not found

@command('heads',
    [('r', 'rev', '',
     _('show only heads which are descendants of STARTREV'), _('STARTREV')),
    ('t', 'topo', False, _('show topological heads only')),
    ('a', 'active', False, _('show active branchheads only (DEPRECATED)')),
    ('c', 'closed', False, _('show normal and closed branch heads')),
    ] + templateopts,
    _('[-ct] [-r STARTREV] [REV]...'))
def heads(ui, repo, *branchrevs, **opts):
    """show branch heads

    With no arguments, show all open branch heads in the repository.
    Branch heads are changesets that have no descendants on the
    same branch. They are where development generally takes place and
    are the usual targets for update and merge operations.

    If one or more REVs are given, only open branch heads on the
    branches associated with the specified changesets are shown. This
    means that you can use :hg:`heads .` to see the heads on the
    currently checked-out branch.

    If -c/--closed is specified, also show branch heads marked closed
    (see :hg:`commit --close-branch`).

    If STARTREV is specified, only those heads that are descendants of
    STARTREV will be displayed.

    If -t/--topo is specified, named branch mechanics will be ignored and only
    topological heads (changesets with no children) will be shown.

    Returns 0 if matching heads are found, 1 if not.
    """

    start = None
    if 'rev' in opts:
        start = scmutil.revsingle(repo, opts['rev'], None).node()

    if opts.get('topo'):
        heads = [repo[h] for h in repo.heads(start)]
    else:
        heads = []
        for branch in repo.branchmap():
            heads += repo.branchheads(branch, start, opts.get('closed'))
        heads = [repo[h] for h in heads]

    if branchrevs:
        branches = set(repo[br].branch() for br in branchrevs)
        heads = [h for h in heads if h.branch() in branches]

    if opts.get('active') and branchrevs:
        dagheads = repo.heads(start)
        heads = [h for h in heads if h.node() in dagheads]

    if branchrevs:
        haveheads = set(h.branch() for h in heads)
        if branches - haveheads:
            headless = ', '.join(b for b in branches - haveheads)
            msg = _('no open branch heads found on branches %s')
            if opts.get('rev'):
                msg += _(' (started at %s)') % opts['rev']
            ui.warn((msg + '\n') % headless)

    if not heads:
        return 1

    heads = sorted(heads, key=lambda x: -x.rev())
    displayer = cmdutil.show_changeset(ui, repo, opts)
    for ctx in heads:
        displayer.show(ctx)
    displayer.close()

@command('help',
    [('e', 'extension', None, _('show only help for extensions')),
     ('c', 'command', None, _('show only help for commands')),
     ('k', 'keyword', '', _('show topics matching keyword')),
     ],
    _('[-ec] [TOPIC]'),
    norepo=True)
def help_(ui, name=None, **opts):
    """show help for a given topic or a help overview

    With no arguments, print a list of commands with short help messages.

    Given a topic, extension, or command name, print help for that
    topic.

    Returns 0 if successful.
    """

    textwidth = min(ui.termwidth(), 80) - 2

    keep = []
    if ui.verbose:
        keep.append('verbose')
    if sys.platform.startswith('win'):
        keep.append('windows')
    elif sys.platform == 'OpenVMS':
        keep.append('vms')
    elif sys.platform == 'plan9':
        keep.append('plan9')
    else:
        keep.append('unix')
        keep.append(sys.platform.lower())

    section = None
    if name and '.' in name:
        name, section = name.split('.', 1)

    text = help.help_(ui, name, **opts)

    formatted, pruned = minirst.format(text, textwidth, keep=keep,
                                       section=section)
    if section and not formatted:
        raise util.Abort(_("help section not found"))

    if 'verbose' in pruned:
        keep.append('omitted')
    else:
        keep.append('notomitted')
    formatted, pruned = minirst.format(text, textwidth, keep=keep,
                                       section=section)
    ui.write(formatted)


@command('identify|id',
    [('r', 'rev', '',
     _('identify the specified revision'), _('REV')),
    ('n', 'num', None, _('show local revision number')),
    ('i', 'id', None, _('show global revision id')),
    ('b', 'branch', None, _('show branch')),
    ('t', 'tags', None, _('show tags')),
    ('B', 'bookmarks', None, _('show bookmarks')),
    ] + remoteopts,
    _('[-nibtB] [-r REV] [SOURCE]'),
    optionalrepo=True)
def identify(ui, repo, source=None, rev=None,
             num=None, id=None, branch=None, tags=None, bookmarks=None, **opts):
    """identify the working directory or specified revision

    Print a summary identifying the repository state at REV using one or
    two parent hash identifiers, followed by a "+" if the working
    directory has uncommitted changes, the branch name (if not default),
    a list of tags, and a list of bookmarks.

    When REV is not given, print a summary of the current state of the
    repository.

    Specifying a path to a repository root or Mercurial bundle will
    cause lookup to operate on that repository/bundle.

    .. container:: verbose

      Examples:

      - generate a build identifier for the working directory::

          hg id --id > build-id.dat

      - find the revision corresponding to a tag::

          hg id -n -r 1.3

      - check the most recent revision of a remote repository::

          hg id -r tip http://selenic.com/hg/

    Returns 0 if successful.
    """

    if not repo and not source:
        raise util.Abort(_("there is no Mercurial repository here "
                           "(.hg not found)"))

    if ui.debugflag:
        hexfunc = hex
    else:
        hexfunc = short
    default = not (num or id or branch or tags or bookmarks)
    output = []
    revs = []

    if source:
        source, branches = hg.parseurl(ui.expandpath(source))
        peer = hg.peer(repo or ui, opts, source) # only pass ui when no repo
        repo = peer.local()
        revs, checkout = hg.addbranchrevs(repo, peer, branches, None)

    if not repo:
        if num or branch or tags:
            raise util.Abort(
                _("can't query remote revision number, branch, or tags"))
        if not rev and revs:
            rev = revs[0]
        if not rev:
            rev = "tip"

        remoterev = peer.lookup(rev)
        if default or id:
            output = [hexfunc(remoterev)]

        def getbms():
            bms = []

            if 'bookmarks' in peer.listkeys('namespaces'):
                hexremoterev = hex(remoterev)
                bms = [bm for bm, bmr in peer.listkeys('bookmarks').iteritems()
                       if bmr == hexremoterev]

            return sorted(bms)

        if bookmarks:
            output.extend(getbms())
        elif default and not ui.quiet:
            # multiple bookmarks for a single parent separated by '/'
            bm = '/'.join(getbms())
            if bm:
                output.append(bm)
    else:
        ctx = scmutil.revsingle(repo, rev, None)

        if ctx.rev() is None:
            ctx = repo[None]
            parents = ctx.parents()
            changed = ""
            if default or id or num:
                if (any(repo.status())
                    or any(ctx.sub(s).dirty() for s in ctx.substate)):
                    changed = '+'
            if default or id:
                output = ["%s%s" %
                  ('+'.join([hexfunc(p.node()) for p in parents]), changed)]
            if num:
                output.append("%s%s" %
                  ('+'.join([str(p.rev()) for p in parents]), changed))
        else:
            if default or id:
                output = [hexfunc(ctx.node())]
            if num:
                output.append(str(ctx.rev()))

        if default and not ui.quiet:
            b = ctx.branch()
            if b != 'default':
                output.append("(%s)" % b)

            # multiple tags for a single parent separated by '/'
            t = '/'.join(ctx.tags())
            if t:
                output.append(t)

            # multiple bookmarks for a single parent separated by '/'
            bm = '/'.join(ctx.bookmarks())
            if bm:
                output.append(bm)
        else:
            if branch:
                output.append(ctx.branch())

            if tags:
                output.extend(ctx.tags())

            if bookmarks:
                output.extend(ctx.bookmarks())

    ui.write("%s\n" % ' '.join(output))

@command('import|patch',
    [('p', 'strip', 1,
     _('directory strip option for patch. This has the same '
       'meaning as the corresponding patch option'), _('NUM')),
    ('b', 'base', '', _('base path (DEPRECATED)'), _('PATH')),
    ('e', 'edit', False, _('invoke editor on commit messages')),
    ('f', 'force', None,
     _('skip check for outstanding uncommitted changes (DEPRECATED)')),
    ('', 'no-commit', None,
     _("don't commit, just update the working directory")),
    ('', 'bypass', None,
     _("apply patch without touching the working directory")),
    ('', 'partial', None,
     _('commit even if some hunks fail')),
    ('', 'exact', None,
     _('apply patch to the nodes from which it was generated')),
    ('', 'prefix', '',
     _('apply patch to subdirectory'), _('DIR')),
    ('', 'import-branch', None,
     _('use any branch information in patch (implied by --exact)'))] +
    commitopts + commitopts2 + similarityopts,
    _('[OPTION]... PATCH...'))
def import_(ui, repo, patch1=None, *patches, **opts):
    """import an ordered set of patches

    Import a list of patches and commit them individually (unless
    --no-commit is specified).

    Because import first applies changes to the working directory,
    import will abort if there are outstanding changes.

    You can import a patch straight from a mail message. Even patches
    as attachments work (to use the body part, it must have type
    text/plain or text/x-patch). From and Subject headers of email
    message are used as default committer and commit message. All
    text/plain body parts before first diff are added to commit
    message.

    If the imported patch was generated by :hg:`export`, user and
    description from patch override values from message headers and
    body. Values given on command line with -m/--message and -u/--user
    override these.

    If --exact is specified, import will set the working directory to
    the parent of each patch before applying it, and will abort if the
    resulting changeset has a different ID than the one recorded in
    the patch. This may happen due to character set problems or other
    deficiencies in the text patch format.

    Use --bypass to apply and commit patches directly to the
    repository, not touching the working directory. Without --exact,
    patches will be applied on top of the working directory parent
    revision.

    With -s/--similarity, hg will attempt to discover renames and
    copies in the patch in the same way as :hg:`addremove`.

    Use --partial to ensure a changeset will be created from the patch
    even if some hunks fail to apply. Hunks that fail to apply will be
    written to a <target-file>.rej file. Conflicts can then be resolved
    by hand before :hg:`commit --amend` is run to update the created
    changeset. This flag exists to let people import patches that
    partially apply without losing the associated metadata (author,
    date, description, ...). Note that when none of the hunk applies
    cleanly, :hg:`import --partial` will create an empty changeset,
    importing only the patch metadata.

    It is possible to use external patch programs to perform the patch
    by setting the ``ui.patch`` configuration option. For the default
    internal tool, the fuzz can also be configured via ``patch.fuzz``.
    See :hg:`help config` for more information about configuration
    files and how to use these options.

    To read a patch from standard input, use "-" as the patch name. If
    a URL is specified, the patch will be downloaded from it.
    See :hg:`help dates` for a list of formats valid for -d/--date.

    .. container:: verbose

      Examples:

      - import a traditional patch from a website and detect renames::

          hg import -s 80 http://example.com/bugfix.patch

      - import a changeset from an hgweb server::

          hg import http://www.selenic.com/hg/rev/5ca8c111e9aa

      - import all the patches in an Unix-style mbox::

          hg import incoming-patches.mbox

      - attempt to exactly restore an exported changeset (not always
        possible)::

          hg import --exact proposed-fix.patch

      - use an external tool to apply a patch which is too fuzzy for
        the default internal tool.

          hg import --config ui.patch="patch --merge" fuzzy.patch

      - change the default fuzzing from 2 to a less strict 7

          hg import --config ui.fuzz=7 fuzz.patch

    Returns 0 on success, 1 on partial success (see --partial).
    """

    if not patch1:
        raise util.Abort(_('need at least one patch to import'))

    patches = (patch1,) + patches

    date = opts.get('date')
    if date:
        opts['date'] = util.parsedate(date)

    update = not opts.get('bypass')
    if not update and opts.get('no_commit'):
        raise util.Abort(_('cannot use --no-commit with --bypass'))
    try:
        sim = float(opts.get('similarity') or 0)
    except ValueError:
        raise util.Abort(_('similarity must be a number'))
    if sim < 0 or sim > 100:
        raise util.Abort(_('similarity must be between 0 and 100'))
    if sim and not update:
        raise util.Abort(_('cannot use --similarity with --bypass'))
    if opts.get('exact') and opts.get('edit'):
        raise util.Abort(_('cannot use --exact with --edit'))
    if opts.get('exact') and opts.get('prefix'):
        raise util.Abort(_('cannot use --exact with --prefix'))

    if update:
        cmdutil.checkunfinished(repo)
    if (opts.get('exact') or not opts.get('force')) and update:
        cmdutil.bailifchanged(repo)

    base = opts["base"]
    wlock = dsguard = lock = tr = None
    msgs = []
    ret = 0


    try:
        try:
            wlock = repo.wlock()
            dsguard = cmdutil.dirstateguard(repo, 'import')
            if not opts.get('no_commit'):
                lock = repo.lock()
                tr = repo.transaction('import')
            parents = repo.parents()
            for patchurl in patches:
                if patchurl == '-':
                    ui.status(_('applying patch from stdin\n'))
                    patchfile = ui.fin
                    patchurl = 'stdin'      # for error message
                else:
                    patchurl = os.path.join(base, patchurl)
                    ui.status(_('applying %s\n') % patchurl)
                    patchfile = hg.openpath(ui, patchurl)

                haspatch = False
                for hunk in patch.split(patchfile):
                    (msg, node, rej) = cmdutil.tryimportone(ui, repo, hunk,
                                                            parents, opts,
                                                            msgs, hg.clean)
                    if msg:
                        haspatch = True
                        ui.note(msg + '\n')
                    if update or opts.get('exact'):
                        parents = repo.parents()
                    else:
                        parents = [repo[node]]
                    if rej:
                        ui.write_err(_("patch applied partially\n"))
                        ui.write_err(_("(fix the .rej files and run "
                                       "`hg commit --amend`)\n"))
                        ret = 1
                        break

                if not haspatch:
                    raise util.Abort(_('%s: no diffs found') % patchurl)

            if tr:
                tr.close()
            if msgs:
                repo.savecommitmessage('\n* * *\n'.join(msgs))
            dsguard.close()
            return ret
        finally:
            # TODO: get rid of this meaningless try/finally enclosing.
            # this is kept only to reduce changes in a patch.
            pass
    finally:
        if tr:
            tr.release()
        release(lock, dsguard, wlock)

@command('incoming|in',
    [('f', 'force', None,
     _('run even if remote repository is unrelated')),
    ('n', 'newest-first', None, _('show newest record first')),
    ('', 'bundle', '',
     _('file to store the bundles into'), _('FILE')),
    ('r', 'rev', [], _('a remote changeset intended to be added'), _('REV')),
    ('B', 'bookmarks', False, _("compare bookmarks")),
    ('b', 'branch', [],
     _('a specific branch you would like to pull'), _('BRANCH')),
    ] + logopts + remoteopts + subrepoopts,
    _('[-p] [-n] [-M] [-f] [-r REV]... [--bundle FILENAME] [SOURCE]'))
def incoming(ui, repo, source="default", **opts):
    """show new changesets found in source

    Show new changesets found in the specified path/URL or the default
    pull location. These are the changesets that would have been pulled
    if a pull at the time you issued this command.

    See pull for valid source format details.

    .. container:: verbose

      With -B/--bookmarks, the result of bookmark comparison between
      local and remote repositories is displayed. With -v/--verbose,
      status is also displayed for each bookmark like below::

        BM1               01234567890a added
        BM2               1234567890ab advanced
        BM3               234567890abc diverged
        BM4               34567890abcd changed

      The action taken locally when pulling depends on the
      status of each bookmark:

      :``added``: pull will create it
      :``advanced``: pull will update it
      :``diverged``: pull will create a divergent bookmark
      :``changed``: result depends on remote changesets

      From the point of view of pulling behavior, bookmark
      existing only in the remote repository are treated as ``added``,
      even if it is in fact locally deleted.

    .. container:: verbose

      For remote repository, using --bundle avoids downloading the
      changesets twice if the incoming is followed by a pull.

      Examples:

      - show incoming changes with patches and full description::

          hg incoming -vp

      - show incoming changes excluding merges, store a bundle::

          hg in -vpM --bundle incoming.hg
          hg pull incoming.hg

      - briefly list changes inside a bundle::

          hg in changes.hg -T "{desc|firstline}\\n"

    Returns 0 if there are incoming changes, 1 otherwise.
    """
    if opts.get('graph'):
        cmdutil.checkunsupportedgraphflags([], opts)
        def display(other, chlist, displayer):
            revdag = cmdutil.graphrevs(other, chlist, opts)
            showparents = [ctx.node() for ctx in repo[None].parents()]
            cmdutil.displaygraph(ui, revdag, displayer, showparents,
                                 graphmod.asciiedges)

        hg._incoming(display, lambda: 1, ui, repo, source, opts, buffered=True)
        return 0

    if opts.get('bundle') and opts.get('subrepos'):
        raise util.Abort(_('cannot combine --bundle and --subrepos'))

    if opts.get('bookmarks'):
        source, branches = hg.parseurl(ui.expandpath(source),
                                       opts.get('branch'))
        other = hg.peer(repo, opts, source)
        if 'bookmarks' not in other.listkeys('namespaces'):
            ui.warn(_("remote doesn't support bookmarks\n"))
            return 0
        ui.status(_('comparing with %s\n') % util.hidepassword(source))
        return bookmarks.incoming(ui, repo, other)

    repo._subtoppath = ui.expandpath(source)
    try:
        return hg.incoming(ui, repo, source, opts)
    finally:
        del repo._subtoppath


@command('^init', remoteopts, _('[-e CMD] [--remotecmd CMD] [DEST]'),
         norepo=True)
def init(ui, dest=".", **opts):
    """create a new repository in the given directory

    Initialize a new repository in the given directory. If the given
    directory does not exist, it will be created.

    If no directory is given, the current directory is used.

    It is possible to specify an ``ssh://`` URL as the destination.
    See :hg:`help urls` for more information.

    Returns 0 on success.
    """
    hg.peer(ui, opts, ui.expandpath(dest), create=True)

@command('locate',
    [('r', 'rev', '', _('search the repository as it is in REV'), _('REV')),
    ('0', 'print0', None, _('end filenames with NUL, for use with xargs')),
    ('f', 'fullpath', None, _('print complete paths from the filesystem root')),
    ] + walkopts,
    _('[OPTION]... [PATTERN]...'))
def locate(ui, repo, *pats, **opts):
    """locate files matching specific patterns (DEPRECATED)

    Print files under Mercurial control in the working directory whose
    names match the given patterns.

    By default, this command searches all directories in the working
    directory. To search just the current directory and its
    subdirectories, use "--include .".

    If no patterns are given to match, this command prints the names
    of all files under Mercurial control in the working directory.

    If you want to feed the output of this command into the "xargs"
    command, use the -0 option to both this command and "xargs". This
    will avoid the problem of "xargs" treating single filenames that
    contain whitespace as multiple filenames.

    See :hg:`help files` for a more versatile command.

    Returns 0 if a match is found, 1 otherwise.
    """
    if opts.get('print0'):
        end = '\0'
    else:
        end = '\n'
    rev = scmutil.revsingle(repo, opts.get('rev'), None).node()

    ret = 1
    ctx = repo[rev]
    m = scmutil.match(ctx, pats, opts, default='relglob',
                      badfn=lambda x, y: False)

    for abs in ctx.matches(m):
        if opts.get('fullpath'):
            ui.write(repo.wjoin(abs), end)
        else:
            ui.write(((pats and m.rel(abs)) or abs), end)
        ret = 0

    return ret

@command('^log|history',
    [('f', 'follow', None,
     _('follow changeset history, or file history across copies and renames')),
    ('', 'follow-first', None,
     _('only follow the first parent of merge changesets (DEPRECATED)')),
    ('d', 'date', '', _('show revisions matching date spec'), _('DATE')),
    ('C', 'copies', None, _('show copied files')),
    ('k', 'keyword', [],
     _('do case-insensitive search for a given text'), _('TEXT')),
    ('r', 'rev', [], _('show the specified revision or revset'), _('REV')),
    ('', 'removed', None, _('include revisions where files were removed')),
    ('m', 'only-merges', None, _('show only merges (DEPRECATED)')),
    ('u', 'user', [], _('revisions committed by user'), _('USER')),
    ('', 'only-branch', [],
     _('show only changesets within the given named branch (DEPRECATED)'),
     _('BRANCH')),
    ('b', 'branch', [],
     _('show changesets within the given named branch'), _('BRANCH')),
    ('P', 'prune', [],
     _('do not display revision or any of its ancestors'), _('REV')),
    ] + logopts + walkopts,
    _('[OPTION]... [FILE]'),
    inferrepo=True)
def log(ui, repo, *pats, **opts):
    """show revision history of entire repository or files

    Print the revision history of the specified files or the entire
    project.

    If no revision range is specified, the default is ``tip:0`` unless
    --follow is set, in which case the working directory parent is
    used as the starting revision.

    File history is shown without following rename or copy history of
    files. Use -f/--follow with a filename to follow history across
    renames and copies. --follow without a filename will only show
    ancestors or descendants of the starting revision.

    By default this command prints revision number and changeset id,
    tags, non-trivial parents, user, date and time, and a summary for
    each commit. When the -v/--verbose switch is used, the list of
    changed files and full commit message are shown.

    With --graph the revisions are shown as an ASCII art DAG with the most
    recent changeset at the top.
    'o' is a changeset, '@' is a working directory parent, 'x' is obsolete,
    and '+' represents a fork where the changeset from the lines below is a
    parent of the 'o' merge on the same line.

    .. note::

       log -p/--patch may generate unexpected diff output for merge
       changesets, as it will only compare the merge changeset against
       its first parent. Also, only files different from BOTH parents
       will appear in files:.

    .. note::

       for performance reasons, log FILE may omit duplicate changes
       made on branches and will not show removals or mode changes. To
       see all such changes, use the --removed switch.

    .. container:: verbose

      Some examples:

      - changesets with full descriptions and file lists::

          hg log -v

      - changesets ancestral to the working directory::

          hg log -f

      - last 10 commits on the current branch::

          hg log -l 10 -b .

      - changesets showing all modifications of a file, including removals::

          hg log --removed file.c

      - all changesets that touch a directory, with diffs, excluding merges::

          hg log -Mp lib/

      - all revision numbers that match a keyword::

          hg log -k bug --template "{rev}\\n"

      - list available log templates::

          hg log -T list

      - check if a given changeset is included in a tagged release::

          hg log -r "a21ccf and ancestor(1.9)"

      - find all changesets by some user in a date range::

          hg log -k alice -d "may 2008 to jul 2008"

      - summary of all changesets after the last tag::

          hg log -r "last(tagged())::" --template "{desc|firstline}\\n"

    See :hg:`help dates` for a list of formats valid for -d/--date.

    See :hg:`help revisions` and :hg:`help revsets` for more about
    specifying revisions.

    See :hg:`help templates` for more about pre-packaged styles and
    specifying custom templates.

    Returns 0 on success.

    """
    if opts.get('follow') and opts.get('rev'):
        opts['rev'] = [revset.formatspec('reverse(::%lr)', opts.get('rev'))]
        del opts['follow']

    if opts.get('graph'):
        return cmdutil.graphlog(ui, repo, *pats, **opts)

    revs, expr, filematcher = cmdutil.getlogrevs(repo, pats, opts)
    limit = cmdutil.loglimit(opts)
    count = 0

    getrenamed = None
    if opts.get('copies'):
        endrev = None
        if opts.get('rev'):
            endrev = scmutil.revrange(repo, opts.get('rev')).max() + 1
        getrenamed = templatekw.getrenamedfn(repo, endrev=endrev)

    displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
    for rev in revs:
        if count == limit:
            break
        ctx = repo[rev]
        copies = None
        if getrenamed is not None and rev:
            copies = []
            for fn in ctx.files():
                rename = getrenamed(fn, rev)
                if rename:
                    copies.append((fn, rename[0]))
        if filematcher:
            revmatchfn = filematcher(ctx.rev())
        else:
            revmatchfn = None
        displayer.show(ctx, copies=copies, matchfn=revmatchfn)
        if displayer.flush(rev):
            count += 1

    displayer.close()

@command('manifest',
    [('r', 'rev', '', _('revision to display'), _('REV')),
     ('', 'all', False, _("list files from all revisions"))]
         + formatteropts,
    _('[-r REV]'))
def manifest(ui, repo, node=None, rev=None, **opts):
    """output the current or given revision of the project manifest

    Print a list of version controlled files for the given revision.
    If no revision is given, the first parent of the working directory
    is used, or the null revision if no revision is checked out.

    With -v, print file permissions, symlink and executable bits.
    With --debug, print file revision hashes.

    If option --all is specified, the list of all files from all revisions
    is printed. This includes deleted and renamed files.

    Returns 0 on success.
    """

    fm = ui.formatter('manifest', opts)

    if opts.get('all'):
        if rev or node:
            raise util.Abort(_("can't specify a revision with --all"))

        res = []
        prefix = "data/"
        suffix = ".i"
        plen = len(prefix)
        slen = len(suffix)
        lock = repo.lock()
        try:
            for fn, b, size in repo.store.datafiles():
                if size != 0 and fn[-slen:] == suffix and fn[:plen] == prefix:
                    res.append(fn[plen:-slen])
        finally:
            lock.release()
        for f in res:
            fm.startitem()
            fm.write("path", '%s\n', f)
        fm.end()
        return

    if rev and node:
        raise util.Abort(_("please specify just one revision"))

    if not node:
        node = rev

    char = {'l': '@', 'x': '*', '': ''}
    mode = {'l': '644', 'x': '755', '': '644'}
    ctx = scmutil.revsingle(repo, node)
    mf = ctx.manifest()
    for f in ctx:
        fm.startitem()
        fl = ctx[f].flags()
        fm.condwrite(ui.debugflag, 'hash', '%s ', hex(mf[f]))
        fm.condwrite(ui.verbose, 'mode type', '%s %1s ', mode[fl], char[fl])
        fm.write('path', '%s\n', f)
    fm.end()

@command('^merge',
    [('f', 'force', None,
      _('force a merge including outstanding changes (DEPRECATED)')),
    ('r', 'rev', '', _('revision to merge'), _('REV')),
    ('P', 'preview', None,
     _('review revisions to merge (no merge is performed)'))
     ] + mergetoolopts,
    _('[-P] [-f] [[-r] REV]'))
def merge(ui, repo, node=None, **opts):
    """merge another revision into working directory

    The current working directory is updated with all changes made in
    the requested revision since the last common predecessor revision.

    Files that changed between either parent are marked as changed for
    the next commit and a commit must be performed before any further
    updates to the repository are allowed. The next commit will have
    two parents.

    ``--tool`` can be used to specify the merge tool used for file
    merges. It overrides the HGMERGE environment variable and your
    configuration files. See :hg:`help merge-tools` for options.

    If no revision is specified, the working directory's parent is a
    head revision, and the current branch contains exactly one other
    head, the other head is merged with by default. Otherwise, an
    explicit revision with which to merge with must be provided.

    :hg:`resolve` must be used to resolve unresolved files.

    To undo an uncommitted merge, use :hg:`update --clean .` which
    will check out a clean copy of the original merge parent, losing
    all changes.

    Returns 0 on success, 1 if there are unresolved files.
    """

    if opts.get('rev') and node:
        raise util.Abort(_("please specify just one revision"))
    if not node:
        node = opts.get('rev')

    if node:
        node = scmutil.revsingle(repo, node).node()

    if not node and repo._activebookmark:
        bmheads = repo.bookmarkheads(repo._activebookmark)
        curhead = repo[repo._activebookmark].node()
        if len(bmheads) == 2:
            if curhead == bmheads[0]:
                node = bmheads[1]
            else:
                node = bmheads[0]
        elif len(bmheads) > 2:
            raise util.Abort(_("multiple matching bookmarks to merge - "
                "please merge with an explicit rev or bookmark"),
                hint=_("run 'hg heads' to see all heads"))
        elif len(bmheads) <= 1:
            raise util.Abort(_("no matching bookmark to merge - "
                "please merge with an explicit rev or bookmark"),
                hint=_("run 'hg heads' to see all heads"))

    if not node and not repo._activebookmark:
        branch = repo[None].branch()
        bheads = repo.branchheads(branch)
        nbhs = [bh for bh in bheads if not repo[bh].bookmarks()]

        if len(nbhs) > 2:
            raise util.Abort(_("branch '%s' has %d heads - "
                               "please merge with an explicit rev")
                             % (branch, len(bheads)),
                             hint=_("run 'hg heads .' to see heads"))

        parent = repo.dirstate.p1()
        if len(nbhs) <= 1:
            if len(bheads) > 1:
                raise util.Abort(_("heads are bookmarked - "
                                   "please merge with an explicit rev"),
                                 hint=_("run 'hg heads' to see all heads"))
            if len(repo.heads()) > 1:
                raise util.Abort(_("branch '%s' has one head - "
                                   "please merge with an explicit rev")
                                 % branch,
                                 hint=_("run 'hg heads' to see all heads"))
            msg, hint = _('nothing to merge'), None
            if parent != repo.lookup(branch):
                hint = _("use 'hg update' instead")
            raise util.Abort(msg, hint=hint)

        if parent not in bheads:
            raise util.Abort(_('working directory not at a head revision'),
                             hint=_("use 'hg update' or merge with an "
                                    "explicit revision"))
        if parent == nbhs[0]:
            node = nbhs[-1]
        else:
            node = nbhs[0]

    if opts.get('preview'):
        # find nodes that are ancestors of p2 but not of p1
        p1 = repo.lookup('.')
        p2 = repo.lookup(node)
        nodes = repo.changelog.findmissing(common=[p1], heads=[p2])

        displayer = cmdutil.show_changeset(ui, repo, opts)
        for node in nodes:
            displayer.show(repo[node])
        displayer.close()
        return 0

    try:
        # ui.forcemerge is an internal variable, do not document
        repo.ui.setconfig('ui', 'forcemerge', opts.get('tool', ''), 'merge')
        return hg.merge(repo, node, force=opts.get('force'))
    finally:
        ui.setconfig('ui', 'forcemerge', '', 'merge')

@command('outgoing|out',
    [('f', 'force', None, _('run even when the destination is unrelated')),
    ('r', 'rev', [],
     _('a changeset intended to be included in the destination'), _('REV')),
    ('n', 'newest-first', None, _('show newest record first')),
    ('B', 'bookmarks', False, _('compare bookmarks')),
    ('b', 'branch', [], _('a specific branch you would like to push'),
     _('BRANCH')),
    ] + logopts + remoteopts + subrepoopts,
    _('[-M] [-p] [-n] [-f] [-r REV]... [DEST]'))
def outgoing(ui, repo, dest=None, **opts):
    """show changesets not found in the destination

    Show changesets not found in the specified destination repository
    or the default push location. These are the changesets that would
    be pushed if a push was requested.

    See pull for details of valid destination formats.

    .. container:: verbose

      With -B/--bookmarks, the result of bookmark comparison between
      local and remote repositories is displayed. With -v/--verbose,
      status is also displayed for each bookmark like below::

        BM1               01234567890a added
        BM2                            deleted
        BM3               234567890abc advanced
        BM4               34567890abcd diverged
        BM5               4567890abcde changed

      The action taken when pushing depends on the
      status of each bookmark:

      :``added``: push with ``-B`` will create it
      :``deleted``: push with ``-B`` will delete it
      :``advanced``: push will update it
      :``diverged``: push with ``-B`` will update it
      :``changed``: push with ``-B`` will update it

      From the point of view of pushing behavior, bookmarks
      existing only in the remote repository are treated as
      ``deleted``, even if it is in fact added remotely.

    Returns 0 if there are outgoing changes, 1 otherwise.
    """
    if opts.get('graph'):
        cmdutil.checkunsupportedgraphflags([], opts)
        o, other = hg._outgoing(ui, repo, dest, opts)
        if not o:
            cmdutil.outgoinghooks(ui, repo, other, opts, o)
            return

        revdag = cmdutil.graphrevs(repo, o, opts)
        displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
        showparents = [ctx.node() for ctx in repo[None].parents()]
        cmdutil.displaygraph(ui, revdag, displayer, showparents,
                             graphmod.asciiedges)
        cmdutil.outgoinghooks(ui, repo, other, opts, o)
        return 0

    if opts.get('bookmarks'):
        dest = ui.expandpath(dest or 'default-push', dest or 'default')
        dest, branches = hg.parseurl(dest, opts.get('branch'))
        other = hg.peer(repo, opts, dest)
        if 'bookmarks' not in other.listkeys('namespaces'):
            ui.warn(_("remote doesn't support bookmarks\n"))
            return 0
        ui.status(_('comparing with %s\n') % util.hidepassword(dest))
        return bookmarks.outgoing(ui, repo, other)

    repo._subtoppath = ui.expandpath(dest or 'default-push', dest or 'default')
    try:
        return hg.outgoing(ui, repo, dest, opts)
    finally:
        del repo._subtoppath

@command('parents',
    [('r', 'rev', '', _('show parents of the specified revision'), _('REV')),
    ] + templateopts,
    _('[-r REV] [FILE]'),
    inferrepo=True)
def parents(ui, repo, file_=None, **opts):
    """show the parents of the working directory or revision (DEPRECATED)

    Print the working directory's parent revisions. If a revision is
    given via -r/--rev, the parent of that revision will be printed.
    If a file argument is given, the revision in which the file was
    last changed (before the working directory revision or the
    argument to --rev if given) is printed.

    See :hg:`summary` and :hg:`help revsets` for related information.

    Returns 0 on success.
    """

    ctx = scmutil.revsingle(repo, opts.get('rev'), None)

    if file_:
        m = scmutil.match(ctx, (file_,), opts)
        if m.anypats() or len(m.files()) != 1:
            raise util.Abort(_('can only specify an explicit filename'))
        file_ = m.files()[0]
        filenodes = []
        for cp in ctx.parents():
            if not cp:
                continue
            try:
                filenodes.append(cp.filenode(file_))
            except error.LookupError:
                pass
        if not filenodes:
            raise util.Abort(_("'%s' not found in manifest!") % file_)
        p = []
        for fn in filenodes:
            fctx = repo.filectx(file_, fileid=fn)
            p.append(fctx.node())
    else:
        p = [cp.node() for cp in ctx.parents()]

    displayer = cmdutil.show_changeset(ui, repo, opts)
    for n in p:
        if n != nullid:
            displayer.show(repo[n])
    displayer.close()

@command('paths', [], _('[NAME]'), optionalrepo=True)
def paths(ui, repo, search=None):
    """show aliases for remote repositories

    Show definition of symbolic path name NAME. If no name is given,
    show definition of all available names.

    Option -q/--quiet suppresses all output when searching for NAME
    and shows only the path names when listing all definitions.

    Path names are defined in the [paths] section of your
    configuration file and in ``/etc/mercurial/hgrc``. If run inside a
    repository, ``.hg/hgrc`` is used, too.

    The path names ``default`` and ``default-push`` have a special
    meaning.  When performing a push or pull operation, they are used
    as fallbacks if no location is specified on the command-line.
    When ``default-push`` is set, it will be used for push and
    ``default`` will be used for pull; otherwise ``default`` is used
    as the fallback for both.  When cloning a repository, the clone
    source is written as ``default`` in ``.hg/hgrc``.  Note that
    ``default`` and ``default-push`` apply to all inbound (e.g.
    :hg:`incoming`) and outbound (e.g. :hg:`outgoing`, :hg:`email` and
    :hg:`bundle`) operations.

    See :hg:`help urls` for more information.

    Returns 0 on success.
    """
    if search:
        for name, path in sorted(ui.paths.iteritems()):
            if name == search:
                ui.status("%s\n" % util.hidepassword(path.loc))
                return
        if not ui.quiet:
            ui.warn(_("not found!\n"))
        return 1
    else:
        for name, path in sorted(ui.paths.iteritems()):
            if ui.quiet:
                ui.write("%s\n" % name)
            else:
                ui.write("%s = %s\n" % (name,
                                        util.hidepassword(path.loc)))

@command('phase',
    [('p', 'public', False, _('set changeset phase to public')),
     ('d', 'draft', False, _('set changeset phase to draft')),
     ('s', 'secret', False, _('set changeset phase to secret')),
     ('f', 'force', False, _('allow to move boundary backward')),
     ('r', 'rev', [], _('target revision'), _('REV')),
    ],
    _('[-p|-d|-s] [-f] [-r] [REV...]'))
def phase(ui, repo, *revs, **opts):
    """set or show the current phase name

    With no argument, show the phase name of the current revision(s).

    With one of -p/--public, -d/--draft or -s/--secret, change the
    phase value of the specified revisions.

    Unless -f/--force is specified, :hg:`phase` won't move changeset from a
    lower phase to an higher phase. Phases are ordered as follows::

        public < draft < secret

    Returns 0 on success, 1 if no phases were changed or some could not
    be changed.

    (For more information about the phases concept, see :hg:`help phases`.)
    """
    # search for a unique phase argument
    targetphase = None
    for idx, name in enumerate(phases.phasenames):
        if opts[name]:
            if targetphase is not None:
                raise util.Abort(_('only one phase can be specified'))
            targetphase = idx

    # look for specified revision
    revs = list(revs)
    revs.extend(opts['rev'])
    if not revs:
        # display both parents as the second parent phase can influence
        # the phase of a merge commit
        revs = [c.rev() for c in repo[None].parents()]

    revs = scmutil.revrange(repo, revs)

    lock = None
    ret = 0
    if targetphase is None:
        # display
        for r in revs:
            ctx = repo[r]
            ui.write('%i: %s\n' % (ctx.rev(), ctx.phasestr()))
    else:
        tr = None
        lock = repo.lock()
        try:
            tr = repo.transaction("phase")
            # set phase
            if not revs:
                raise util.Abort(_('empty revision set'))
            nodes = [repo[r].node() for r in revs]
            # moving revision from public to draft may hide them
            # We have to check result on an unfiltered repository
            unfi = repo.unfiltered()
            getphase = unfi._phasecache.phase
            olddata = [getphase(unfi, r) for r in unfi]
            phases.advanceboundary(repo, tr, targetphase, nodes)
            if opts['force']:
                phases.retractboundary(repo, tr, targetphase, nodes)
            tr.close()
        finally:
            if tr is not None:
                tr.release()
            lock.release()
        getphase = unfi._phasecache.phase
        newdata = [getphase(unfi, r) for r in unfi]
        changes = sum(newdata[r] != olddata[r] for r in unfi)
        cl = unfi.changelog
        rejected = [n for n in nodes
                    if newdata[cl.rev(n)] < targetphase]
        if rejected:
            ui.warn(_('cannot move %i changesets to a higher '
                      'phase, use --force\n') % len(rejected))
            ret = 1
        if changes:
            msg = _('phase changed for %i changesets\n') % changes
            if ret:
                ui.status(msg)
            else:
                ui.note(msg)
        else:
            ui.warn(_('no phases changed\n'))
            ret = 1
    return ret

def postincoming(ui, repo, modheads, optupdate, checkout):
    if modheads == 0:
        return
    if optupdate:
        checkout, movemarkfrom = bookmarks.calculateupdate(ui, repo, checkout)
        try:
            ret = hg.update(repo, checkout)
        except util.Abort as inst:
            ui.warn(_("not updating: %s\n") % str(inst))
            if inst.hint:
                ui.warn(_("(%s)\n") % inst.hint)
            return 0
        if not ret and not checkout:
            if bookmarks.update(repo, [movemarkfrom], repo['.'].node()):
                ui.status(_("updating bookmark %s\n") % repo._activebookmark)
        return ret
    if modheads > 1:
        currentbranchheads = len(repo.branchheads())
        if currentbranchheads == modheads:
            ui.status(_("(run 'hg heads' to see heads, 'hg merge' to merge)\n"))
        elif currentbranchheads > 1:
            ui.status(_("(run 'hg heads .' to see heads, 'hg merge' to "
                        "merge)\n"))
        else:
            ui.status(_("(run 'hg heads' to see heads)\n"))
    else:
        ui.status(_("(run 'hg update' to get a working copy)\n"))

@command('^pull',
    [('u', 'update', None,
     _('update to new branch head if changesets were pulled')),
    ('f', 'force', None, _('run even when remote repository is unrelated')),
    ('r', 'rev', [], _('a remote changeset intended to be added'), _('REV')),
    ('B', 'bookmark', [], _("bookmark to pull"), _('BOOKMARK')),
    ('b', 'branch', [], _('a specific branch you would like to pull'),
     _('BRANCH')),
    ] + remoteopts,
    _('[-u] [-f] [-r REV]... [-e CMD] [--remotecmd CMD] [SOURCE]'))
def pull(ui, repo, source="default", **opts):
    """pull changes from the specified source

    Pull changes from a remote repository to a local one.

    This finds all changes from the repository at the specified path
    or URL and adds them to a local repository (the current one unless
    -R is specified). By default, this does not update the copy of the
    project in the working directory.

    Use :hg:`incoming` if you want to see what would have been added
    by a pull at the time you issued this command. If you then decide
    to add those changes to the repository, you should use :hg:`pull
    -r X` where ``X`` is the last changeset listed by :hg:`incoming`.

    If SOURCE is omitted, the 'default' path will be used.
    See :hg:`help urls` for more information.

    Returns 0 on success, 1 if an update had unresolved files.
    """
    source, branches = hg.parseurl(ui.expandpath(source), opts.get('branch'))
    ui.status(_('pulling from %s\n') % util.hidepassword(source))
    other = hg.peer(repo, opts, source)
    try:
        revs, checkout = hg.addbranchrevs(repo, other, branches,
                                          opts.get('rev'))


        pullopargs = {}
        if opts.get('bookmark'):
            if not revs:
                revs = []
            # The list of bookmark used here is not the one used to actually
            # update the bookmark name. This can result in the revision pulled
            # not ending up with the name of the bookmark because of a race
            # condition on the server. (See issue 4689 for details)
            remotebookmarks = other.listkeys('bookmarks')
            pullopargs['remotebookmarks'] = remotebookmarks
            for b in opts['bookmark']:
                if b not in remotebookmarks:
                    raise util.Abort(_('remote bookmark %s not found!') % b)
                revs.append(remotebookmarks[b])

        if revs:
            try:
                # When 'rev' is a bookmark name, we cannot guarantee that it
                # will be updated with that name because of a race condition
                # server side. (See issue 4689 for details)
                oldrevs = revs
                revs = [] # actually, nodes
                for r in oldrevs:
                    node = other.lookup(r)
                    revs.append(node)
                    if r == checkout:
                        checkout = node
            except error.CapabilityError:
                err = _("other repository doesn't support revision lookup, "
                        "so a rev cannot be specified.")
                raise util.Abort(err)

        modheads = exchange.pull(repo, other, heads=revs,
                                 force=opts.get('force'),
                                 bookmarks=opts.get('bookmark', ()),
                                 opargs=pullopargs).cgresult
        if checkout:
            checkout = str(repo.changelog.rev(checkout))
        repo._subtoppath = source
        try:
            ret = postincoming(ui, repo, modheads, opts.get('update'), checkout)

        finally:
            del repo._subtoppath

    finally:
        other.close()
    return ret

@command('^push',
    [('f', 'force', None, _('force push')),
    ('r', 'rev', [],
     _('a changeset intended to be included in the destination'),
     _('REV')),
    ('B', 'bookmark', [], _("bookmark to push"), _('BOOKMARK')),
    ('b', 'branch', [],
     _('a specific branch you would like to push'), _('BRANCH')),
    ('', 'new-branch', False, _('allow pushing a new branch')),
    ] + remoteopts,
    _('[-f] [-r REV]... [-e CMD] [--remotecmd CMD] [DEST]'))
def push(ui, repo, dest=None, **opts):
    """push changes to the specified destination

    Push changesets from the local repository to the specified
    destination.

    This operation is symmetrical to pull: it is identical to a pull
    in the destination repository from the current one.

    By default, push will not allow creation of new heads at the
    destination, since multiple heads would make it unclear which head
    to use. In this situation, it is recommended to pull and merge
    before pushing.

    Use --new-branch if you want to allow push to create a new named
    branch that is not present at the destination. This allows you to
    only create a new branch without forcing other changes.

    .. note::

      Extra care should be taken with the -f/--force option,
      which will push all new heads on all branches, an action which will
      almost always cause confusion for collaborators.

    If -r/--rev is used, the specified revision and all its ancestors
    will be pushed to the remote repository.

    If -B/--bookmark is used, the specified bookmarked revision, its
    ancestors, and the bookmark will be pushed to the remote
    repository.

    Please see :hg:`help urls` for important details about ``ssh://``
    URLs. If DESTINATION is omitted, a default path will be used.

    Returns 0 if push was successful, 1 if nothing to push.
    """

    if opts.get('bookmark'):
        ui.setconfig('bookmarks', 'pushing', opts['bookmark'], 'push')
        for b in opts['bookmark']:
            # translate -B options to -r so changesets get pushed
            if b in repo._bookmarks:
                opts.setdefault('rev', []).append(b)
            else:
                # if we try to push a deleted bookmark, translate it to null
                # this lets simultaneous -r, -b options continue working
                opts.setdefault('rev', []).append("null")

    dest = ui.expandpath(dest or 'default-push', dest or 'default')
    dest, branches = hg.parseurl(dest, opts.get('branch'))
    ui.status(_('pushing to %s\n') % util.hidepassword(dest))
    revs, checkout = hg.addbranchrevs(repo, repo, branches, opts.get('rev'))
    try:
        other = hg.peer(repo, opts, dest)
    except error.RepoError:
        if dest == "default-push":
            raise util.Abort(_("default repository not configured!"),
                    hint=_('see the "path" section in "hg help config"'))
        else:
            raise

    if revs:
        revs = [repo.lookup(r) for r in scmutil.revrange(repo, revs)]
        if not revs:
            raise util.Abort(_("specified revisions evaluate to an empty set"),
                             hint=_("use different revision arguments"))

    repo._subtoppath = dest
    try:
        # push subrepos depth-first for coherent ordering
        c = repo['']
        subs = c.substate # only repos that are committed
        for s in sorted(subs):
            result = c.sub(s).push(opts)
            if result == 0:
                return not result
    finally:
        del repo._subtoppath
    pushop = exchange.push(repo, other, opts.get('force'), revs=revs,
                           newbranch=opts.get('new_branch'),
                           bookmarks=opts.get('bookmark', ()))

    result = not pushop.cgresult

    if pushop.bkresult is not None:
        if pushop.bkresult == 2:
            result = 2
        elif not result and pushop.bkresult:
            result = 2

    return result

@command('recover', [])
def recover(ui, repo):
    """roll back an interrupted transaction

    Recover from an interrupted commit or pull.

    This command tries to fix the repository status after an
    interrupted operation. It should only be necessary when Mercurial
    suggests it.

    Returns 0 if successful, 1 if nothing to recover or verify fails.
    """
    if repo.recover():
        return hg.verify(repo)
    return 1

@command('^remove|rm',
    [('A', 'after', None, _('record delete for missing files')),
    ('f', 'force', None,
     _('remove (and delete) file even if added or modified')),
    ] + subrepoopts + walkopts,
    _('[OPTION]... FILE...'),
    inferrepo=True)
def remove(ui, repo, *pats, **opts):
    """remove the specified files on the next commit

    Schedule the indicated files for removal from the current branch.

    This command schedules the files to be removed at the next commit.
    To undo a remove before that, see :hg:`revert`. To undo added
    files, see :hg:`forget`.

    .. container:: verbose

      -A/--after can be used to remove only files that have already
      been deleted, -f/--force can be used to force deletion, and -Af
      can be used to remove files from the next revision without
      deleting them from the working directory.

      The following table details the behavior of remove for different
      file states (columns) and option combinations (rows). The file
      states are Added [A], Clean [C], Modified [M] and Missing [!]
      (as reported by :hg:`status`). The actions are Warn, Remove
      (from branch) and Delete (from disk):

      ========= == == == ==
      opt/state A  C  M  !
      ========= == == == ==
      none      W  RD W  R
      -f        R  RD RD R
      -A        W  W  W  R
      -Af       R  R  R  R
      ========= == == == ==

      Note that remove never deletes files in Added [A] state from the
      working directory, not even if option --force is specified.

    Returns 0 on success, 1 if any warnings encountered.
    """

    after, force = opts.get('after'), opts.get('force')
    if not pats and not after:
        raise util.Abort(_('no files specified'))

    m = scmutil.match(repo[None], pats, opts)
    subrepos = opts.get('subrepos')
    return cmdutil.remove(ui, repo, m, "", after, force, subrepos)

@command('rename|move|mv',
    [('A', 'after', None, _('record a rename that has already occurred')),
    ('f', 'force', None, _('forcibly copy over an existing managed file')),
    ] + walkopts + dryrunopts,
    _('[OPTION]... SOURCE... DEST'))
def rename(ui, repo, *pats, **opts):
    """rename files; equivalent of copy + remove

    Mark dest as copies of sources; mark sources for deletion. If dest
    is a directory, copies are put in that directory. If dest is a
    file, there can only be one source.

    By default, this command copies the contents of files as they
    exist in the working directory. If invoked with -A/--after, the
    operation is recorded, but no copying is performed.

    This command takes effect at the next commit. To undo a rename
    before that, see :hg:`revert`.

    Returns 0 on success, 1 if errors are encountered.
    """
    wlock = repo.wlock(False)
    try:
        return cmdutil.copy(ui, repo, pats, opts, rename=True)
    finally:
        wlock.release()

@command('resolve',
    [('a', 'all', None, _('select all unresolved files')),
    ('l', 'list', None, _('list state of files needing merge')),
    ('m', 'mark', None, _('mark files as resolved')),
    ('u', 'unmark', None, _('mark files as unresolved')),
    ('n', 'no-status', None, _('hide status prefix'))]
    + mergetoolopts + walkopts + formatteropts,
    _('[OPTION]... [FILE]...'),
    inferrepo=True)
def resolve(ui, repo, *pats, **opts):
    """redo merges or set/view the merge status of files

    Merges with unresolved conflicts are often the result of
    non-interactive merging using the ``internal:merge`` configuration
    setting, or a command-line merge tool like ``diff3``. The resolve
    command is used to manage the files involved in a merge, after
    :hg:`merge` has been run, and before :hg:`commit` is run (i.e. the
    working directory must have two parents). See :hg:`help
    merge-tools` for information on configuring merge tools.

    The resolve command can be used in the following ways:

    - :hg:`resolve [--tool TOOL] FILE...`: attempt to re-merge the specified
      files, discarding any previous merge attempts. Re-merging is not
      performed for files already marked as resolved. Use ``--all/-a``
      to select all unresolved files. ``--tool`` can be used to specify
      the merge tool used for the given files. It overrides the HGMERGE
      environment variable and your configuration files.  Previous file
      contents are saved with a ``.orig`` suffix.

    - :hg:`resolve -m [FILE]`: mark a file as having been resolved
      (e.g. after having manually fixed-up the files). The default is
      to mark all unresolved files.

    - :hg:`resolve -u [FILE]...`: mark a file as unresolved. The
      default is to mark all resolved files.

    - :hg:`resolve -l`: list files which had or still have conflicts.
      In the printed list, ``U`` = unresolved and ``R`` = resolved.

    Note that Mercurial will not let you commit files with unresolved
    merge conflicts. You must use :hg:`resolve -m ...` before you can
    commit after a conflicting merge.

    Returns 0 on success, 1 if any files fail a resolve attempt.
    """

    all, mark, unmark, show, nostatus = \
        [opts.get(o) for o in 'all mark unmark list no_status'.split()]

    if (show and (mark or unmark)) or (mark and unmark):
        raise util.Abort(_("too many options specified"))
    if pats and all:
        raise util.Abort(_("can't specify --all and patterns"))
    if not (all or pats or show or mark or unmark):
        raise util.Abort(_('no files or directories specified'),
                         hint=('use --all to remerge all files'))

    if show:
        fm = ui.formatter('resolve', opts)
        ms = mergemod.mergestate(repo)
        m = scmutil.match(repo[None], pats, opts)
        for f in ms:
            if not m(f):
                continue
            l = 'resolve.' + {'u': 'unresolved', 'r': 'resolved'}[ms[f]]
            fm.startitem()
            fm.condwrite(not nostatus, 'status', '%s ', ms[f].upper(), label=l)
            fm.write('path', '%s\n', f, label=l)
        fm.end()
        return 0

    wlock = repo.wlock()
    try:
        ms = mergemod.mergestate(repo)

        if not (ms.active() or repo.dirstate.p2() != nullid):
            raise util.Abort(
                _('resolve command not applicable when not merging'))

        m = scmutil.match(repo[None], pats, opts)
        ret = 0
        didwork = False

        for f in ms:
            if not m(f):
                continue

            didwork = True

            if mark:
                ms.mark(f, "r")
            elif unmark:
                ms.mark(f, "u")
            else:
                wctx = repo[None]

                # backup pre-resolve (merge uses .orig for its own purposes)
                a = repo.wjoin(f)
                util.copyfile(a, a + ".resolve")

                try:
                    # resolve file
                    ui.setconfig('ui', 'forcemerge', opts.get('tool', ''),
                                 'resolve')
                    if ms.resolve(f, wctx):
                        ret = 1
                finally:
                    ui.setconfig('ui', 'forcemerge', '', 'resolve')
                    ms.commit()

                # replace filemerge's .orig file with our resolve file
                util.rename(a + ".resolve", a + ".orig")

        ms.commit()

        if not didwork and pats:
            ui.warn(_("arguments do not match paths that need resolving\n"))

    finally:
        wlock.release()

    # Nudge users into finishing an unfinished operation
    if not list(ms.unresolved()):
        ui.status(_('(no more unresolved files)\n'))

    return ret

@command('revert',
    [('a', 'all', None, _('revert all changes when no arguments given')),
    ('d', 'date', '', _('tipmost revision matching date'), _('DATE')),
    ('r', 'rev', '', _('revert to the specified revision'), _('REV')),
    ('C', 'no-backup', None, _('do not save backup copies of files')),
    ('i', 'interactive', None,
            _('interactively select the changes (EXPERIMENTAL)')),
    ] + walkopts + dryrunopts,
    _('[OPTION]... [-r REV] [NAME]...'))
def revert(ui, repo, *pats, **opts):
    """restore files to their checkout state

    .. note::

       To check out earlier revisions, you should use :hg:`update REV`.
       To cancel an uncommitted merge (and lose your changes),
       use :hg:`update --clean .`.

    With no revision specified, revert the specified files or directories
    to the contents they had in the parent of the working directory.
    This restores the contents of files to an unmodified
    state and unschedules adds, removes, copies, and renames. If the
    working directory has two parents, you must explicitly specify a
    revision.

    Using the -r/--rev or -d/--date options, revert the given files or
    directories to their states as of a specific revision. Because
    revert does not change the working directory parents, this will
    cause these files to appear modified. This can be helpful to "back
    out" some or all of an earlier change. See :hg:`backout` for a
    related method.

    Modified files are saved with a .orig suffix before reverting.
    To disable these backups, use --no-backup.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    Returns 0 on success.
    """

    if opts.get("date"):
        if opts.get("rev"):
            raise util.Abort(_("you can't specify a revision and a date"))
        opts["rev"] = cmdutil.finddate(ui, repo, opts["date"])

    parent, p2 = repo.dirstate.parents()
    if not opts.get('rev') and p2 != nullid:
        # revert after merge is a trap for new users (issue2915)
        raise util.Abort(_('uncommitted merge with no revision specified'),
                         hint=_('use "hg update" or see "hg help revert"'))

    ctx = scmutil.revsingle(repo, opts.get('rev'))

    if (not (pats or opts.get('include') or opts.get('exclude') or
             opts.get('all') or opts.get('interactive'))):
        msg = _("no files or directories specified")
        if p2 != nullid:
            hint = _("uncommitted merge, use --all to discard all changes,"
                     " or 'hg update -C .' to abort the merge")
            raise util.Abort(msg, hint=hint)
        dirty = any(repo.status())
        node = ctx.node()
        if node != parent:
            if dirty:
                hint = _("uncommitted changes, use --all to discard all"
                         " changes, or 'hg update %s' to update") % ctx.rev()
            else:
                hint = _("use --all to revert all files,"
                         " or 'hg update %s' to update") % ctx.rev()
        elif dirty:
            hint = _("uncommitted changes, use --all to discard all changes")
        else:
            hint = _("use --all to revert all files")
        raise util.Abort(msg, hint=hint)

    return cmdutil.revert(ui, repo, ctx, (parent, p2), *pats, **opts)

@command('rollback', dryrunopts +
         [('f', 'force', False, _('ignore safety measures'))])
def rollback(ui, repo, **opts):
    """roll back the last transaction (DANGEROUS) (DEPRECATED)

    Please use :hg:`commit --amend` instead of rollback to correct
    mistakes in the last commit.

    This command should be used with care. There is only one level of
    rollback, and there is no way to undo a rollback. It will also
    restore the dirstate at the time of the last transaction, losing
    any dirstate changes since that time. This command does not alter
    the working directory.

    Transactions are used to encapsulate the effects of all commands
    that create new changesets or propagate existing changesets into a
    repository.

    .. container:: verbose

      For example, the following commands are transactional, and their
      effects can be rolled back:

      - commit
      - import
      - pull
      - push (with this repository as the destination)
      - unbundle

      To avoid permanent data loss, rollback will refuse to rollback a
      commit transaction if it isn't checked out. Use --force to
      override this protection.

    This command is not intended for use on public repositories. Once
    changes are visible for pull by other users, rolling a transaction
    back locally is ineffective (someone else may already have pulled
    the changes). Furthermore, a race is possible with readers of the
    repository; for example an in-progress pull from the repository
    may fail if a rollback is performed.

    Returns 0 on success, 1 if no rollback data is available.
    """
    return repo.rollback(dryrun=opts.get('dry_run'),
                         force=opts.get('force'))

@command('root', [])
def root(ui, repo):
    """print the root (top) of the current working directory

    Print the root directory of the current repository.

    Returns 0 on success.
    """
    ui.write(repo.root + "\n")

@command('^serve',
    [('A', 'accesslog', '', _('name of access log file to write to'),
     _('FILE')),
    ('d', 'daemon', None, _('run server in background')),
    ('', 'daemon-pipefds', '', _('used internally by daemon mode'), _('FILE')),
    ('E', 'errorlog', '', _('name of error log file to write to'), _('FILE')),
    # use string type, then we can check if something was passed
    ('p', 'port', '', _('port to listen on (default: 8000)'), _('PORT')),
    ('a', 'address', '', _('address to listen on (default: all interfaces)'),
     _('ADDR')),
    ('', 'prefix', '', _('prefix path to serve from (default: server root)'),
     _('PREFIX')),
    ('n', 'name', '',
     _('name to show in web pages (default: working directory)'), _('NAME')),
    ('', 'web-conf', '',
     _('name of the hgweb config file (see "hg help hgweb")'), _('FILE')),
    ('', 'webdir-conf', '', _('name of the hgweb config file (DEPRECATED)'),
     _('FILE')),
    ('', 'pid-file', '', _('name of file to write process ID to'), _('FILE')),
    ('', 'stdio', None, _('for remote clients')),
    ('', 'cmdserver', '', _('for remote clients'), _('MODE')),
    ('t', 'templates', '', _('web templates to use'), _('TEMPLATE')),
    ('', 'style', '', _('template style to use'), _('STYLE')),
    ('6', 'ipv6', None, _('use IPv6 in addition to IPv4')),
    ('', 'certificate', '', _('SSL certificate file'), _('FILE'))],
    _('[OPTION]...'),
    optionalrepo=True)
def serve(ui, repo, **opts):
    """start stand-alone webserver

    Start a local HTTP repository browser and pull server. You can use
    this for ad-hoc sharing and browsing of repositories. It is
    recommended to use a real web server to serve a repository for
    longer periods of time.

    Please note that the server does not implement access control.
    This means that, by default, anybody can read from the server and
    nobody can write to it by default. Set the ``web.allow_push``
    option to ``*`` to allow everybody to push to the server. You
    should use a real web server if you need to authenticate users.

    By default, the server logs accesses to stdout and errors to
    stderr. Use the -A/--accesslog and -E/--errorlog options to log to
    files.

    To have the server choose a free port number to listen on, specify
    a port number of 0; in this case, the server will print the port
    number it uses.

    Returns 0 on success.
    """

    if opts["stdio"] and opts["cmdserver"]:
        raise util.Abort(_("cannot use --stdio with --cmdserver"))

    if opts["stdio"]:
        if repo is None:
            raise error.RepoError(_("there is no Mercurial repository here"
                                    " (.hg not found)"))
        s = sshserver.sshserver(ui, repo)
        s.serve_forever()

    if opts["cmdserver"]:
        service = commandserver.createservice(ui, repo, opts)
        return cmdutil.service(opts, initfn=service.init, runfn=service.run)

    # this way we can check if something was given in the command-line
    if opts.get('port'):
        opts['port'] = util.getport(opts.get('port'))

    if repo:
        baseui = repo.baseui
    else:
        baseui = ui
    optlist = ("name templates style address port prefix ipv6"
               " accesslog errorlog certificate encoding")
    for o in optlist.split():
        val = opts.get(o, '')
        if val in (None, ''): # should check against default options instead
            continue
        baseui.setconfig("web", o, val, 'serve')
        if repo and repo.ui != baseui:
            repo.ui.setconfig("web", o, val, 'serve')

    o = opts.get('web_conf') or opts.get('webdir_conf')
    if not o:
        if not repo:
            raise error.RepoError(_("there is no Mercurial repository"
                                    " here (.hg not found)"))
        o = repo

    app = hgweb.hgweb(o, baseui=baseui)
    service = httpservice(ui, app, opts)
    cmdutil.service(opts, initfn=service.init, runfn=service.run)

class httpservice(object):
    def __init__(self, ui, app, opts):
        self.ui = ui
        self.app = app
        self.opts = opts

    def init(self):
        util.setsignalhandler()
        self.httpd = hgweb_server.create_server(self.ui, self.app)

        if self.opts['port'] and not self.ui.verbose:
            return

        if self.httpd.prefix:
            prefix = self.httpd.prefix.strip('/') + '/'
        else:
            prefix = ''

        port = ':%d' % self.httpd.port
        if port == ':80':
            port = ''

        bindaddr = self.httpd.addr
        if bindaddr == '0.0.0.0':
            bindaddr = '*'
        elif ':' in bindaddr: # IPv6
            bindaddr = '[%s]' % bindaddr

        fqaddr = self.httpd.fqaddr
        if ':' in fqaddr:
            fqaddr = '[%s]' % fqaddr
        if self.opts['port']:
            write = self.ui.status
        else:
            write = self.ui.write
        write(_('listening at http://%s%s/%s (bound to %s:%d)\n') %
              (fqaddr, port, prefix, bindaddr, self.httpd.port))
        self.ui.flush()  # avoid buffering of status message

    def run(self):
        self.httpd.serve_forever()


@command('^status|st',
    [('A', 'all', None, _('show status of all files')),
    ('m', 'modified', None, _('show only modified files')),
    ('a', 'added', None, _('show only added files')),
    ('r', 'removed', None, _('show only removed files')),
    ('d', 'deleted', None, _('show only deleted (but tracked) files')),
    ('c', 'clean', None, _('show only files without changes')),
    ('u', 'unknown', None, _('show only unknown (not tracked) files')),
    ('i', 'ignored', None, _('show only ignored files')),
    ('n', 'no-status', None, _('hide status prefix')),
    ('C', 'copies', None, _('show source of copied files')),
    ('0', 'print0', None, _('end filenames with NUL, for use with xargs')),
    ('', 'rev', [], _('show difference from revision'), _('REV')),
    ('', 'change', '', _('list the changed files of a revision'), _('REV')),
    ] + walkopts + subrepoopts + formatteropts,
    _('[OPTION]... [FILE]...'),
    inferrepo=True)
def status(ui, repo, *pats, **opts):
    """show changed files in the working directory

    Show status of files in the repository. If names are given, only
    files that match are shown. Files that are clean or ignored or
    the source of a copy/move operation, are not listed unless
    -c/--clean, -i/--ignored, -C/--copies or -A/--all are given.
    Unless options described with "show only ..." are given, the
    options -mardu are used.

    Option -q/--quiet hides untracked (unknown and ignored) files
    unless explicitly requested with -u/--unknown or -i/--ignored.

    .. note::

       status may appear to disagree with diff if permissions have
       changed or a merge has occurred. The standard diff format does
       not report permission changes and diff only reports changes
       relative to one merge parent.

    If one revision is given, it is used as the base revision.
    If two revisions are given, the differences between them are
    shown. The --change option can also be used as a shortcut to list
    the changed files of a revision from its first parent.

    The codes used to show the status of files are::

      M = modified
      A = added
      R = removed
      C = clean
      ! = missing (deleted by non-hg command, but still tracked)
      ? = not tracked
      I = ignored
        = origin of the previous file (with --copies)

    .. container:: verbose

      Examples:

      - show changes in the working directory relative to a
        changeset::

          hg status --rev 9353

      - show changes in the working directory relative to the
        current directory (see :hg:`help patterns` for more information)::

          hg status re:

      - show all changes including copies in an existing changeset::

          hg status --copies --change 9353

      - get a NUL separated list of added files, suitable for xargs::

          hg status -an0

    Returns 0 on success.
    """

    revs = opts.get('rev')
    change = opts.get('change')

    if revs and change:
        msg = _('cannot specify --rev and --change at the same time')
        raise util.Abort(msg)
    elif change:
        node2 = scmutil.revsingle(repo, change, None).node()
        node1 = repo[node2].p1().node()
    else:
        node1, node2 = scmutil.revpair(repo, revs)

    if pats:
        cwd = repo.getcwd()
    else:
        cwd = ''

    if opts.get('print0'):
        end = '\0'
    else:
        end = '\n'
    copy = {}
    states = 'modified added removed deleted unknown ignored clean'.split()
    show = [k for k in states if opts.get(k)]
    if opts.get('all'):
        show += ui.quiet and (states[:4] + ['clean']) or states
    if not show:
        if ui.quiet:
            show = states[:4]
        else:
            show = states[:5]

    m = scmutil.match(repo[node2], pats, opts)
    stat = repo.status(node1, node2, m,
                       'ignored' in show, 'clean' in show, 'unknown' in show,
                       opts.get('subrepos'))
    changestates = zip(states, 'MAR!?IC', stat)

    if (opts.get('all') or opts.get('copies')
        or ui.configbool('ui', 'statuscopies')) and not opts.get('no_status'):
        copy = copies.pathcopies(repo[node1], repo[node2], m)

    fm = ui.formatter('status', opts)
    fmt = '%s' + end
    showchar = not opts.get('no_status')

    for state, char, files in changestates:
        if state in show:
            label = 'status.' + state
            for f in files:
                fm.startitem()
                fm.condwrite(showchar, 'status', '%s ', char, label=label)
                fm.write('path', fmt, repo.pathto(f, cwd), label=label)
                if f in copy:
                    fm.write("copy", '  %s' + end, repo.pathto(copy[f], cwd),
                             label='status.copied')
    fm.end()

@command('^summary|sum',
    [('', 'remote', None, _('check for push and pull'))], '[--remote]')
def summary(ui, repo, **opts):
    """summarize working directory state

    This generates a brief summary of the working directory state,
    including parents, branch, commit status, phase and available updates.

    With the --remote option, this will check the default paths for
    incoming and outgoing changes. This can be time-consuming.

    Returns 0 on success.
    """

    ctx = repo[None]
    parents = ctx.parents()
    pnode = parents[0].node()
    marks = []

    for p in parents:
        # label with log.changeset (instead of log.parent) since this
        # shows a working directory parent *changeset*:
        # i18n: column positioning for "hg summary"
        ui.write(_('parent: %d:%s ') % (p.rev(), str(p)),
                 label='log.changeset changeset.%s' % p.phasestr())
        ui.write(' '.join(p.tags()), label='log.tag')
        if p.bookmarks():
            marks.extend(p.bookmarks())
        if p.rev() == -1:
            if not len(repo):
                ui.write(_(' (empty repository)'))
            else:
                ui.write(_(' (no revision checked out)'))
        ui.write('\n')
        if p.description():
            ui.status(' ' + p.description().splitlines()[0].strip() + '\n',
                      label='log.summary')

    branch = ctx.branch()
    bheads = repo.branchheads(branch)
    # i18n: column positioning for "hg summary"
    m = _('branch: %s\n') % branch
    if branch != 'default':
        ui.write(m, label='log.branch')
    else:
        ui.status(m, label='log.branch')

    if marks:
        active = repo._activebookmark
        # i18n: column positioning for "hg summary"
        ui.write(_('bookmarks:'), label='log.bookmark')
        if active is not None:
            if active in marks:
                ui.write(' *' + active, label=activebookmarklabel)
                marks.remove(active)
            else:
                ui.write(' [%s]' % active, label=activebookmarklabel)
        for m in marks:
            ui.write(' ' + m, label='log.bookmark')
        ui.write('\n', label='log.bookmark')

    status = repo.status(unknown=True)

    c = repo.dirstate.copies()
    copied, renamed = [], []
    for d, s in c.iteritems():
        if s in status.removed:
            status.removed.remove(s)
            renamed.append(d)
        else:
            copied.append(d)
        if d in status.added:
            status.added.remove(d)

    ms = mergemod.mergestate(repo)
    unresolved = [f for f in ms if ms[f] == 'u']

    subs = [s for s in ctx.substate if ctx.sub(s).dirty()]

    labels = [(ui.label(_('%d modified'), 'status.modified'), status.modified),
              (ui.label(_('%d added'), 'status.added'), status.added),
              (ui.label(_('%d removed'), 'status.removed'), status.removed),
              (ui.label(_('%d renamed'), 'status.copied'), renamed),
              (ui.label(_('%d copied'), 'status.copied'), copied),
              (ui.label(_('%d deleted'), 'status.deleted'), status.deleted),
              (ui.label(_('%d unknown'), 'status.unknown'), status.unknown),
              (ui.label(_('%d unresolved'), 'resolve.unresolved'), unresolved),
              (ui.label(_('%d subrepos'), 'status.modified'), subs)]
    t = []
    for l, s in labels:
        if s:
            t.append(l % len(s))

    t = ', '.join(t)
    cleanworkdir = False

    if repo.vfs.exists('updatestate'):
        t += _(' (interrupted update)')
    elif len(parents) > 1:
        t += _(' (merge)')
    elif branch != parents[0].branch():
        t += _(' (new branch)')
    elif (parents[0].closesbranch() and
          pnode in repo.branchheads(branch, closed=True)):
        t += _(' (head closed)')
    elif not (status.modified or status.added or status.removed or renamed or
              copied or subs):
        t += _(' (clean)')
        cleanworkdir = True
    elif pnode not in bheads:
        t += _(' (new branch head)')

    if parents:
        pendingphase = max(p.phase() for p in parents)
    else:
        pendingphase = phases.public

    if pendingphase > phases.newcommitphase(ui):
        t += ' (%s)' % phases.phasenames[pendingphase]

    if cleanworkdir:
        # i18n: column positioning for "hg summary"
        ui.status(_('commit: %s\n') % t.strip())
    else:
        # i18n: column positioning for "hg summary"
        ui.write(_('commit: %s\n') % t.strip())

    # all ancestors of branch heads - all ancestors of parent = new csets
    new = len(repo.changelog.findmissing([pctx.node() for pctx in parents],
                                         bheads))

    if new == 0:
        # i18n: column positioning for "hg summary"
        ui.status(_('update: (current)\n'))
    elif pnode not in bheads:
        # i18n: column positioning for "hg summary"
        ui.write(_('update: %d new changesets (update)\n') % new)
    else:
        # i18n: column positioning for "hg summary"
        ui.write(_('update: %d new changesets, %d branch heads (merge)\n') %
                 (new, len(bheads)))

    t = []
    draft = len(repo.revs('draft()'))
    if draft:
        t.append(_('%d draft') % draft)
    secret = len(repo.revs('secret()'))
    if secret:
        t.append(_('%d secret') % secret)

    if draft or secret:
        ui.status(_('phases: %s\n') % ', '.join(t))

    cmdutil.summaryhooks(ui, repo)

    if opts.get('remote'):
        needsincoming, needsoutgoing = True, True
    else:
        needsincoming, needsoutgoing = False, False
        for i, o in cmdutil.summaryremotehooks(ui, repo, opts, None):
            if i:
                needsincoming = True
            if o:
                needsoutgoing = True
        if not needsincoming and not needsoutgoing:
            return

    def getincoming():
        source, branches = hg.parseurl(ui.expandpath('default'))
        sbranch = branches[0]
        try:
            other = hg.peer(repo, {}, source)
        except error.RepoError:
            if opts.get('remote'):
                raise
            return source, sbranch, None, None, None
        revs, checkout = hg.addbranchrevs(repo, other, branches, None)
        if revs:
            revs = [other.lookup(rev) for rev in revs]
        ui.debug('comparing with %s\n' % util.hidepassword(source))
        repo.ui.pushbuffer()
        commoninc = discovery.findcommonincoming(repo, other, heads=revs)
        repo.ui.popbuffer()
        return source, sbranch, other, commoninc, commoninc[1]

    if needsincoming:
        source, sbranch, sother, commoninc, incoming = getincoming()
    else:
        source = sbranch = sother = commoninc = incoming = None

    def getoutgoing():
        dest, branches = hg.parseurl(ui.expandpath('default-push', 'default'))
        dbranch = branches[0]
        revs, checkout = hg.addbranchrevs(repo, repo, branches, None)
        if source != dest:
            try:
                dother = hg.peer(repo, {}, dest)
            except error.RepoError:
                if opts.get('remote'):
                    raise
                return dest, dbranch, None, None
            ui.debug('comparing with %s\n' % util.hidepassword(dest))
        elif sother is None:
            # there is no explicit destination peer, but source one is invalid
            return dest, dbranch, None, None
        else:
            dother = sother
        if (source != dest or (sbranch is not None and sbranch != dbranch)):
            common = None
        else:
            common = commoninc
        if revs:
            revs = [repo.lookup(rev) for rev in revs]
        repo.ui.pushbuffer()
        outgoing = discovery.findcommonoutgoing(repo, dother, onlyheads=revs,
                                                commoninc=common)
        repo.ui.popbuffer()
        return dest, dbranch, dother, outgoing

    if needsoutgoing:
        dest, dbranch, dother, outgoing = getoutgoing()
    else:
        dest = dbranch = dother = outgoing = None

    if opts.get('remote'):
        t = []
        if incoming:
            t.append(_('1 or more incoming'))
        o = outgoing.missing
        if o:
            t.append(_('%d outgoing') % len(o))
        other = dother or sother
        if 'bookmarks' in other.listkeys('namespaces'):
            counts = bookmarks.summary(repo, other)
            if counts[0] > 0:
                t.append(_('%d incoming bookmarks') % counts[0])
            if counts[1] > 0:
                t.append(_('%d outgoing bookmarks') % counts[1])

        if t:
            # i18n: column positioning for "hg summary"
            ui.write(_('remote: %s\n') % (', '.join(t)))
        else:
            # i18n: column positioning for "hg summary"
            ui.status(_('remote: (synced)\n'))

    cmdutil.summaryremotehooks(ui, repo, opts,
                               ((source, sbranch, sother, commoninc),
                                (dest, dbranch, dother, outgoing)))

@command('tag',
    [('f', 'force', None, _('force tag')),
    ('l', 'local', None, _('make the tag local')),
    ('r', 'rev', '', _('revision to tag'), _('REV')),
    ('', 'remove', None, _('remove a tag')),
    # -l/--local is already there, commitopts cannot be used
    ('e', 'edit', None, _('invoke editor on commit messages')),
    ('m', 'message', '', _('use text as commit message'), _('TEXT')),
    ] + commitopts2,
    _('[-f] [-l] [-m TEXT] [-d DATE] [-u USER] [-r REV] NAME...'))
def tag(ui, repo, name1, *names, **opts):
    """add one or more tags for the current or given revision

    Name a particular revision using <name>.

    Tags are used to name particular revisions of the repository and are
    very useful to compare different revisions, to go back to significant
    earlier versions or to mark branch points as releases, etc. Changing
    an existing tag is normally disallowed; use -f/--force to override.

    If no revision is given, the parent of the working directory is
    used.

    To facilitate version control, distribution, and merging of tags,
    they are stored as a file named ".hgtags" which is managed similarly
    to other project files and can be hand-edited if necessary. This
    also means that tagging creates a new commit. The file
    ".hg/localtags" is used for local tags (not shared among
    repositories).

    Tag commits are usually made at the head of a branch. If the parent
    of the working directory is not a branch head, :hg:`tag` aborts; use
    -f/--force to force the tag commit to be based on a non-head
    changeset.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    Since tag names have priority over branch names during revision
    lookup, using an existing branch name as a tag name is discouraged.

    Returns 0 on success.
    """
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        rev_ = "."
        names = [t.strip() for t in (name1,) + names]
        if len(names) != len(set(names)):
            raise util.Abort(_('tag names must be unique'))
        for n in names:
            scmutil.checknewlabel(repo, n, 'tag')
            if not n:
                raise util.Abort(_('tag names cannot consist entirely of '
                                   'whitespace'))
        if opts.get('rev') and opts.get('remove'):
            raise util.Abort(_("--rev and --remove are incompatible"))
        if opts.get('rev'):
            rev_ = opts['rev']
        message = opts.get('message')
        if opts.get('remove'):
            if opts.get('local'):
                expectedtype = 'local'
            else:
                expectedtype = 'global'

            for n in names:
                if not repo.tagtype(n):
                    raise util.Abort(_("tag '%s' does not exist") % n)
                if repo.tagtype(n) != expectedtype:
                    if expectedtype == 'global':
                        raise util.Abort(_("tag '%s' is not a global tag") % n)
                    else:
                        raise util.Abort(_("tag '%s' is not a local tag") % n)
            rev_ = nullid
            if not message:
                # we don't translate commit messages
                message = 'Removed tag %s' % ', '.join(names)
        elif not opts.get('force'):
            for n in names:
                if n in repo.tags():
                    raise util.Abort(_("tag '%s' already exists "
                                       "(use -f to force)") % n)
        if not opts.get('local'):
            p1, p2 = repo.dirstate.parents()
            if p2 != nullid:
                raise util.Abort(_('uncommitted merge'))
            bheads = repo.branchheads()
            if not opts.get('force') and bheads and p1 not in bheads:
                raise util.Abort(_('not at a branch head (use -f to force)'))
        r = scmutil.revsingle(repo, rev_).node()

        if not message:
            # we don't translate commit messages
            message = ('Added tag %s for changeset %s' %
                       (', '.join(names), short(r)))

        date = opts.get('date')
        if date:
            date = util.parsedate(date)

        if opts.get('remove'):
            editform = 'tag.remove'
        else:
            editform = 'tag.add'
        editor = cmdutil.getcommiteditor(editform=editform, **opts)

        # don't allow tagging the null rev
        if (not opts.get('remove') and
            scmutil.revsingle(repo, rev_).rev() == nullrev):
            raise util.Abort(_("cannot tag null revision"))

        repo.tag(names, r, message, opts.get('local'), opts.get('user'), date,
                 editor=editor)
    finally:
        release(lock, wlock)

@command('tags', formatteropts, '')
def tags(ui, repo, **opts):
    """list repository tags

    This lists both regular and local tags. When the -v/--verbose
    switch is used, a third column "local" is printed for local tags.

    Returns 0 on success.
    """

    fm = ui.formatter('tags', opts)
    hexfunc = fm.hexfunc
    tagtype = ""

    for t, n in reversed(repo.tagslist()):
        hn = hexfunc(n)
        label = 'tags.normal'
        tagtype = ''
        if repo.tagtype(t) == 'local':
            label = 'tags.local'
            tagtype = 'local'

        fm.startitem()
        fm.write('tag', '%s', t, label=label)
        fmt = " " * (30 - encoding.colwidth(t)) + ' %5d:%s'
        fm.condwrite(not ui.quiet, 'rev node', fmt,
                     repo.changelog.rev(n), hn, label=label)
        fm.condwrite(ui.verbose and tagtype, 'type', ' %s',
                     tagtype, label=label)
        fm.plain('\n')
    fm.end()

@command('tip',
    [('p', 'patch', None, _('show patch')),
    ('g', 'git', None, _('use git extended diff format')),
    ] + templateopts,
    _('[-p] [-g]'))
def tip(ui, repo, **opts):
    """show the tip revision (DEPRECATED)

    The tip revision (usually just called the tip) is the changeset
    most recently added to the repository (and therefore the most
    recently changed head).

    If you have just made a commit, that commit will be the tip. If
    you have just pulled changes from another repository, the tip of
    that repository becomes the current tip. The "tip" tag is special
    and cannot be renamed or assigned to a different changeset.

    This command is deprecated, please use :hg:`heads` instead.

    Returns 0 on success.
    """
    displayer = cmdutil.show_changeset(ui, repo, opts)
    displayer.show(repo['tip'])
    displayer.close()

@command('unbundle',
    [('u', 'update', None,
     _('update to new branch head if changesets were unbundled'))],
    _('[-u] FILE...'))
def unbundle(ui, repo, fname1, *fnames, **opts):
    """apply one or more changegroup files

    Apply one or more compressed changegroup files generated by the
    bundle command.

    Returns 0 on success, 1 if an update has unresolved files.
    """
    fnames = (fname1,) + fnames

    lock = repo.lock()
    try:
        for fname in fnames:
            f = hg.openpath(ui, fname)
            gen = exchange.readbundle(ui, f, fname)
            if isinstance(gen, bundle2.unbundle20):
                tr = repo.transaction('unbundle')
                try:
                    op = bundle2.processbundle(repo, gen, lambda: tr)
                    tr.close()
                finally:
                    if tr:
                        tr.release()
                changes = [r.get('result', 0)
                           for r in op.records['changegroup']]
                modheads = changegroup.combineresults(changes)
            else:
                modheads = changegroup.addchangegroup(repo, gen, 'unbundle',
                                                      'bundle:' + fname)
    finally:
        lock.release()

    return postincoming(ui, repo, modheads, opts.get('update'), None)

@command('^update|up|checkout|co',
    [('C', 'clean', None, _('discard uncommitted changes (no backup)')),
    ('c', 'check', None,
     _('update across branches if no uncommitted changes')),
    ('d', 'date', '', _('tipmost revision matching date'), _('DATE')),
    ('r', 'rev', '', _('revision'), _('REV'))
     ] + mergetoolopts,
    _('[-c] [-C] [-d DATE] [[-r] REV]'))
def update(ui, repo, node=None, rev=None, clean=False, date=None, check=False,
           tool=None):
    """update working directory (or switch revisions)

    Update the repository's working directory to the specified
    changeset. If no changeset is specified, update to the tip of the
    current named branch and move the active bookmark (see :hg:`help
    bookmarks`).

    Update sets the working directory's parent revision to the specified
    changeset (see :hg:`help parents`).

    If the changeset is not a descendant or ancestor of the working
    directory's parent, the update is aborted. With the -c/--check
    option, the working directory is checked for uncommitted changes; if
    none are found, the working directory is updated to the specified
    changeset.

    .. container:: verbose

      The following rules apply when the working directory contains
      uncommitted changes:

      1. If neither -c/--check nor -C/--clean is specified, and if
         the requested changeset is an ancestor or descendant of
         the working directory's parent, the uncommitted changes
         are merged into the requested changeset and the merged
         result is left uncommitted. If the requested changeset is
         not an ancestor or descendant (that is, it is on another
         branch), the update is aborted and the uncommitted changes
         are preserved.

      2. With the -c/--check option, the update is aborted and the
         uncommitted changes are preserved.

      3. With the -C/--clean option, uncommitted changes are discarded and
         the working directory is updated to the requested changeset.

    To cancel an uncommitted merge (and lose your changes), use
    :hg:`update --clean .`.

    Use null as the changeset to remove the working directory (like
    :hg:`clone -U`).

    If you want to revert just one file to an older revision, use
    :hg:`revert [-r REV] NAME`.

    See :hg:`help dates` for a list of formats valid for -d/--date.

    Returns 0 on success, 1 if there are unresolved files.
    """
    if rev and node:
        raise util.Abort(_("please specify just one revision"))

    if rev is None or rev == '':
        rev = node

    cmdutil.clearunfinished(repo)

    # with no argument, we also move the active bookmark, if any
    rev, movemarkfrom = bookmarks.calculateupdate(ui, repo, rev)

    # if we defined a bookmark, we have to remember the original bookmark name
    brev = rev
    rev = scmutil.revsingle(repo, rev, rev).rev()

    if check and clean:
        raise util.Abort(_("cannot specify both -c/--check and -C/--clean"))

    if date:
        if rev is not None:
            raise util.Abort(_("you can't specify a revision and a date"))
        rev = cmdutil.finddate(ui, repo, date)

    if check:
        cmdutil.bailifchanged(repo, merge=False)
        if rev is None:
            rev = repo[repo[None].branch()].rev()

    repo.ui.setconfig('ui', 'forcemerge', tool, 'update')

    if clean:
        ret = hg.clean(repo, rev)
    else:
        ret = hg.update(repo, rev)

    if not ret and movemarkfrom:
        if bookmarks.update(repo, [movemarkfrom], repo['.'].node()):
            ui.status(_("updating bookmark %s\n") % repo._activebookmark)
    elif brev in repo._bookmarks:
        bookmarks.activate(repo, brev)
        ui.status(_("(activating bookmark %s)\n") % brev)
    elif brev:
        if repo._activebookmark:
            ui.status(_("(leaving bookmark %s)\n") %
                      repo._activebookmark)
        bookmarks.deactivate(repo)

    return ret

@command('verify', [])
def verify(ui, repo):
    """verify the integrity of the repository

    Verify the integrity of the current repository.

    This will perform an extensive check of the repository's
    integrity, validating the hashes and checksums of each entry in
    the changelog, manifest, and tracked files, as well as the
    integrity of their crosslinks and indices.

    Please see http://mercurial.selenic.com/wiki/RepositoryCorruption
    for more information about recovery from corruption of the
    repository.

    Returns 0 on success, 1 if errors are encountered.
    """
    return hg.verify(repo)

@command('version', [], norepo=True)
def version_(ui):
    """output version and copyright information"""
    ui.write(_("Mercurial Distributed SCM (version %s)\n")
             % util.version())
    ui.status(_(
        "(see http://mercurial.selenic.com for more information)\n"
        "\nCopyright (C) 2005-2015 Matt Mackall and others\n"
        "This is free software; see the source for copying conditions. "
        "There is NO\nwarranty; "
        "not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n"
    ))

    ui.note(_("\nEnabled extensions:\n\n"))
    if ui.verbose:
        # format names and versions into columns
        names = []
        vers = []
        for name, module in extensions.extensions():
            names.append(name)
            vers.append(extensions.moduleversion(module))
        if names:
            maxnamelen = max(len(n) for n in names)
            for i, name in enumerate(names):
                ui.write("  %-*s  %s\n" % (maxnamelen, name, vers[i]))
