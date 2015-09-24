# tweakdefaults.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""user friendly defaults

This extension changes defaults to be more user friendly.

  hg log        always follows history (-f)
  hg rebase     aborts without arguments
  hg update     aborts without arguments
                aborts if working copy is not clean
  hg branch     aborts and encourages use of bookmarks
  hg grep       greps the working directory instead of history
  hg histgrep   renamed from grep

"""

from mercurial import util, cmdutil, commands, hg, scmutil
from mercurial import bookmarks
from mercurial.extensions import wrapcommand, _order
import mercurial.extensions
from mercurial.i18n import _
from hgext import rebase
import errno, os, stat, subprocess

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

logopts = [
    ('', 'all', None, _('shows all commits in the repo')),
]

def uisetup(ui):
    tweakorder()

def extsetup(ui):
    entry = wrapcommand(commands.table, 'update', update)
    options = entry[1]
    # try to put in alphabetical order
    options.insert(3, ('n', 'nocheck', None,
        _('update even with outstanding changes')))

    wrapcommand(rebase.cmdtable, 'rebase', _rebase)

    entry = wrapcommand(commands.table, 'log', log)
    for opt in logopts:
        opt = (opt[0], opt[1], opt[2], opt[3])
        entry[1].append(opt)

    entry = wrapcommand(commands.table, 'branch', branchcmd)
    options = entry[1]
    options.append(('', 'new', None, _('allow branch creation')))

    entry = wrapcommand(commands.table, 'status', statuscmd)
    options = entry[1]
    options.append(
        ('', 'root-relative', None, _('show status relative to root')))

    wrapcommand(commands.table, 'tag', tagcmd)
    wrapcommand(commands.table, 'tags', tagscmd)

def tweakorder():
    """
    Tweakdefaults generally should load first; other extensions may modify
    behavior such that tweakdefaults will be happy, so we should not prevent
    that from happening too early in the process. Note that by loading first,
    we ensure that tweakdefault's function wrappers run *last*.

    As of this writing, the extensions that we should load before are
    remotenames and directaccess (NB: directaccess messes with order as well).
    """
    order = mercurial.extensions._order
    order.remove('tweakdefaults')
    order.insert(0, 'tweakdefaults')
    mercurial.extensions._order = order

def update(orig, ui, repo, node=None, rev=None, **kwargs):
    # 'hg update' should do nothing
    if not node and not rev and not kwargs['date']:
        raise util.Abort(
            'You must specify a destination to update to,' +
            ' for example "hg update master".',
            hint='If you\'re trying to move a bookmark forward, try ' +
                 '"hg rebase -d <destination>".')


    # By default, never update when there are local changes unless updating to
    # the current rev. This is useful for, eg, arc feature when the only
    # thing changing is the bookmark.
    if not kwargs['clean'] and not kwargs['nocheck']:
        target = node or rev
        if target and scmutil.revsingle(repo, target, target).rev() != \
                repo.revs('.').first():
            kwargs['check'] = True

    if 'nocheck' in kwargs:
        del kwargs['nocheck']

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

    grepcommand = ui.config('grep', 'command', default='grep')
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
        'xargs -0 %s --no-messages --binary-files=without-match '
        '--with-filename --regexp=%s %s --' %
        (grepcommand, util.shellquote(pattern), optstr),
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
            if bmactive(repo):
                bookmarks.update(repo, [prev.node()], dest.node())
            return result

    return orig(ui, repo, **opts)

def log(orig, ui, repo, *pats, **opts):
    # 'hg log' defaults to -f
    # All special uses of log (--date, --branch, etc) will also now do follow.
    if not opts.get('rev') and not opts.get('all'):
        opts['follow'] = True

    return orig(ui, repo, *pats, **opts)

def branchcmd(orig, ui, repo, label=None, **opts):
    if label is None or opts.get('new'):
        if 'new' in opts:
            del opts['new']
        return orig(ui, repo, label, **opts)
    raise util.Abort(
            _('do not use branches; use bookmarks instead'),
            hint=_('use --new if you are certain you want a branch'))

def statuscmd(orig, ui, repo, *pats, **opts):
    """
    Make status relative by default for interactive usage
    """
    if opts.get('root_relative'):
        del opts['root_relative']
    elif os.environ.get('HGPLAIN'): # don't break automation
        pass
    # Here's an ugly hack! If users are passing "re:" to make status relative,
    # hgwatchman will never refresh the full state and status will become and
    # remain slow after a restart or 24 hours. Here, we check for this and
    # replace 're:' with '' which has the same semantic effect but works for
    # hgwatchman (because match.always() == True), if and only if 're:' is the
    # only pattern passed.
    #
    # Also set pats to [''] if pats is empty because that makes status relative.
    elif not pats or (len(pats) == 1 and pats[0] == 're:'):
        pats = ['']
    return orig(ui, repo, *pats, **opts)

def tagcmd(orig, ui, repo, name1, *names, **opts):
    """
    Disabling tags unless allowed
    """
    message = ui.config('tweakdefaults', 'tagmessage',
            'new tags are disabled in this repository')
    if ui.configbool('tweakdefaults', 'allowtags'):
        return orig(ui, repo, name1, *names, **opts)
    else:
        raise util.Abort(message)

def tagscmd(orig, ui, repo, **opts):
    message = ui.config('tweakdefaults', 'tagsmessage', '')
    if message:
        ui.warn(message + '\n')
    return orig(ui, repo, **opts)


### bookmarks api compatibility layer ###
def bmactive(repo):
    try:
        return repo._activebookmark
    except AttributeError:
        return repo._bookmarkcurrent
