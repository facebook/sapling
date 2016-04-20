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

from __future__ import absolute_import

import heapq

from .node import nullrev
from . import (
    revset,
    util,
)

CHANGESET = 'C'
PARENT = 'P'
GRANDPARENT = 'G'
MISSINGPARENT = 'M'
# Style of line to draw. None signals a line that ends and is removed at this
# point.
EDGES = {PARENT: '|', GRANDPARENT: ':', MISSINGPARENT: None}

def groupbranchiter(revs, parentsfunc, firstbranch=()):
    """Yield revisions from heads to roots one (topo) branch at a time.

    This function aims to be used by a graph generator that wishes to minimize
    the number of parallel branches and their interleaving.

    Example iteration order (numbers show the "true" order in a changelog):

      o  4
      |
      o  1
      |
      | o  3
      | |
      | o  2
      |/
      o  0

    Note that the ancestors of merges are understood by the current
    algorithm to be on the same branch. This means no reordering will
    occur behind a merge.
    """

    ### Quick summary of the algorithm
    #
    # This function is based around a "retention" principle. We keep revisions
    # in memory until we are ready to emit a whole branch that immediately
    # "merges" into an existing one. This reduces the number of parallel
    # branches with interleaved revisions.
    #
    # During iteration revs are split into two groups:
    # A) revision already emitted
    # B) revision in "retention". They are stored as different subgroups.
    #
    # for each REV, we do the following logic:
    #
    #   1) if REV is a parent of (A), we will emit it. If there is a
    #   retention group ((B) above) that is blocked on REV being
    #   available, we emit all the revisions out of that retention
    #   group first.
    #
    #   2) else, we'll search for a subgroup in (B) awaiting for REV to be
    #   available, if such subgroup exist, we add REV to it and the subgroup is
    #   now awaiting for REV.parents() to be available.
    #
    #   3) finally if no such group existed in (B), we create a new subgroup.
    #
    #
    # To bootstrap the algorithm, we emit the tipmost revision (which
    # puts it in group (A) from above).

    revs.sort(reverse=True)

    # Set of parents of revision that have been emitted. They can be considered
    # unblocked as the graph generator is already aware of them so there is no
    # need to delay the revisions that reference them.
    #
    # If someone wants to prioritize a branch over the others, pre-filling this
    # set will force all other branches to wait until this branch is ready to be
    # emitted.
    unblocked = set(firstbranch)

    # list of groups waiting to be displayed, each group is defined by:
    #
    #   (revs:    lists of revs waiting to be displayed,
    #    blocked: set of that cannot be displayed before those in 'revs')
    #
    # The second value ('blocked') correspond to parents of any revision in the
    # group ('revs') that is not itself contained in the group. The main idea
    # of this algorithm is to delay as much as possible the emission of any
    # revision.  This means waiting for the moment we are about to display
    # these parents to display the revs in a group.
    #
    # This first implementation is smart until it encounters a merge: it will
    # emit revs as soon as any parent is about to be emitted and can grow an
    # arbitrary number of revs in 'blocked'. In practice this mean we properly
    # retains new branches but gives up on any special ordering for ancestors
    # of merges. The implementation can be improved to handle this better.
    #
    # The first subgroup is special. It corresponds to all the revision that
    # were already emitted. The 'revs' lists is expected to be empty and the
    # 'blocked' set contains the parents revisions of already emitted revision.
    #
    # You could pre-seed the <parents> set of groups[0] to a specific
    # changesets to select what the first emitted branch should be.
    groups = [([], unblocked)]
    pendingheap = []
    pendingset = set()

    heapq.heapify(pendingheap)
    heappop = heapq.heappop
    heappush = heapq.heappush
    for currentrev in revs:
        # Heap works with smallest element, we want highest so we invert
        if currentrev not in pendingset:
            heappush(pendingheap, -currentrev)
            pendingset.add(currentrev)
        # iterates on pending rev until after the current rev have been
        # processed.
        rev = None
        while rev != currentrev:
            rev = -heappop(pendingheap)
            pendingset.remove(rev)

            # Seek for a subgroup blocked, waiting for the current revision.
            matching = [i for i, g in enumerate(groups) if rev in g[1]]

            if matching:
                # The main idea is to gather together all sets that are blocked
                # on the same revision.
                #
                # Groups are merged when a common blocking ancestor is
                # observed. For example, given two groups:
                #
                # revs [5, 4] waiting for 1
                # revs [3, 2] waiting for 1
                #
                # These two groups will be merged when we process
                # 1. In theory, we could have merged the groups when
                # we added 2 to the group it is now in (we could have
                # noticed the groups were both blocked on 1 then), but
                # the way it works now makes the algorithm simpler.
                #
                # We also always keep the oldest subgroup first. We can
                # probably improve the behavior by having the longest set
                # first. That way, graph algorithms could minimise the length
                # of parallel lines their drawing. This is currently not done.
                targetidx = matching.pop(0)
                trevs, tparents = groups[targetidx]
                for i in matching:
                    gr = groups[i]
                    trevs.extend(gr[0])
                    tparents |= gr[1]
                # delete all merged subgroups (except the one we kept)
                # (starting from the last subgroup for performance and
                # sanity reasons)
                for i in reversed(matching):
                    del groups[i]
            else:
                # This is a new head. We create a new subgroup for it.
                targetidx = len(groups)
                groups.append(([], set([rev])))

            gr = groups[targetidx]

            # We now add the current nodes to this subgroups. This is done
            # after the subgroup merging because all elements from a subgroup
            # that relied on this rev must precede it.
            #
            # we also update the <parents> set to include the parents of the
            # new nodes.
            if rev == currentrev: # only display stuff in rev
                gr[0].append(rev)
            gr[1].remove(rev)
            parents = [p for p in parentsfunc(rev) if p > nullrev]
            gr[1].update(parents)
            for p in parents:
                if p not in pendingset:
                    pendingset.add(p)
                    heappush(pendingheap, -p)

            # Look for a subgroup to display
            #
            # When unblocked is empty (if clause), we were not waiting for any
            # revisions during the first iteration (if no priority was given) or
            # if we emitted a whole disconnected set of the graph (reached a
            # root).  In that case we arbitrarily take the oldest known
            # subgroup. The heuristic could probably be better.
            #
            # Otherwise (elif clause) if the subgroup is blocked on
            # a revision we just emitted, we can safely emit it as
            # well.
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
    # Check if we have some subgroup waiting for revisions we are not going to
    # iterate over
    for g in groups:
        for r in g[0]:
            yield r

