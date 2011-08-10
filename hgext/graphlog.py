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
from mercurial.commands import templateopts
from mercurial.i18n import _
from mercurial.node import nullrev
from mercurial import cmdutil, commands, extensions, scmutil
from mercurial import hg, util, graphmod

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
    for op in ["follow_first", "copies", "newest_first"]:
        if op in opts and opts[op]:
            raise util.Abort(_("-G/--graph option is incompatible with --%s")
                             % op.replace("_", "-"))
    if pats and opts.get('follow'):
        raise util.Abort(_("-G/--graph option is incompatible with --follow "
                           "with file argument"))

def revset(pats, opts):
    """Return revset str built of revisions, log options and file patterns.
    """
    opt2revset = {
        'follow': (0, 'follow()'),
        'no_merges': (0, 'not merge()'),
        'only_merges': (0, 'merge()'),
        'removed': (0, 'removes("*")'),
        'date': (1, 'date($)'),
        'branch': (2, 'branch($)'),
        'exclude': (2, 'not file($)'),
        'include': (2, 'file($)'),
        'keyword': (2, 'keyword($)'),
        'only_branch': (2, 'branch($)'),
        'prune': (2, 'not ($ or ancestors($))'),
        'user': (2, 'user($)'),
        }
    optrevset = []
    revset = []
    for op, val in opts.iteritems():
        if not val:
            continue
        if op == 'rev':
            # Already a revset
            revset.extend(val)
        if op not in opt2revset:
            continue
        arity, revop = opt2revset[op]
        revop = revop.replace('$', '%(val)r')
        if arity == 0:
            optrevset.append(revop)
        elif arity == 1:
            optrevset.append(revop % {'val': val})
        else:
            for f in val:
                optrevset.append(revop % {'val': f})

    for path in pats:
        optrevset.append('file(%r)' % path)

    if revset or optrevset:
        if revset:
            revset = ['(' + ' or '.join(revset) + ')']
        if optrevset:
            revset.append('(' + ' and '.join(optrevset) + ')')
        revset = ' and '.join(revset)
    else:
        revset = 'all()'
    return revset

def generate(ui, dag, displayer, showparents, edgefn):
    seen, state = [], asciistate()
    for rev, type, ctx, parents in dag:
        char = ctx.node() in showparents and '@' or 'o'
        displayer.show(ctx)
        lines = displayer.hunk.pop(rev).split('\n')[:-1]
        displayer.flush(rev)
        edges = edgefn(type, char, lines, seen, rev, parents)
        for type, char, lines, coldata in edges:
            ascii(ui, state, type, char, lines, coldata)
    displayer.close()

@command('glog',
    [('l', 'limit', '',
     _('limit number of changes displayed'), _('NUM')),
    ('p', 'patch', False, _('show patch')),
    ('r', 'rev', [], _('show the specified revision or range'), _('REV')),
    ] + templateopts,
    _('hg glog [OPTION]... [FILE]'))
def graphlog(ui, repo, *pats, **opts):
    """show revision history alongside an ASCII revision graph

    Print a revision history alongside a revision graph drawn with
    ASCII characters.

    Nodes printed as an @ character are parents of the working
    directory.
    """

    check_unsupported_flags(pats, opts)

    revs = sorted(scmutil.revrange(repo, [revset(pats, opts)]), reverse=1)
    limit = cmdutil.loglimit(opts)
    if limit is not None:
        revs = revs[:limit]
    revdag = graphmod.dagwalker(repo, revs)

    displayer = show_changeset(ui, repo, opts, buffered=True)
    showparents = [ctx.node() for ctx in repo[None].parents()]
    generate(ui, revdag, displayer, showparents, asciiedges)

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
