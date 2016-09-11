# Copyright 2016-present Facebook. All Rights Reserved.
#
# commands: fastannotate commands
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from fastannotate import (
    context as facontext,
    error as faerror,
    formatter as faformatter,
)

from mercurial import (
    commands,
    error,
    extensions,
    scmutil,
)

from mercurial.i18n import _

fastannotatecommandargs = {
    'options': [
        ('r', 'rev', '.', _('annotate the specified revision'), _('REV')),
        ('u', 'user', None, _('list the author (long with -v)')),
        ('f', 'file', None, _('list the filename')),
        ('d', 'date', None, _('list the date (short with -q)')),
        ('n', 'number', None, _('list the revision number (default)')),
        ('c', 'changeset', None, _('list the changeset')),
        ('l', 'line-number', None, _('show line number at the first '
                                     'appearance')),
        ('e', 'deleted', None, _('show deleted lines (slow)')),
        ('h', 'no-content', None, _('do not show file content')),
        ('', 'no-follow', None, _("don't follow copies and renames")),
        ('', 'linear', None, _('enforce linear history, ignore second parent '
                               'of merges (faster)')),
        ('', 'long-hash', None, _('show long changeset hash (EXPERIMENTAL)')),
        ('', 'rebuild', None, _('rebuild cache even if it exists '
                                '(EXPERIMENTAL)')),
    ] + commands.walkopts,
    'synopsis': _('[-r REV] [-f] [-a] [-u] [-d] [-n] [-c] [-l] FILE...'),
    'inferrepo': True,
}

def fastannotate(ui, repo, *pats, **opts):
    """show changeset information by line for each file

    List changes in files, showing the revision id responsible for each line.

    This command is useful for discovering when a change was made and by whom.

    If you include --file, --user, or --date, the revision number is suppressed
    unless you also include --number.

    This command uses an implementation different from the vanilla annotate
    command, which may produce slightly different (while still reasonable)
    output for some cases.

    For the best performance, use -c, -l, avoid -u, -d, -n. Use --linear
    and --no-content to make it even faster.

    Returns 0 on success.
    """
    if not pats:
        raise error.Abort(_('at least one filename or pattern is required'))

    # performance hack: filtered repo can be slow. unfilter by default.
    if ui.configbool('fastannotate', 'unfilteredrepo', True):
        repo = repo.unfiltered()

    rev = opts.get('rev', '.')
    rebuild = opts.get('rebuild', False)

    ctx = scmutil.revsingle(repo, rev)
    m = scmutil.match(ctx, pats, opts)

    aopts = facontext.annotateopts(
        followmerge=not opts.get('linear', False),
        followrename=not opts.get('no_follow', False))

    if not any(opts.get(s)
               for s in ['user', 'date', 'file', 'number', 'changeset']):
        # default 'number' for compatibility. but fastannotate is more
        # efficient with "changeset", "line-number" and "no-content".
        for name in ui.configlist('fastannotate', 'defaultformat', ['number']):
            opts[name] = True

    formatter = faformatter.defaultformatter(ui, repo, opts)
    showdeleted = opts.get('deleted', False)
    showlines = not bool(opts.get('no_content'))
    showpath = opts.get('file', False)

    # find the head of the main (master) branch
    masterrev = ui.config('fastannotate', 'mainbranch')
    if masterrev:
        master = lambda: scmutil.revsingle(repo, masterrev).rev()
    else:
        master = rev

    for path in ctx.walk(m):
        result = lines = existinglines = None
        while True:
            try:
                with facontext.annotatecontext(repo, path, aopts, rebuild) as a:
                    result = a.annotate(rev, master=master, showpath=showpath,
                                        showlines=(showlines and
                                                   not showdeleted))
                    if showdeleted:
                        existinglines = set((l[0], l[1]) for l in result)
                        result = a.annotatealllines(
                            rev, showpath=showpath, showlines=showlines)
                break
            except faerror.CannotReuseError: # happens if master moves backwards
                if rebuild: # give up since we have tried rebuild alreadyraise
                    raise
                else: # try a second time rebuilding the cache (slow)
                    rebuild = True
                    continue

        if showlines:
            result, lines = result

        formatter.write(result, lines, existinglines=existinglines)

_newopts = set([])
_knownopts = set([opt[1].replace('-', '_') for opt in
                  (fastannotatecommandargs['options'] + commands.globalopts)])

def _annotatewrapper(orig, ui, repo, *pats, **opts):
    """use this in extensions.wrapcommand"""
    nonemptyopts = set(k for k, v in opts.iteritems() if v)
    unknownopts = nonemptyopts.difference(_knownopts)
    if unknownopts:
        ui.debug('fastannotate: option %r is not supported, falling back '
                 'to the original annotate\n' % list(unknownopts))
        unsafeopts = _newopts.intersection(nonemptyopts)
        if unsafeopts:
            raise error.Abort(_('--%s cannot be used together with --%s')
                              % (list(unknownopts)[0].replace('_', '-'),
                                 list(unsafeopts)[0].replace('_', '-')))
        return orig(ui, repo, *pats, **opts)
    return fastannotate(ui, repo, *pats, **opts)

def _appendoptions(origopts):
    """append our options to the original amend option list"""
    for newopt in fastannotatecommandargs['options']:
        if any([o[1] == newopt[1] for o in origopts]):
            continue
        origopts.append(newopt)
        _newopts.add(newopt[1].replace('-', '_'))

def replacedefault():
    """replace the default annotate command"""
    entry = extensions.wrapcommand(commands.table, 'annotate', _annotatewrapper)
    _appendoptions(entry[1])