def dagwalker(repo, revs):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentinfo]) tuples

    This generator function walks through revisions (which should be ordered
    from bigger to lower). It returns a tuple for each node.

    Each parentinfo entry is a tuple with (edgetype, parentid), where edgetype
    is one of PARENT, GRANDPARENT or MISSINGPARENT. The node and parent ids
    are arbitrary integers which identify a node in the context of the graph
    returned.

    """
    if not revs:
        return

    gpcache = {}

    if repo.ui.configbool('experimental', 'graph-group-branches', False):
        firstbranch = ()
        firstbranchrevset = repo.ui.config(
            'experimental', 'graph-group-branches.firstbranch', '')
        if firstbranchrevset:
            firstbranch = repo.revs(firstbranchrevset)
        parentrevs = repo.changelog.parentrevs
        revs = groupbranchiter(revs, parentrevs, firstbranch)
        revs = revset.baseset(revs)

    for rev in revs:
        ctx = repo[rev]
        # partition into parents in the rev set and missing parents, then
        # augment the lists with markers, to inform graph drawing code about
        # what kind of edge to draw between nodes.
        pset = set(p.rev() for p in ctx.parents() if p.rev() in revs)
        mpars = [p.rev() for p in ctx.parents()
                 if p.rev() != nullrev and p.rev() not in pset]
        parents = [(PARENT, p) for p in sorted(pset)]

        for mpar in mpars:
            gp = gpcache.get(mpar)
            if gp is None:
                # precompute slow query as we know reachableroots() goes
                # through all revs (issue4782)
                if not isinstance(revs, revset.baseset):
                    revs = revset.baseset(revs)
                gp = gpcache[mpar] = sorted(set(revset.reachableroots(
                    repo, revs, [mpar])))
            if not gp:
                parents.append((MISSINGPARENT, mpar))
                pset.add(mpar)
            else:
                parents.extend((GRANDPARENT, g) for g in gp if g not in pset)
                pset.update(gp)

        yield (ctx.rev(), CHANGESET, ctx, parents)

def nodes(repo, nodes):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

    This generator function walks the given nodes. It only returns parents
    that are in nodes, too.
    """
    include = set(nodes)
    for node in nodes:
        ctx = repo[node]
        parents = set((PARENT, p.rev()) for p in ctx.parents()
                      if p.node() in include)
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
        addparents = [p for pt, p in parents if p not in next]
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
                for ptype, p in parents:
                    bconf = getconf(p)
                    edges.append((
                        ecol, next.index(p), color,
                        bconf.get('width', -1),
                        bconf.get('color', '')))

        # Yield and move on
        yield (cur, type, data, (col, color), edges)
        seen = next

