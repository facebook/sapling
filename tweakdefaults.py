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

from mercurial import util, cmdutil, commands, extensions, hg, scmutil
from mercurial import bookmarks
from mercurial.extensions import wrapcommand, wrapfunction
from mercurial import extensions
from mercurial.i18n import _
from hgext import rebase
import errno, os, stat, subprocess, time

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

    entry = wrapcommand(commands.table, 'commit', commitcmd)
    options = entry[1]
    options.insert(9, ('M', 'reuse-message', '',
        _('reuse commit message from REV'), _('REV')))

    wrapcommand(rebase.cmdtable, 'rebase', _rebase)
    entry = wrapcommand(commands.table, 'pull', pull)
    options = entry[1]
    options.append(
        ('d', 'dest', '', _('destination for rebase or update')))

    try:
        remotenames = extensions.find('remotenames')
        wrapfunction(remotenames, '_getrebasedest', _getrebasedest)
    except KeyError:
        pass  # no remotenames, no worries
    except AttributeError:
        pass  # old version of remotenames doh

    entry = wrapcommand(commands.table, 'log', log)
    for opt in logopts:
        opt = (opt[0], opt[1], opt[2], opt[3])
        entry[1].append(opt)

    entry = wrapcommand(commands.table, 'branch', branchcmd)
    options = entry[1]
    options.append(('', 'new', None, _('allow branch creation')))
    wrapcommand(commands.table, 'branches', branchescmd)

    wrapcommand(commands.table, 'merge', mergecmd)

    entry = wrapcommand(commands.table, 'status', statuscmd)
    options = entry[1]
    options.append(
        ('', 'root-relative', None, _('show status relative to root')))

    wrapcommand(commands.table, 'rollback', rollbackcmd)

    wrapcommand(commands.table, 'tag', tagcmd)
    wrapcommand(commands.table, 'tags', tagscmd)
    wrapcommand(commands.table, 'graft', graftcmd)
    try:
      fbamendmodule = extensions.find('fbamend')
      wrapcommand(fbamendmodule.cmdtable, 'amend', amendcmd)
    except KeyError:
      pass

    # Tweak Behavior
    tweakbehaviors(ui)

def tweakorder():
    """
    Tweakdefaults generally should load first; other extensions may modify
    behavior such that tweakdefaults will be happy, so we should not prevent
    that from happening too early in the process. Note that by loading first,
    we ensure that tweakdefault's function wrappers run *last*.

    As of this writing, the extensions that we should load before are
    remotenames and directaccess (NB: directaccess messes with order as well).
    """
    order = extensions._order
    order.remove('tweakdefaults')
    order.insert(0, 'tweakdefaults')
    extensions._order = order

# This is an ugly hack
# The remotenames extension removes the --rebase flag from pull so that the
# upstream rebase won't rebase to the wrong place. However, we want to allow
# the user to specify an explicit destination, but still abort if the user
# specifies dest without update or rebase. Conveniently, _getrebasedest is
# called before the --rebase flag is stripped from the opts. We will save it
# when _getrebasedest is called, then look it up in the pull command to do the
# right thing.
rebaseflag = False
rebasedest = None

def _getrebasedest(orig, repo, opts):
    """Use the manually specified destination over the tracking destination"""
    global rebaseflag, rebasedest
    rebaseflag = opts.get('rebase')
    origdest = orig(repo, opts)
    dest = opts.get('dest')
    if not dest:
        dest = origdest
    rebasedest = dest
    return dest

def pull(orig, ui, repo, *args, **opts):
    """pull --rebase/--update are problematic without an explicit destination"""
    try:
        rebasemodule = extensions.find('rebase')
    except KeyError:
        rebasemodule = None

    rebase = opts.get('rebase')
    update = opts.get('update')
    isrebase = rebase or rebaseflag
    if isrebase:
        dest = rebasedest
    else:
        dest = opts.get('dest')

    if not dest:
        dest = ui.config('tweakdefaults', 'defaultdest')

    if isrebase and update:
        mess = _('specify either rebase or update, not both')
        raise util.Abort(mess)

    if dest and not (isrebase or update):
        mess = _('only specify a destination if rebasing or updating')
        raise util.Abort(mess)

    if (isrebase or update) and not dest:
        rebasemsg = _('you must use a bookmark with tracking '
                      'or manually specify a destination for the rebase')
        if isrebase and bmactive(repo):
            rebasehint = _('set up tracking with `hg book -t <destination>` '
                           'or manually supply --dest / -d')
            mess = ui.config('tweakdefaults', 'bmnodestmsg', rebasemsg)
            hint = ui.config('tweakdefaults', 'bmnodesthint', _(
                'set up tracking with `hg book -t <destination>` '
                'or manually supply --dest / -d'))
        elif isrebase:
            mess = ui.config('tweakdefaults', 'nodestmsg', rebasemsg)
            hint = ui.config('tweakdefaults', 'nodesthint', _(
                'set up tracking with `hg book <name> -t <destination>` '
                'or manually supply --dest / -d'))
        else: # update
            mess = _('you must specify a destination for the update')
            hint = _('use `hg pull --update --dest <destination>`')
        raise util.Abort(mess, hint=hint)

    if 'rebase' in opts:
        del opts['rebase']
    if 'update' in opts:
        del opts['update']
    if 'dest' in opts:
        del opts['dest']

    ret = orig(ui, repo, *args, **opts)

    # NB: we use rebase and not isrebase on the next line because
    # remotenames may have already handled the rebase.
    if dest and rebase:
        ret = ret or rebasemodule.rebase(ui, repo, dest=dest)
    if dest and update:
        ret = ret or commands.update(ui, repo, node=dest, check=True)

    return ret


