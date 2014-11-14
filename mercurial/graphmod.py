# Revision graph generator for Mercurial
#
# Copyright 2008 Dirkjan Ochtman <dirkjan@ochtman.nl>
# Copyright 2007 Joel Rosdahl <joel@rosdahl.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""supports walking the history as DAGs suitable for graphical output

The most basic format we use is that of::

  (id, type, data, [parentids])

The node and parent ids are arbitrary integers which identify a node in the
context of the graph returned. Type is a constant specifying the node type.
Data depends on type.
"""

from mercurial.node import nullrev
import util

CHANGESET = 'C'

def groupbranchiter(revs, parentsfunc):
    """yield revision from heads to roots one (topo) branch after the other.

    This function aims to be used by a graph generator that wishes to minimize
    the amount of parallel branches and their interleaving.

    Example iteration order:

      o  4
      |
      o  1
      |
      | o  3
      | |
      | o  2
      |/
      o  0

    Currently does not handle non-contiguous <revs> input.

    Currently consider every changeset under a merge to be on the same branch
    using revision number to sort them.

    Could be easily extend to give priority to an initial branch."""
    ### Quick summary of the algorithm
    #
    # This function is based around a "retention" principle. We keep revisions
    # in memory until we are ready to emit a whole branch that immediately
    # "merge" into an existing one. This reduce the number of branch "ongoing"
    # at the same time.
    #
    # During iteration revs are split into two groups:
    # A) revision already emitted
    # B) revision in "retention". They are stored as different subgroups.
    #
    # for each REV, we do the follow logic:
    #
    #   a) if REV is a parent of (A), we will emit it. But before emitting it,
    #   we'll "free" all the revs from subgroup in (B) that were waiting for
    #   REV to be available. So we emit all revision of such subgroup before
    #   emitting REV
    #
    #   b) else, we'll search for a subgroup in (B) awaiting for REV to be
    #   available, if such subgroup exist, we add REV to it and the subgroup is
    #   now awaiting for REV.parents() to be available.
    #
    #   c) finally if no such group existed in (B), we create a new subgroup.
    #
    #
    # To bootstrap the algorithm, we emit the tipmost revision.

    revs.sort(reverse=True)

    # Set of parents of revision that have been yield. They can be considered
    # unblocked as the graph generator is already aware of them so there is no
    # need to delay the one that reference them.
    unblocked = set()

    # list of group waiting to be displayed, each group is defined by:
    #
    #   (revs:    lists of revs waiting to be displayed,
    #    blocked: set of that cannot be displayed before those in 'revs')
    #
    # The second value ('blocked') correspond to parents of any revision in the
    # group ('revs') that is not itself contained in the group. The main idea
    # of this algorithm is to delay as much as possible the emission of any
    # revision.  This means waiting for the moment we are about to display
    # theses parents to display the revs in a group.
    #
    # This first implementation is smart until it meet a merge: it will emit
    # revs as soon as any parents is about to be emitted and can grow an
    # arbitrary number of revs in 'blocked'. In practice this mean we properly
    # retains new branches but give up on any special ordering for ancestors of
    # merges. The implementation can be improved to handle this better.
    #
    # The first subgroup is special. It correspond to all the revision that
    # were already emitted. The 'revs' lists is expected to be empty and the
    # 'blocked' set contains the parents revisions of already emitted revision.
    #
    # You could pre-seed the <parents> set of groups[0] to a specific
    # changesets to select what the first emitted branch should be.
    #
    # We do not support revisions will hole yet, but adding such support would
    # be easy. The iteration will have to be done using both input revision and
    # parents (see cl.ancestors function + a few tweaks) but only revisions
    # parts of the initial set should be emitted.
    groups = [([], unblocked)]
    for current in revs:
        # Look for a subgroup blocked, waiting for the current revision.
        matching = [i for i, g in enumerate(groups) if current in g[1]]

        if matching:
            # The main idea is to gather together all sets that await on the
            # same revision.
            #
            # This merging is done at the time we are about to add this common
            # awaited to the subgroup for simplicity purpose. Such merge could
            # happen sooner when we update the "blocked" set of revision.
            #
            # We also always keep the oldest subgroup first. We can probably
            # improve the behavior by having the longuest set first. That way,
            # graph algorythms could minimise the length of parallele lines
            # their draw. This is currently not done.
            targetidx = matching.pop(0)
            trevs, tparents = groups[targetidx]
            for i in matching:
                gr = groups[i]
                trevs.extend(gr[0])
                tparents |= gr[1]
            # delete all merged subgroups (but the one we keep)
            # (starting from the last subgroup for performance and sanity reason)
            for i in reversed(matching):
                del groups[i]
        else:
            # This is a new head. We create a new subgroup for it.
            targetidx = len(groups)
            groups.append(([], set([current])))

        gr = groups[targetidx]

        # We now adds the current nodes to this subgroups. This is done after
        # the subgroup merging because all elements from a subgroup that relied
        # on this rev must preceed it.
        #
        # we also update the <parents> set to includes the parents on the
        # new nodes.
        gr[0].append(current)
        gr[1].remove(current)
        gr[1].update([p for p in parentsfunc(current) if p > nullrev])

        # Look for a subgroup to display
        #
        # When unblocked is empty (if clause), We are not waiting over any
        # revision during the first iteration (if no priority was given) or if
        # we outputed a whole disconnected sets of the graph (reached a root).
        # In that case we arbitrarily takes the oldest known subgroup. The
        # heuristique could probably be better.
        #
        # Otherwise (elif clause) this mean we have some emitted revision.  if
        # the subgroup awaits on the same revision that the outputed ones, we
        # can safely output it.
        if not unblocked:
            if len(groups) > 1:  # display other subset
                targetidx = 1
                gr = groups[1]
        elif not gr[1] & unblocked:
            gr = None

        if gr is not None:
            # update the set of awaited revisions with the one from the
            # subgroup
            unblocked |= gr[1]
            # output all revisions in the subgroup
            for r in gr[0]:
                yield r
            # delete the subgroup that you just output
            # unless it is groups[0] in which case you just empty it.
            if targetidx:
                del groups[targetidx]
            else:
                gr[0][:] = []

def dagwalker(repo, revs):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

    This generator function walks through revisions (which should be ordered
    from bigger to lower). It returns a tuple for each node. The node and parent
    ids are arbitrary integers which identify a node in the context of the graph
    returned.
    """
    if not revs:
        return

    cl = repo.changelog
    lowestrev = revs.min()
    gpcache = {}

    if repo.ui.configbool('experimental', 'graph-topological', False):
        revs = list(groupbranchiter(revs, repo.changelog.parentrevs))

    for rev in revs:
        ctx = repo[rev]
        parents = sorted(set([p.rev() for p in ctx.parents()
                              if p.rev() in revs]))
        mpars = [p.rev() for p in ctx.parents() if
                 p.rev() != nullrev and p.rev() not in parents]

        for mpar in mpars:
            gp = gpcache.get(mpar)
            if gp is None:
                gp = gpcache[mpar] = grandparent(cl, lowestrev, revs, mpar)
            if not gp:
                parents.append(mpar)
            else:
                parents.extend(g for g in gp if g not in parents)

        yield (ctx.rev(), CHANGESET, ctx, parents)