def asciiedges(type, char, lines, state, rev, parents):
    """adds edge info to changelog DAG walk suitable for ascii()"""
    seen = state['seen']
    if rev not in seen:
        seen.append(rev)
    nodeidx = seen.index(rev)

    knownparents = []
    newparents = []
    for ptype, parent in parents:
        if parent in seen:
            knownparents.append(parent)
        else:
            newparents.append(parent)
            state['edges'][parent] = state['styles'].get(ptype, '|')

    ncols = len(seen)
    nextseen = seen[:]
    nextseen[nodeidx:nodeidx + 1] = newparents
    edges = [(nodeidx, nextseen.index(p))
             for p in knownparents if p != nullrev]

    seen[:] = nextseen
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
    # remove current node from edge characters, no longer needed
    state['edges'].pop(rev, None)
    yield (type, char, lines, (nodeidx, edges, ncols, nmorecols))

def _fixlongrightedges(edges):
    for (i, (start, end)) in enumerate(edges):
        if end > start:
            edges[i] = (start, end + 1)

def _getnodelineedgestail(
        echars, idx, pidx, ncols, coldiff, pdiff, fix_tail):
    if fix_tail and coldiff == pdiff and coldiff != 0:
        # Still going in the same non-vertical direction.
        if coldiff == -1:
            start = max(idx + 1, pidx)
            tail = echars[idx * 2:(start - 1) * 2]
            tail.extend(["/", " "] * (ncols - start))
            return tail
        else:
            return ["\\", " "] * (ncols - idx - 1)
    else:
        remainder = (ncols - idx - 1)
        return echars[-(remainder * 2):] if remainder > 0 else []

def _drawedges(echars, edges, nodeline, interline):
    for (start, end) in edges:
        if start == end + 1:
            interline[2 * end + 1] = "/"
        elif start == end - 1:
            interline[2 * start + 1] = "\\"
        elif start == end:
            interline[2 * start] = echars[2 * start]
        else:
            if 2 * end >= len(nodeline):
                continue
            nodeline[2 * end] = "+"
            if start > end:
                (start, end) = (end, start)
            for i in range(2 * start + 1, 2 * end):
                if nodeline[i] != "+":
                    nodeline[i] = "-"

def _getpaddingline(echars, idx, ncols, edges):
    # all edges up to the current node
    line = echars[:idx * 2]
    # an edge for the current node, if there is one
    if (idx, idx - 1) in edges or (idx, idx) in edges:
        # (idx, idx - 1)      (idx, idx)
        # | | | |           | | | |
        # +---o |           | o---+
        # | | X |           | X | |
        # | |/ /            | |/ /
        # | | |             | | |
        line.extend(echars[idx * 2:(idx + 1) * 2])
    else:
        line.extend('  ')
    # all edges to the right of the current node
    remainder = ncols - idx - 1
    if remainder > 0:
        line.extend(echars[-(remainder * 2):])
    return line