def tweakbehaviors(ui):
    """Tweak Behaviors

    Right now this only tweaks the rebase behavior such that the default
    exit status code for a noop rebase becomes 0 instead of 1.

    In future we may add or modify other behaviours here.
    """

    # noop rebase returns 0
    def _nothingtorebase(orig, *args, **kwargs):
        return 0

    if ui.configbool("tweakdefaults", "nooprebase", True):
        try:
            rebase = extensions.find("rebase")
            extensions.wrapfunction(
                rebase, "_nothingtorebase", _nothingtorebase
            )
        except (KeyError, AttributeError):
            pass

def commitcmd(orig, ui, repo, *pats, **opts):
    if (opts.get("amend")
            and not opts.get("date")
            and not ui.configbool('tweakdefaults', 'amendkeepdate')):
        opts["date"] = currentdate()

    rev = opts.get('reuse_message')
    if rev:
        invalidargs = ['message', 'logfile']
        currentinvalidargs = [ia for ia in invalidargs if opts.get(ia)]
        if currentinvalidargs:
            raise util.Abort(_('--reuse-message and --%s are '
                'mutually exclusive') % (currentinvalidargs[0]))

    if rev:
       opts['message'] = repo[rev].description()

    return orig(ui, repo, *pats, **opts)

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
    if not opts.get('date') and not ui.configbool('tweakdefaults', 'rebasekeepdate'):
        opts['date'] = currentdate()

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

def currentdate():
    return "%d %d" % util.makedate(time.time())

def graftcmd(orig, ui, repo, *revs, **opts):
    if not opts.get("date") and not ui.configbool('tweakdefaults', 'graftkeepdate'):
        opts["date"] = currentdate()
    return orig(ui, repo, *revs, **opts)

def amendcmd(orig, ui, repo, *pats, **opts):
    if not opts.get("date") and not ui.configbool('tweakdefaults', 'amendkeepdate'):
        opts["date"] = currentdate()
    return orig(ui, repo, *pats, **opts)

def log(orig, ui, repo, *pats, **opts):
    # 'hg log' defaults to -f
    # All special uses of log (--date, --branch, etc) will also now do follow.
    if not opts.get('rev') and not opts.get('all'):
        opts['follow'] = True

    return orig(ui, repo, *pats, **opts)

def branchcmd(orig, ui, repo, label=None, **opts):
    message = ui.config('tweakdefaults', 'branchmessage',
            _('new named branches are disabled in this repository'))
    enabled = ui.configbool('tweakdefaults', 'allowbranch', True)
    if (enabled and opts.get('new')) or label is None:
        if 'new' in opts:
            del opts['new']
        return orig(ui, repo, label, **opts)
    elif enabled:
        raise util.Abort(
            _('do not use branches; use bookmarks instead'),
            hint=_('use --new if you are certain you want a branch'))
    else:
        raise util.Abort(message)

def branchescmd(orig, ui, repo, active, closed, **opts):
    message = ui.config('tweakdefaults', 'branchesmessage')
    if message:
        ui.warn(message + '\n')
    return orig(ui, repo, active, closed, **opts)

def mergecmd(orig, ui, repo, node=None, **opts):
    """
    Allowing to disable merges
    """
    if ui.configbool('tweakdefaults','allowmerge', True):
        return orig(ui, repo, node, **opts)
    else:
        message = ui.config('tweakdefaults', 'mergemessage',
            _('merging is not supported for this repository'))
        hint = ui.config('tweakdefaults', 'mergehint', _('use rebase instead'))
        raise util.Abort(message, hint=hint)

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

def rollbackcmd(orig, ui, repo, **opts):
    """
    Allowing to disable the rollback command
    """
    if ui.configbool('tweakdefaults', 'allowrollback', True):
        return orig(ui, repo, **opts)
    else:
        message = ui.config('tweakdefaults', 'rollbackmessage',
            _('the use of rollback is disabled'))
        hint = ui.config('tweakdefaults', 'rollbackhint', None)
        raise util.Abort(message, hint=hint)

def tagcmd(orig, ui, repo, name1, *names, **opts):
    """
    Allowing to disable tags
    """
    message = ui.config('tweakdefaults', 'tagmessage',
            _('new tags are disabled in this repository'))
    if ui.configbool('tweakdefaults', 'allowtags', True):
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
