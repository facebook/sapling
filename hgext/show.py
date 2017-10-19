# show.py - Extension implementing `hg show`
#
# Copyright 2017 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""unified command to show various repository information (EXPERIMENTAL)

This extension provides the :hg:`show` command, which provides a central
command for displaying commonly-accessed repository data and views of that
data.

The following config options can influence operation.

``commands``
------------

``show.aliasprefix``
   List of strings that will register aliases for views. e.g. ``s`` will
   effectively set config options ``alias.s<view> = show <view>`` for all
   views. i.e. `hg swork` would execute `hg show work`.

   Aliases that would conflict with existing registrations will not be
   performed.
"""

from __future__ import absolute_import

from mercurial.i18n import _
from mercurial.node import nullrev
from mercurial import (
    cmdutil,
    commands,
    destutil,
    error,
    formatter,
    graphmod,
    phases,
    pycompat,
    registrar,
    revset,
    revsetlang,
)

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

cmdtable = {}
command = registrar.command(cmdtable)

revsetpredicate = registrar.revsetpredicate()

class showcmdfunc(registrar._funcregistrarbase):
    """Register a function to be invoked for an `hg show <thing>`."""

    # Used by _formatdoc().
    _docformat = '%s -- %s'

    def _extrasetup(self, name, func, fmtopic=None, csettopic=None):
        """Called with decorator arguments to register a show view.

        ``name`` is the sub-command name.

        ``func`` is the function being decorated.

        ``fmtopic`` is the topic in the style that will be rendered for
        this view.

        ``csettopic`` is the topic in the style to be used for a changeset
        printer.

        If ``fmtopic`` is specified, the view function will receive a
        formatter instance. If ``csettopic`` is specified, the view
        function will receive a changeset printer.
        """
        func._fmtopic = fmtopic
        func._csettopic = csettopic

showview = showcmdfunc()

@command('show', [
    # TODO: Switch this template flag to use cmdutil.formatteropts if
    # 'hg show' becomes stable before --template/-T is stable. For now,
    # we are putting it here without the '(EXPERIMENTAL)' flag because it
    # is an important part of the 'hg show' user experience and the entire
    # 'hg show' experience is experimental.
    ('T', 'template', '', ('display with template'), _('TEMPLATE')),
    ], _('VIEW'))
def show(ui, repo, view=None, template=None):
    """show various repository information

    A requested view of repository data is displayed.

    If no view is requested, the list of available views is shown and the
    command aborts.

    .. note::

       There are no backwards compatibility guarantees for the output of this
       command. Output may change in any future Mercurial release.

       Consumers wanting stable command output should specify a template via
       ``-T/--template``.

    List of available views:
    """
    if ui.plain() and not template:
        hint = _('invoke with -T/--template to control output format')
        raise error.Abort(_('must specify a template in plain mode'), hint=hint)

    views = showview._table

    if not view:
        ui.pager('show')
        # TODO consider using formatter here so available views can be
        # rendered to custom format.
        ui.write(_('available views:\n'))
        ui.write('\n')

        for name, func in sorted(views.items()):
            ui.write(('%s\n') % func.__doc__)

        ui.write('\n')
        raise error.Abort(_('no view requested'),
                          hint=_('use "hg show VIEW" to choose a view'))

    # TODO use same logic as dispatch to perform prefix matching.
    if view not in views:
        raise error.Abort(_('unknown view: %s') % view,
                          hint=_('run "hg show" to see available views'))

    template = template or 'show'

    fn = views[view]
    ui.pager('show')

    if fn._fmtopic:
        fmtopic = 'show%s' % fn._fmtopic
        with ui.formatter(fmtopic, {'template': template}) as fm:
            return fn(ui, repo, fm)
    elif fn._csettopic:
        ref = 'show%s' % fn._csettopic
        spec = formatter.lookuptemplate(ui, ref, template)
        displayer = cmdutil.changeset_templater(ui, repo, spec, buffered=True)
        return fn(ui, repo, displayer)
    else:
        return fn(ui, repo)

@showview('bookmarks', fmtopic='bookmarks')
def showbookmarks(ui, repo, fm):
    """bookmarks and their associated changeset"""
    marks = repo._bookmarks
    if not len(marks):
        # This is a bit hacky. Ideally, templates would have a way to
        # specify an empty output, but we shouldn't corrupt JSON while
        # waiting for this functionality.
        if not isinstance(fm, formatter.jsonformatter):
            ui.write(_('(no bookmarks set)\n'))
        return

    revs = [repo[node].rev() for node in marks.values()]
    active = repo._activebookmark
    longestname = max(len(b) for b in marks)
    nodelen = longestshortest(repo, revs)

    for bm, node in sorted(marks.items()):
        fm.startitem()
        fm.context(ctx=repo[node])
        fm.write('bookmark', '%s', bm)
        fm.write('node', fm.hexfunc(node), fm.hexfunc(node))
        fm.data(active=bm == active,
                longestbookmarklen=longestname,
                nodelen=nodelen)

@showview('stack', csettopic='stack')
def showstack(ui, repo, displayer):
    """current line of work"""
    wdirctx = repo['.']
    if wdirctx.rev() == nullrev:
        raise error.Abort(_('stack view only available when there is a '
                            'working directory'))

    if wdirctx.phase() == phases.public:
        ui.write(_('(empty stack; working directory parent is a published '
                   'changeset)\n'))
        return

    # TODO extract "find stack" into a function to facilitate
    # customization and reuse.

    baserev = destutil.stackbase(ui, repo)
    basectx = None

    if baserev is None:
        baserev = wdirctx.rev()
        stackrevs = {wdirctx.rev()}
    else:
        stackrevs = set(repo.revs('%d::.', baserev))

    ctx = repo[baserev]
    if ctx.p1().rev() != nullrev:
        basectx = ctx.p1()

    # And relevant descendants.
    branchpointattip = False
    cl = repo.changelog

    for rev in cl.descendants([wdirctx.rev()]):
        ctx = repo[rev]

        # Will only happen if . is public.
        if ctx.phase() == phases.public:
            break

        stackrevs.add(ctx.rev())

        # ctx.children() within a function iterating on descandants
        # potentially has severe performance concerns because revlog.children()
        # iterates over all revisions after ctx's node. However, the number of
        # draft changesets should be a reasonably small number. So even if
        # this is quadratic, the perf impact should be minimal.
        if len(ctx.children()) > 1:
            branchpointattip = True
            break

    stackrevs = list(sorted(stackrevs, reverse=True))

    # Find likely target heads for the current stack. These are likely
    # merge or rebase targets.
    if basectx:
        # TODO make this customizable?
        newheads = set(repo.revs('heads(%d::) - %ld - not public()',
                                 basectx.rev(), stackrevs))
    else:
        newheads = set()

    allrevs = set(stackrevs) | newheads | set([baserev])
    nodelen = longestshortest(repo, allrevs)

    try:
        cmdutil.findcmd('rebase', commands.table)
        haverebase = True
    except (error.AmbiguousCommand, error.UnknownCommand):
        haverebase = False

    # TODO use templating.
    # TODO consider using graphmod. But it may not be necessary given
    # our simplicity and the customizations required.
    # TODO use proper graph symbols from graphmod

    shortesttmpl = formatter.maketemplater(ui, '{shortest(node, %d)}' % nodelen)
    def shortest(ctx):
        return shortesttmpl.render({'ctx': ctx, 'node': ctx.hex()})

    # We write out new heads to aid in DAG awareness and to help with decision
    # making on how the stack should be reconciled with commits made since the
    # branch point.
    if newheads:
        # Calculate distance from base so we can render the count and so we can
        # sort display order by commit distance.
        revdistance = {}
        for head in newheads:
            # There is some redundancy in DAG traversal here and therefore
            # room to optimize.
            ancestors = cl.ancestors([head], stoprev=basectx.rev())
            revdistance[head] = len(list(ancestors))

        sourcectx = repo[stackrevs[-1]]

        sortedheads = sorted(newheads, key=lambda x: revdistance[x],
                             reverse=True)

        for i, rev in enumerate(sortedheads):
            ctx = repo[rev]

            if i:
                ui.write(': ')
            else:
                ui.write('  ')

            ui.write(('o  '))
            displayer.show(ctx, nodelen=nodelen)
            displayer.flush(ctx)
            ui.write('\n')

            if i:
                ui.write(':/')
            else:
                ui.write(' /')

            ui.write('    (')
            ui.write(_('%d commits ahead') % revdistance[rev],
                     label='stack.commitdistance')

            if haverebase:
                # TODO may be able to omit --source in some scenarios
                ui.write('; ')
                ui.write(('hg rebase --source %s --dest %s' % (
                         shortest(sourcectx), shortest(ctx))),
                         label='stack.rebasehint')

            ui.write(')\n')

        ui.write(':\n:    ')
        ui.write(_('(stack head)\n'), label='stack.label')

    if branchpointattip:
        ui.write(' \\ /  ')
        ui.write(_('(multiple children)\n'), label='stack.label')
        ui.write('  |\n')

    for rev in stackrevs:
        ctx = repo[rev]
        symbol = '@' if rev == wdirctx.rev() else 'o'

        if newheads:
            ui.write(': ')
        else:
            ui.write('  ')

        ui.write(symbol, '  ')
        displayer.show(ctx, nodelen=nodelen)
        displayer.flush(ctx)
        ui.write('\n')

    # TODO display histedit hint?

    if basectx:
        # Vertically and horizontally separate stack base from parent
        # to reinforce stack boundary.
        if newheads:
            ui.write(':/   ')
        else:
            ui.write(' /   ')

        ui.write(_('(stack base)'), '\n', label='stack.label')
        ui.write(('o  '))

        displayer.show(basectx, nodelen=nodelen)
        displayer.flush(basectx)
        ui.write('\n')

@revsetpredicate('_underway([commitage[, headage]])')
def underwayrevset(repo, subset, x):
    args = revset.getargsdict(x, 'underway', 'commitage headage')
    if 'commitage' not in args:
        args['commitage'] = None
    if 'headage' not in args:
        args['headage'] = None

    # We assume callers of this revset add a topographical sort on the
    # result. This means there is no benefit to making the revset lazy
    # since the topographical sort needs to consume all revs.
    #
    # With this in mind, we build up the set manually instead of constructing
    # a complex revset. This enables faster execution.

    # Mutable changesets (non-public) are the most important changesets
    # to return. ``not public()`` will also pull in obsolete changesets if
    # there is a non-obsolete changeset with obsolete ancestors. This is
    # why we exclude obsolete changesets from this query.
    rs = 'not public() and not obsolete()'
    rsargs = []
    if args['commitage']:
        rs += ' and date(%s)'
        rsargs.append(revsetlang.getstring(args['commitage'],
                                           _('commitage requires a string')))

    mutable = repo.revs(rs, *rsargs)
    relevant = revset.baseset(mutable)

    # Add parents of mutable changesets to provide context.
    relevant += repo.revs('parents(%ld)', mutable)

    # We also pull in (public) heads if they a) aren't closing a branch
    # b) are recent.
    rs = 'head() and not closed()'
    rsargs = []
    if args['headage']:
        rs += ' and date(%s)'
        rsargs.append(revsetlang.getstring(args['headage'],
                                           _('headage requires a string')))

    relevant += repo.revs(rs, *rsargs)

    # Add working directory parent.
    wdirrev = repo['.'].rev()
    if wdirrev != nullrev:
        relevant += revset.baseset({wdirrev})

    return subset & relevant

@showview('work', csettopic='work')
def showwork(ui, repo, displayer):
    """changesets that aren't finished"""
    # TODO support date-based limiting when calling revset.
    revs = repo.revs('sort(_underway(), topo)')
    nodelen = longestshortest(repo, revs)

    revdag = graphmod.dagwalker(repo, revs)

    ui.setconfig('experimental', 'graphshorten', True)
    cmdutil.displaygraph(ui, repo, revdag, displayer, graphmod.asciiedges,
                         props={'nodelen': nodelen})