def _drawendinglines(lines, extra, edgemap, seen):
    """Draw ending lines for missing parent edges

    None indicates an edge that ends at between this node and the next
    Replace with a short line ending in ~ and add / lines to any edges to
    the right.

    """
    if None not in edgemap.values():
        return

    # Check for more edges to the right of our ending edges.
    # We need enough space to draw adjustment lines for these.
    edgechars = extra[::2]
    while edgechars and edgechars[-1] is None:
        edgechars.pop()
    shift_size = max((edgechars.count(None) * 2) - 1, 0)
    while len(lines) < 3 + shift_size:
        lines.append(extra[:])

    if shift_size:
        empties = []
        toshift = []
        first_empty = extra.index(None)
        for i, c in enumerate(extra[first_empty::2], first_empty // 2):
            if c is None:
                empties.append(i * 2)
            else:
                toshift.append(i * 2)
        targets = list(range(first_empty, first_empty + len(toshift) * 2, 2))
        positions = toshift[:]
        for line in lines[-shift_size:]:
            line[first_empty:] = [' '] * (len(line) - first_empty)
            for i in range(len(positions)):
                pos = positions[i] - 1
                positions[i] = max(pos, targets[i])
                line[pos] = '/' if pos > targets[i] else extra[toshift[i]]

    map = {1: '|', 2: '~'}
    for i, line in enumerate(lines):
        if None not in line:
            continue
        line[:] = [c or map.get(i, ' ') for c in line]

    # remove edges that ended
    remove = [p for p, c in edgemap.items() if c is None]
    for parent in remove:
        del edgemap[parent]
        seen.remove(parent)

def asciistate():
    """returns the initial value for the "state" argument to ascii()"""
    return {
        'seen': [],
        'edges': {},
        'lastcoldiff': 0,
        'lastindex': 0,
        'styles': EDGES.copy(),
        'graphshorten': False,
    }

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

    edgemap, seen = state['edges'], state['seen']
    # Be tolerant of history issues; make sure we have at least ncols + coldiff
    # elements to work with. See test-glog.t for broken history test cases.
    echars = [c for p in seen for c in (edgemap.get(p, '|'), ' ')]
    echars.extend(('|', ' ') * max(ncols + coldiff - len(seen), 0))

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
    nodeline = echars[:idx * 2]
    nodeline.extend([char, " "])

    nodeline.extend(
        _getnodelineedgestail(
            echars, idx, state['lastindex'], ncols, coldiff,
            state['lastcoldiff'], fix_nodeline_tail))

    # shift_interline is the line containing the non-vertical
    # edges between this entry and the next
    shift_interline = echars[:idx * 2]
    shift_interline.extend(' ' * (2 + coldiff))
    count = ncols - idx - 1
    if coldiff == -1:
        shift_interline.extend('/ ' * count)
    elif coldiff == 0:
        shift_interline.extend(echars[(idx + 1) * 2:ncols * 2])
    else:
        shift_interline.extend(r'\ ' * count)

    # draw edges from the current node to its parents
    _drawedges(echars, edges, nodeline, shift_interline)

    # lines is the list of all graph lines to print
    lines = [nodeline]
    if add_padding_line:
        lines.append(_getpaddingline(echars, idx, ncols, edges))

    # If 'graphshorten' config, only draw shift_interline
    # when there is any non vertical flow in graph.
    if state['graphshorten']:
        if any(c in '\/' for c in shift_interline if c):
            lines.append(shift_interline)
    # Else, no 'graphshorten' config so draw shift_interline.
    else:
        lines.append(shift_interline)

    # make sure that there are as many graph lines as there are
    # log strings
    extra_interline = echars[:(ncols + coldiff) * 2]
    if len(lines) < len(text):
        while len(lines) < len(text):
            lines.append(extra_interline[:])

    _drawendinglines(lines, extra_interline, edgemap, seen)

    while len(text) < len(lines):
        text.append("")

    # print lines
    indentation_level = max(ncols, ncols + coldiff)
    for (line, logstr) in zip(lines, text):
        ln = "%-*s %s" % (2 * indentation_level, "".join(line), logstr)
        ui.write(ln.rstrip() + '\n')

    # ... and start over
    state['lastcoldiff'] = coldiff
    state['lastindex'] = idx
