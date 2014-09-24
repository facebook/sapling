# tweakdefaults.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import util, cmdutil, commands, hg, scmutil
from mercurial import bookmarks
from mercurial.extensions import wrapcommand
from mercurial.i18n import _
from hgext import rebase
import errno, os

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def update(orig, ui, repo, node=None, rev=None, **kwargs):
    # 'hg update' should do nothing
    if not node and not rev:
        raise util.Abort(
            'you must specify a destination to update to',
            hint="if you're trying to move a bookmark forward, try " +
                 "'hg rebase -d <destination>'")

    return orig(ui, repo, node=node, rev=rev, **kwargs)
wrapcommand(commands.table, 'update', update)

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
@command('grep', [('A', 'after-context', '', 'Print NUM lines of trailing context after matching lines. Places a line containing -- between contiguous groups of matches', 'NUM'),
                  ('B', 'before-context', '', 'Print  NUM  lines   of  leading  context  before  matching lines. Places  a   line  containing  --  between  contiguous  groups  of matches.', 'NUM'),
                  ('C', 'context', '', 'Print  NUM lines of output context.  Places a line containing -- between contiguous groups of matches.', 'NUM'),
                  ('i', 'ignore-case', None, 'Ignore  case  distinctions  in  both  the   PATTERN and the input files.'),
                  ('l', 'files-with-matches', None, 'Suppress normal output; instead print the   name  of  each   input file  from  which   output would normally have been printed.  The scanning will stop on the first match.'),
                  ('n', 'line-number', None,'Prefix each line of output with the line number within its input file.'),
                  ('V', 'invert-match', None, 'Invert the sense of matching, to select non-matching lines.'),
                  ('w', 'word-regexp', None, 'Select only those   lines  containing  matches  that  form   whole words.   The  test is that the matching substring must either be at the beginning of the line, or preceded   by  a  non-word  constituent  character.  Similarly, it must be either at the end of the line or followed by a non-word constituent character.   Wordconstituent  characters are letters, digits, and the underscore.')
                  ], '[OPTION]... PATTERN')
def grep(orig, ui, pattern, **opts):
    """search for a pattern in tracked files in the working directory"""

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
    return os.system("hg status --no-status --clean --modified --added --print0 ."
                     " | xargs -0 grep --binary-files=without-match --regexp='%s' "
                     "%s 2>/dev/null" % (pattern, optstr))



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
wrapcommand(rebase.cmdtable, 'rebase', _rebase)

logopts = [
    ('', 'all', None, _('shows all commits in the repo')),
]

def log(orig, ui, repo, *pats, **opts):
    # 'hg log' defaults to -f
    # All special uses of log (--date, --branch, etc) will also now do follow.
    if not opts.get('rev') and not opts.get('all'):
        opts['follow'] = True

    return orig(ui, repo, *pats, **opts)

entry = wrapcommand(commands.table, 'log', log)
for opt in logopts:
    opt = (opt[0], opt[1], opt[2], opt[3])
    entry[1].append(opt)
