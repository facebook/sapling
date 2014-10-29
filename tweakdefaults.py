# tweakdefaults.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""FBONLY: user friendly deafults

This extension changes defaults to be more user friendly.

  hg log      always follows history (-f)
  hg rebase   does nothing without arguments
  hg update   does nothing without arguments
"""

from mercurial import util, cmdutil, commands, hg, scmutil
from mercurial import bookmarks
from mercurial.extensions import wrapcommand
from mercurial.i18n import _
from hgext import rebase
import errno, os, stat, subprocess

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def extsetup(ui):
    wrapcommand(commands.table, 'update', update)
    wrapcommand(rebase.cmdtable, 'rebase', _rebase)

    entry = wrapcommand(commands.table, 'log', log)
    for opt in logopts:
        opt = (opt[0], opt[1], opt[2], opt[3])
        entry[1].append(opt)

def update(orig, ui, repo, node=None, rev=None, **kwargs):
    # 'hg update' should do nothing
    if not node and not rev:
        raise util.Abort(
            'you must specify a destination to update to',
            hint="if you're trying to move a bookmark forward, try " +
                 "'hg rebase -d <destination>'")

    return orig(ui, repo, node=node, rev=rev, **kwargs)

@command('histgrep', commands.table['grep'][1], commands.table['grep'][2])
def histgrep(ui, repo, pattern, *pats, **opts):
    """search for a pattern in specified files and revisions

    Search revisions of files for a regular expression.

    The command used to be hg grep.

    This command behaves differently than Unix grep. It only accepts
    Python/Perl regexps. It searches repository history, not the working
    directory. It always prints the revision number in which a match appears.

    By default, grep only prints output for the first revision of a file in
    which it finds a match. To get it to print every revision that contains a
    change in match status ("-" for a match that becomes a non-match, or "+"
    for a non-match that becomes a match), use the --all flag.

    Returns 0 if a match is found, 1 otherwise."""
    return commands.grep(ui, repo, pattern, **opts)

del commands.table['grep']
@command('grep',
    [('A', 'after-context', '', 'print NUM lines of trailing context', 'NUM'),
     ('B', 'before-context', '', 'print NUM lines of leading context', 'NUM'),
     ('C', 'context', '', 'print NUM lines of output context', 'NUM'),
     ('i', 'ignore-case', None, 'ignore case when matching'),
     ('l', 'files-with-matches', None, 'print only filenames that match'),
     ('n', 'line-number', None, 'print matching line numbers'),
     ('V', 'invert-match', None, 'select non-matching lines'),
     ('w', 'word-regexp', None, 'match whole words only'),
     ('E', 'extended-regexp', None, 'use POSIX extended regexps'),
     ('F', 'fixed-strings', None, 'interpret pattern as fixed string'),
     ('P', 'perl-regexp', None, 'use Perl-compatible regexps'),
     ], '[OPTION]... PATTERN [FILE]...',
     inferrepo=True)
def grep(ui, repo, pattern, *pats, **opts):
    """search for a pattern in tracked files in the working directory

    The default regexp style is POSIX basic regexps. If no FILE parameters are
    passed in, the current directory and its subdirectories will be searched.

    For the old 'hg grep', see 'histgrep'."""

    optstr = ''
    if opts.get('after_context'):
        optstr += '-A' + opts.get('after_context') + ' '
    if opts.get('before_context'):
        optstr += '-B' + opts.get('before_context') + ' '
    if opts.get('context'):
        optstr += '-C' + opts.get('context') + ' '
    if opts.get('ignore_case'):
        optstr += '-i '
    if opts.get('files_with_matches'):
        optstr += '-l '
    if opts.get('line_number'):
        optstr += '-n '
    if opts.get('invert_match'):
        optstr += '-v '
    if opts.get('word_regexp'):
        optstr += '-w '
    if opts.get('extended_regexp'):
        optstr += '-E '
    if opts.get('fixed_strings'):
        optstr += '-F '
    if opts.get('perl_regexp'):
        optstr += '-P '

    # color support, using the color extension
    colormode = getattr(ui, '_colormode', '')
    if colormode == 'ansi':
        optstr += '--color=always '

    wctx = repo[None]
    m = scmutil.match(wctx, ['.'], {'include': pats})
    p = subprocess.Popen(
        'xargs -0 grep --no-messages --binary-files=without-match '
        '--with-filename --regexp=%s %s --' %
        (util.shellquote(pattern), optstr),
        shell=True, bufsize=-1, close_fds=util.closefds,
        stdin=subprocess.PIPE)

    write = p.stdin.write
    ds = repo.dirstate
    getkind = stat.S_IFMT
    lnkkind = stat.S_IFLNK
    for f in wctx.matches(m):
        # skip symlinks and removed files
        t = ds._map[f]
        if t[0] == 'r' or getkind(t[1]) == lnkkind:
            continue
        write(m.rel(f) + '\0')

    p.stdin.close()
    return p.wait()

def _rebase(orig, ui, repo, **opts):
    if opts.get('continue') or opts.get('abort'):
        return orig(ui, repo, **opts)

    # 'hg rebase' w/o args should do nothing
    if not opts.get('dest'):
        raise util.Abort("you must specify a destination (-d) for the rebase")

    # 'hg rebase' can fast-forward bookmark
    prev = repo['.']
    dest = scmutil.revsingle(repo, opts.get('dest'))

    # Only fast-forward the bookmark if no source nodes were explicitly
    # specified.
    if not (opts.get('base') or opts.get('source') or opts.get('rev')):
        common = dest.ancestor(prev)
        if prev == common:
            result = hg.update(repo, dest.node())
            if repo._bookmarkcurrent:
                bookmarks.update(repo, [prev.node()], dest.node())
            return result

    return orig(ui, repo, **opts)

logopts = [
    ('', 'all', None, _('shows all commits in the repo')),
]

def log(orig, ui, repo, *pats, **opts):
    # 'hg log' defaults to -f
    # All special uses of log (--date, --branch, etc) will also now do follow.
    if not opts.get('rev') and not opts.get('all'):
        opts['follow'] = True

    return orig(ui, repo, *pats, **opts)

