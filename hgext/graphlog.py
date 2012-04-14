# ASCII graph log extension for Mercurial
#
# Copyright 2007 Joel Rosdahl <joel@rosdahl.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''command to view revision graphs from a shell

This extension adds a --graph option to the incoming, outgoing and log
commands. When this options is given, an ASCII representation of the
revision graph is also shown.
'''

from mercurial.cmdutil import show_changeset
from mercurial.i18n import _
from mercurial.node import nullrev
from mercurial import cmdutil, commands, extensions, scmutil
from mercurial import hg, util, graphmod, templatekw, revset

cmdtable = {}
command = cmdutil.command(cmdtable)

ASCIIDATA = 'ASC'

def asciiedges(type, char, lines, seen, rev, parents):
    """adds edge info to changelog DAG walk suitable for ascii()"""
    if rev not in seen:
        seen.append(rev)
    nodeidx = seen.index(rev)

    knownparents = []
    newparents = []
    for parent in parents:
        if parent in seen:
            knownparents.append(parent)
        else:
            newparents.append(parent)

    ncols = len(seen)
    nextseen = seen[:]
    nextseen[nodeidx:nodeidx + 1] = newparents
    edges = [(nodeidx, nextseen.index(p)) for p in knownparents]

    while len(newparents) > 2:
        # ascii() only knows how to add or remove a single column between two
        # calls. Nodes with more than two parents break this constraint so we
        # introduce intermediate expansion lines to grow the active node list
        # slowly.
        edges.append((nodeidx, nodeidx))
        edges.append((nodeidx, nodeidx + 1))
        nmorecols = 1
        yield (type, char, lines, (nodeidx, edges, ncols, nmorecols))
        char = '\\'
        lines = []
        nodeidx += 1
        ncols += 1
        edges = []
        del newparents[0]

    if len(newparents) > 0:
        edges.append((nodeidx, nodeidx))
    if len(newparents) > 1:
        edges.append((nodeidx, nodeidx + 1))
    nmorecols = len(nextseen) - ncols
    seen[:] = nextseen
    yield (type, char, lines, (nodeidx, edges, ncols, nmorecols))

def fix_long_right_edges(edges):
    for (i, (start, end)) in enumerate(edges):
        if end > start:
            edges[i] = (start, end + 1)

def get_nodeline_edges_tail(
        node_index, p_node_index, n_columns, n_columns_diff, p_diff, fix_tail):
    if fix_tail and n_columns_diff == p_diff and n_columns_diff != 0:
        # Still going in the same non-vertical direction.
        if n_columns_diff == -1:
            start = max(node_index + 1, p_node_index)
            tail = ["|", " "] * (start - node_index - 1)
            tail.extend(["/", " "] * (n_columns - start))
            return tail
        else:
            return ["\\", " "] * (n_columns - node_index - 1)
    else:
        return ["|", " "] * (n_columns - node_index - 1)

def draw_edges(edges, nodeline, interline):
    for (start, end) in edges:
        if start == end + 1:
            interline[2 * end + 1] = "/"
        elif start == end - 1:
            interline[2 * start + 1] = "\\"
        elif start == end:
            interline[2 * start] = "|"
        else:
            if 2 * end >= len(nodeline):
                continue
            nodeline[2 * end] = "+"
            if start > end:
                (start, end) = (end, start)
            for i in range(2 * start + 1, 2 * end):
                if nodeline[i] != "+":
                    nodeline[i] = "-"

def get_padding_line(ni, n_columns, edges):
    line = []
    line.extend(["|", " "] * ni)
    if (ni, ni - 1) in edges or (ni, ni) in edges:
        # (ni, ni - 1)      (ni, ni)
        # | | | |           | | | |
        # +---o |           | o---+
        # | | c |           | c | |
        # | |/ /            | |/ /
        # | | |             | | |
        c = "|"
    else:
        c = " "
    line.extend([c, " "])
    line.extend(["|", " "] * (n_columns - ni - 1))
    return line

def asciistate():
    """returns the initial value for the "state" argument to ascii()"""
    return [0, 0]

def ascii(ui, state, type, char, text, coldata):
    """prints an ASCII graph of the DAG

    takes the following arguments (one call per node in the graph):

      - ui to write to
      - Somewhere to keep the needed state in (init to asciistate())
      - Column of the current node in the set of ongoing edges.
      - Type indicator of node data == ASCIIDATA.
      - Payload: (char, lines):
        - Character to use as node's symbol.
        - List of lines to display as the node's text.
      - Edges; a list of (col, next_col) indicating the edges between
        the current node and its parents.
      - Number of columns (ongoing edges) in the current revision.
      - The difference between the number of columns (ongoing edges)
        in the next revision and the number of columns (ongoing edges)
        in the current revision. That is: -1 means one column removed;
        0 means no columns added or removed; 1 means one column added.
    """

    idx, edges, ncols, coldiff = coldata
    assert -2 < coldiff < 2
    if coldiff == -1:
        # Transform
        #
        #     | | |        | | |
        #     o | |  into  o---+
        #     |X /         |/ /
        #     | |          | |
        fix_long_right_edges(edges)

    # add_padding_line says whether to rewrite
    #
    #     | | | |        | | | |
    #     | o---+  into  | o---+
    #     |  / /         |   | |  # <--- padding line
    #     o | |          |  / /
    #                    o | |
    add_padding_line = (len(text) > 2 and coldiff == -1 and
                        [x for (x, y) in edges if x + 1 < y])

    # fix_nodeline_tail says whether to rewrite
    #
    #     | | o | |        | | o | |
    #     | | |/ /         | | |/ /
    #     | o | |    into  | o / /   # <--- fixed nodeline tail
    #     | |/ /           | |/ /
    #     o | |            o | |
    fix_nodeline_tail = len(text) <= 2 and not add_padding_line

    # nodeline is the line containing the node character (typically o)
    nodeline = ["|", " "] * idx
    nodeline.extend([char, " "])

    nodeline.extend(
        get_nodeline_edges_tail(idx, state[1], ncols, coldiff,
                                state[0], fix_nodeline_tail))

    # shift_interline is the line containing the non-vertical
    # edges between this entry and the next
    shift_interline = ["|", " "] * idx
    if coldiff == -1:
        n_spaces = 1
        edge_ch = "/"
    elif coldiff == 0:
        n_spaces = 2
        edge_ch = "|"
    else:
        n_spaces = 3
        edge_ch = "\\"
    shift_interline.extend(n_spaces * [" "])
    shift_interline.extend([edge_ch, " "] * (ncols - idx - 1))

    # draw edges from the current node to its parents
    draw_edges(edges, nodeline, shift_interline)

    # lines is the list of all graph lines to print
    lines = [nodeline]
    if add_padding_line:
        lines.append(get_padding_line(idx, ncols, edges))
    lines.append(shift_interline)

    # make sure that there are as many graph lines as there are
    # log strings
    while len(text) < len(lines):
        text.append("")
    if len(lines) < len(text):
        extra_interline = ["|", " "] * (ncols + coldiff)
        while len(lines) < len(text):
            lines.append(extra_interline)

    # print lines
    indentation_level = max(ncols, ncols + coldiff)
    for (line, logstr) in zip(lines, text):
        ln = "%-*s %s" % (2 * indentation_level, "".join(line), logstr)
        ui.write(ln.rstrip() + '\n')

    # ... and start over
    state[0] = coldiff
    state[1] = idx

def get_revs(repo, rev_opt):
    if rev_opt:
        revs = scmutil.revrange(repo, rev_opt)
        if len(revs) == 0:
            return (nullrev, nullrev)
        return (max(revs), min(revs))
    else:
        return (len(repo) - 1, 0)

def check_unsupported_flags(pats, opts):
    for op in ["newest_first"]:
        if op in opts and opts[op]:
            raise util.Abort(_("-G/--graph option is incompatible with --%s")
                             % op.replace("_", "-"))

def _makefilematcher(repo, pats, followfirst):
    # When displaying a revision with --patch --follow FILE, we have
    # to know which file of the revision must be diffed. With
    # --follow, we want the names of the ancestors of FILE in the
    # revision, stored in "fcache". "fcache" is populated by
    # reproducing the graph traversal already done by --follow revset
    # and relating linkrevs to file names (which is not "correct" but
    # good enough).
    fcache = {}
    fcacheready = [False]
    pctx = repo['.']
    wctx = repo[None]

    def populate():
        for fn in pats:
            for i in ((pctx[fn],), pctx[fn].ancestors(followfirst=followfirst)):
                for c in i:
                    fcache.setdefault(c.linkrev(), set()).add(c.path())

    def filematcher(rev):
        if not fcacheready[0]:
            # Lazy initialization
            fcacheready[0] = True
            populate()
        return scmutil.match(wctx, fcache.get(rev, []), default='path')

    return filematcher

def _makelogrevset(repo, pats, opts, revs):
    """Return (expr, filematcher) where expr is a revset string built
    from log options and file patterns or None. If --stat or --patch
    are not passed filematcher is None. Otherwise it is a callable
    taking a revision number and returning a match objects filtering
    the files to be detailed when displaying the revision.
    """
    opt2revset = {
        'no_merges':        ('not merge()', None),
        'only_merges':      ('merge()', None),
        '_ancestors':       ('ancestors(%(val)s)', None),
        '_fancestors':      ('_firstancestors(%(val)s)', None),
        '_descendants':     ('descendants(%(val)s)', None),
        '_fdescendants':    ('_firstdescendants(%(val)s)', None),
        '_matchfiles':      ('_matchfiles(%(val)s)', None),
        'date':             ('date(%(val)r)', None),
        'branch':           ('branch(%(val)r)', ' or '),
        '_patslog':         ('filelog(%(val)r)', ' or '),
        '_patsfollow':      ('follow(%(val)r)', ' or '),
        '_patsfollowfirst': ('_followfirst(%(val)r)', ' or '),
        'keyword':          ('keyword(%(val)r)', ' or '),
        'prune':            ('not (%(val)r or ancestors(%(val)r))', ' and '),
        'user':             ('user(%(val)r)', ' or '),
        }

    opts = dict(opts)
    # follow or not follow?
    follow = opts.get('follow') or opts.get('follow_first')
    followfirst = opts.get('follow_first') and 1 or 0
    # --follow with FILE behaviour depends on revs...
    startrev = revs[0]
    followdescendants = (len(revs) > 1 and revs[0] < revs[1]) and 1 or 0

    # branch and only_branch are really aliases and must be handled at
    # the same time
    opts['branch'] = opts.get('branch', []) + opts.get('only_branch', [])
    opts['branch'] = [repo.lookupbranch(b) for b in opts['branch']]
    # pats/include/exclude are passed to match.match() directly in
    # _matchfile() revset but walkchangerevs() builds its matcher with
    # scmutil.match(). The difference is input pats are globbed on
    # platforms without shell expansion (windows).
    pctx = repo[None]
    match, pats = scmutil.matchandpats(pctx, pats, opts)
    slowpath = match.anypats() or (match.files() and opts.get('removed'))
    if not slowpath:
        for f in match.files():
            if follow and f not in pctx:
                raise util.Abort(_('cannot follow file not in parent '
                                   'revision: "%s"') % f)
            filelog = repo.file(f)
            if not len(filelog):
                # A zero count may be a directory or deleted file, so
                # try to find matching entries on the slow path.
                if follow:
                    raise util.Abort(
                        _('cannot follow nonexistent file: "%s"') % f)
                slowpath = True
    if slowpath:
        # See cmdutil.walkchangerevs() slow path.
        #
        if follow:
            raise util.Abort(_('can only follow copies/renames for explicit '
                               'filenames'))
        # pats/include/exclude cannot be represented as separate
        # revset expressions as their filtering logic applies at file
        # level. For instance "-I a -X a" matches a revision touching
        # "a" and "b" while "file(a) and not file(b)" does
        # not. Besides, filesets are evaluated against the working
        # directory.
        matchargs = ['r:', 'd:relpath']
        for p in pats:
            matchargs.append('p:' + p)
        for p in opts.get('include', []):
            matchargs.append('i:' + p)
        for p in opts.get('exclude', []):
            matchargs.append('x:' + p)
        matchargs = ','.join(('%r' % p) for p in matchargs)
        opts['_matchfiles'] = matchargs
    else:
        if follow:
            fpats = ('_patsfollow', '_patsfollowfirst')
            fnopats = (('_ancestors', '_fancestors'),
                       ('_descendants', '_fdescendants'))
            if pats:
                # follow() revset inteprets its file argument as a
                # manifest entry, so use match.files(), not pats.
                opts[fpats[followfirst]] = list(match.files())
            else:
                opts[fnopats[followdescendants][followfirst]] = str(startrev)
        else:
            opts['_patslog'] = list(pats)

    filematcher = None
    if opts.get('patch') or opts.get('stat'):
        if follow:
            filematcher = _makefilematcher(repo, pats, followfirst)
        else:
            filematcher = lambda rev: match

    expr = []
    for op, val in opts.iteritems():
        if not val:
            continue
        if op not in opt2revset:
            continue
        revop, andor = opt2revset[op]
        if '%(val)' not in revop:
            expr.append(revop)
        else:
            if not isinstance(val, list):
                e = revop % {'val': val}
            else:
                e = '(' + andor.join((revop % {'val': v}) for v in val) + ')'
            expr.append(e)

    if expr:
        expr = '(' + ' and '.join(expr) + ')'
    else:
        expr = None
    return expr, filematcher

def getlogrevs(repo, pats, opts):
    """Return (revs, expr, filematcher) where revs is a list of
    revision numbers, expr is a revset string built from log options
    and file patterns or None, and used to filter 'revs'. If --stat or
    --patch are not passed filematcher is None. Otherwise it is a
    callable taking a revision number and returning a match objects
    filtering the files to be detailed when displaying the revision.
    """
    if not len(repo):
        return [], None, None
    # Default --rev value depends on --follow but --follow behaviour
    # depends on revisions resolved from --rev...
    follow = opts.get('follow') or opts.get('follow_first')
    if opts.get('rev'):
        revs = scmutil.revrange(repo, opts['rev'])
    else:
        if follow and len(repo) > 0:
            revs = scmutil.revrange(repo, ['.:0'])
        else:
            revs = range(len(repo) - 1, -1, -1)
    if not revs:
        return [], None, None
    expr, filematcher = _makelogrevset(repo, pats, opts, revs)
    if expr:
        # Evaluate revisions in changelog order for performance
        # reasons but preserve the original sequence order in the
        # filtered result.
        matched = set(revset.match(repo.ui, expr)(repo, sorted(revs)))
        revs = [r for r in revs if r in matched]
    if not opts.get('hidden'):
        # --hidden is still experimental and not worth a dedicated revset
        # yet. Fortunately, filtering revision number is fast.
        revs = [r for r in revs if r not in repo.changelog.hiddenrevs]
    return revs, expr, filematcher

def generate(ui, dag, displayer, showparents, edgefn, getrenamed=None,
             filematcher=None):
    seen, state = [], asciistate()
    for rev, type, ctx, parents in dag:
        char = ctx.node() in showparents and '@' or 'o'
        copies = None
        if getrenamed and ctx.rev():
            copies = []
            for fn in ctx.files():
                rename = getrenamed(fn, ctx.rev())
                if rename:
                    copies.append((fn, rename[0]))
        revmatchfn = None
        if filematcher is not None:
            revmatchfn = filematcher(ctx.rev())
        displayer.show(ctx, copies=copies, matchfn=revmatchfn)
        lines = displayer.hunk.pop(rev).split('\n')[:-1]
        displayer.flush(rev)
        edges = edgefn(type, char, lines, seen, rev, parents)
        for type, char, lines, coldata in edges:
            ascii(ui, state, type, char, lines, coldata)
    displayer.close()

@command('glog',
    [('f', 'follow', None,
     _('follow changeset history, or file history across copies and renames')),
    ('', 'follow-first', None,
     _('only follow the first parent of merge changesets (DEPRECATED)')),
    ('d', 'date', '', _('show revisions matching date spec'), _('DATE')),
    ('C', 'copies', None, _('show copied files')),
    ('k', 'keyword', [],
     _('do case-insensitive search for a given text'), _('TEXT')),
    ('r', 'rev', [], _('show the specified revision or range'), _('REV')),
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
    ('', 'hidden', False, _('show hidden changesets (DEPRECATED)')),
    ] + commands.logopts + commands.walkopts,
    _('[OPTION]... [FILE]'))
def graphlog(ui, repo, *pats, **opts):
    """show revision history alongside an ASCII revision graph

    Print a revision history alongside a revision graph drawn with
    ASCII characters.

    Nodes printed as an @ character are parents of the working
    directory.
    """

    revs, expr, filematcher = getlogrevs(repo, pats, opts)
    revs = sorted(revs, reverse=1)
    limit = cmdutil.loglimit(opts)
    if limit is not None:
        revs = revs[:limit]
    revdag = graphmod.dagwalker(repo, revs)

    getrenamed = None
    if opts.get('copies'):
        endrev = None
        if opts.get('rev'):
            endrev = max(scmutil.revrange(repo, opts.get('rev'))) + 1
        getrenamed = templatekw.getrenamedfn(repo, endrev=endrev)
    displayer = show_changeset(ui, repo, opts, buffered=True)
    showparents = [ctx.node() for ctx in repo[None].parents()]
    generate(ui, revdag, displayer, showparents, asciiedges, getrenamed,
             filematcher)

def graphrevs(repo, nodes, opts):
    limit = cmdutil.loglimit(opts)
    nodes.reverse()
    if limit is not None:
        nodes = nodes[:limit]
    return graphmod.nodes(repo, nodes)

def goutgoing(ui, repo, dest=None, **opts):
    """show the outgoing changesets alongside an ASCII revision graph

    Print the outgoing changesets alongside a revision graph drawn with
    ASCII characters.

    Nodes printed as an @ character are parents of the working
    directory.
    """

    check_unsupported_flags([], opts)
    o = hg._outgoing(ui, repo, dest, opts)
    if o is None:
        return

    revdag = graphrevs(repo, o, opts)
    displayer = show_changeset(ui, repo, opts, buffered=True)
    showparents = [ctx.node() for ctx in repo[None].parents()]
    generate(ui, revdag, displayer, showparents, asciiedges)

def gincoming(ui, repo, source="default", **opts):
    """show the incoming changesets alongside an ASCII revision graph

    Print the incoming changesets alongside a revision graph drawn with
    ASCII characters.

    Nodes printed as an @ character are parents of the working
    directory.
    """
    def subreporecurse():
        return 1

    check_unsupported_flags([], opts)
    def display(other, chlist, displayer):
        revdag = graphrevs(other, chlist, opts)
        showparents = [ctx.node() for ctx in repo[None].parents()]
        generate(ui, revdag, displayer, showparents, asciiedges)

    hg._incoming(display, subreporecurse, ui, repo, source, opts, buffered=True)

def uisetup(ui):
    '''Initialize the extension.'''
    _wrapcmd('log', commands.table, graphlog)
    _wrapcmd('incoming', commands.table, gincoming)
    _wrapcmd('outgoing', commands.table, goutgoing)

def _wrapcmd(cmd, table, wrapfn):
    '''wrap the command'''
    def graph(orig, *args, **kwargs):
        if kwargs['graph']:
            return wrapfn(*args, **kwargs)
        return orig(*args, **kwargs)
    entry = extensions.wrapcommand(table, cmd, graph)
    entry[1].append(('G', 'graph', None, _("show the revision DAG")))