def extsetup(ui):
    # Alias `hg <prefix><view>` to `hg show <view>`.
    for prefix in ui.configlist('commands', 'show.aliasprefix'):
        for view in showview._table:
            name = '%s%s' % (prefix, view)

            choice, allcommands = cmdutil.findpossible(name, commands.table,
                                                       strict=True)

            # This alias is already a command name. Don't set it.
            if name in choice:
                continue

            # Same for aliases.
            if ui.config('alias', name):
                continue

            ui.setconfig('alias', name, 'show %s' % view, source='show')

def longestshortest(repo, revs, minlen=4):
    """Return the length of the longest shortest node to identify revisions.

    The result of this function can be used with the ``shortest()`` template
    function to ensure that a value is unique and unambiguous for a given
    set of nodes.

    The number of revisions in the repo is taken into account to prevent
    a numeric node prefix from conflicting with an integer revision number.
    If we fail to do this, a value of e.g. ``10023`` could mean either
    revision 10023 or node ``10023abc...``.
    """
    tmpl = formatter.maketemplater(repo.ui, '{shortest(node, %d)}' % minlen)
    lens = [minlen]
    for rev in revs:
        ctx = repo[rev]
        shortest = tmpl.render({'ctx': ctx, 'node': ctx.hex()})
        lens.append(len(shortest))

    return max(lens)

# Adjust the docstring of the show command so it shows all registered views.
# This is a bit hacky because it runs at the end of module load. When moved
# into core or when another extension wants to provide a view, we'll need
# to do this more robustly.
# TODO make this more robust.
def _updatedocstring():
    longest = max(map(len, showview._table.keys()))
    entries = []
    for key in sorted(showview._table.keys()):
        entries.append(pycompat.sysstr('    %s   %s' % (
            key.ljust(longest), showview._table[key]._origdoc)))

    cmdtable['show'][0].__doc__ = pycompat.sysstr('%s\n\n%s\n    ') % (
        cmdtable['show'][0].__doc__.rstrip(),
        pycompat.sysstr('\n\n').join(entries))

_updatedocstring()