def nodes(repo, nodes):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

    This generator function walks the given nodes. It only returns parents
    that are in nodes, too.
    """
    include = set(nodes)
    for node in nodes:
        ctx = repo[node]
        parents = set([p.rev() for p in ctx.parents() if p.node() in include])
        yield (ctx.rev(), CHANGESET, ctx, sorted(parents))

def colored(dag, repo):
    """annotates a DAG with colored edge information

    For each DAG node this function emits tuples::

      (id, type, data, (col, color), [(col, nextcol, color)])

    with the following new elements:

      - Tuple (col, color) with column and color index for the current node
      - A list of tuples indicating the edges between the current node and its
        parents.
    """
    seen = []
    colors = {}
    newcolor = 1
    config = {}

    for key, val in repo.ui.configitems('graph'):
        if '.' in key:
            branch, setting = key.rsplit('.', 1)
            # Validation
            if setting == "width" and val.isdigit():
                config.setdefault(branch, {})[setting] = int(val)
            elif setting == "color" and val.isalnum():
                config.setdefault(branch, {})[setting] = val

    if config:
        getconf = util.lrucachefunc(
            lambda rev: config.get(repo[rev].branch(), {}))
    else:
        getconf = lambda rev: {}

    for (cur, type, data, parents) in dag:

        # Compute seen and next
        if cur not in seen:
            seen.append(cur) # new head
            colors[cur] = newcolor
            newcolor += 1

        col = seen.index(cur)
        color = colors.pop(cur)
        next = seen[:]

        # Add parents to next
        addparents = [p for p in parents if p not in next]
        next[col:col + 1] = addparents

        # Set colors for the parents
        for i, p in enumerate(addparents):
            if not i:
                colors[p] = color
            else:
                colors[p] = newcolor
                newcolor += 1

        # Add edges to the graph
        edges = []
        for ecol, eid in enumerate(seen):
            if eid in next:
                bconf = getconf(eid)
                edges.append((
                    ecol, next.index(eid), colors[eid],
                    bconf.get('width', -1),
                    bconf.get('color', '')))
            elif eid == cur:
                for p in parents:
                    bconf = getconf(p)
                    edges.append((
                        ecol, next.index(p), color,
                        bconf.get('width', -1),
                        bconf.get('color', '')))

        # Yield and move on
        yield (cur, type, data, (col, color), edges)
        seen = next

def grandparent(cl, lowestrev, roots, head):
    """Return all ancestors of head in roots which revision is
    greater or equal to lowestrev.
    """
    pending = set([head])
    seen = set()
    kept = set()
    llowestrev = max(nullrev, lowestrev)
    while pending:
        r = pending.pop()
        if r >= llowestrev and r not in seen:
            if r in roots:
                kept.add(r)
            else:
                pending.update([p for p in cl.parentrevs(r)])
            seen.add(r)
    return sorted(kept)

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
    edges = [(nodeidx, nextseen.index(p)) for p in knownparents if p != nullrev]

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

def _fixlongrightedges(edges):
    for (i, (start, end)) in enumerate(edges):
        if end > start:
            edges[i] = (start, end + 1)

def _getnodelineedgestail(
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

def _drawedges(edges, nodeline, interline):
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

def _getpaddingline(ni, n_columns, edges):
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
      - Type indicator of node data, usually 'C' for changesets.
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
        _fixlongrightedges(edges)

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
        _getnodelineedgestail(idx, state[1], ncols, coldiff,
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
    _drawedges(edges, nodeline, shift_interline)

    # lines is the list of all graph lines to print
    lines = [nodeline]
    if add_padding_line:
        lines.append(_getpaddingline(idx, ncols, edges))
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
