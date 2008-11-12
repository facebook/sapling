# ASCII graph log extension for Mercurial
#
# Copyright 2007 Joel Rosdahl <joel@rosdahl.net>
#
# This software may be used and distributed according to the terms of
# the GNU General Public License, incorporated herein by reference.
'''show revision graphs in terminal windows'''

import os
import sys
from mercurial.cmdutil import revrange, show_changeset
from mercurial.commands import templateopts
from mercurial.i18n import _
from mercurial.node import nullrev
from mercurial.util import Abort, canonpath
from mercurial import util

def get_rev_parents(repo, rev):
    return [x for x in repo.changelog.parentrevs(rev) if x != nullrev]

def revision_grapher(repo, start_rev, stop_rev):
    """incremental revision grapher

    This generator function walks through the revision history from
    revision start_rev to revision stop_rev (which must be less than
    or equal to start_rev) and for each revision emits tuples with the
    following elements:

      - Current revision.
      - Current node.
      - Column of the current node in the set of ongoing edges.
      - Edges; a list of (col, next_col) indicating the edges between
        the current node and its parents.
      - Number of columns (ongoing edges) in the current revision.
      - The difference between the number of columns (ongoing edges)
        in the next revision and the number of columns (ongoing edges)
        in the current revision. That is: -1 means one column removed;
        0 means no columns added or removed; 1 means one column added.
    """

    assert start_rev >= stop_rev
    curr_rev = start_rev
    revs = []
    while curr_rev >= stop_rev:
        node = repo.changelog.node(curr_rev)

        # Compute revs and next_revs.
        if curr_rev not in revs:
            # New head.
            revs.append(curr_rev)
        rev_index = revs.index(curr_rev)
        next_revs = revs[:]

        # Add parents to next_revs.
        parents = get_rev_parents(repo, curr_rev)
        parents_to_add = []
        for parent in parents:
            if parent not in next_revs:
                parents_to_add.append(parent)
        next_revs[rev_index:rev_index + 1] = util.sort(parents_to_add)

        edges = []
        for parent in parents:
            edges.append((rev_index, next_revs.index(parent)))

        n_columns_diff = len(next_revs) - len(revs)
        yield (curr_rev, node, rev_index, edges, len(revs), n_columns_diff)

        revs = next_revs
        curr_rev -= 1

def filelog_grapher(repo, path, start_rev, stop_rev):
    """incremental file log grapher

    This generator function walks through the revision history of a
    single file from revision start_rev to revision stop_rev (which must
    be less than or equal to start_rev) and for each revision emits
    tuples with the following elements:

      - Current revision.
      - Current node.
      - Column of the current node in the set of ongoing edges.
      - Edges; a list of (col, next_col) indicating the edges between
        the current node and its parents.
      - Number of columns (ongoing edges) in the current revision.
      - The difference between the number of columns (ongoing edges)
        in the next revision and the number of columns (ongoing edges)
        in the current revision. That is: -1 means one column removed;
        0 means no columns added or removed; 1 means one column added.
    """

    assert start_rev >= stop_rev
    revs = []
    filerev = len(repo.file(path)) - 1
    while filerev >= 0:
        fctx = repo.filectx(path, fileid=filerev)

        # Compute revs and next_revs.
        if filerev not in revs:
            revs.append(filerev)
        rev_index = revs.index(filerev)
        next_revs = revs[:]

        # Add parents to next_revs.
        parents = [f.filerev() for f in fctx.parents() if f.path() == path]
        parents_to_add = []
        for parent in parents:
            if parent not in next_revs:
                parents_to_add.append(parent)
        next_revs[rev_index:rev_index + 1] = util.sort(parents_to_add)

        edges = []
        for parent in parents:
            edges.append((rev_index, next_revs.index(parent)))

        changerev = fctx.linkrev()
        if changerev <= start_rev:
            node = repo.changelog.node(changerev)
            n_columns_diff = len(next_revs) - len(revs)
            yield (changerev, node, rev_index, edges, len(revs), n_columns_diff)
        if changerev <= stop_rev:
            break
        revs = next_revs
        filerev -= 1

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
            nodeline[2 * end] = "+"
            if start > end:
                (start, end) = (end,start)
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

def ascii(ui, grapher):
    """prints an ASCII graph of the DAG returned by the grapher

    grapher is a generator that emits tuples with the following elements:

      - Character to use as node's symbol.
      - List of lines to display as the node's text.
      - Column of the current node in the set of ongoing edges.
      - Edges; a list of (col, next_col) indicating the edges between
        the current node and its parents.
      - Number of columns (ongoing edges) in the current revision.
      - The difference between the number of columns (ongoing edges)
        in the next revision and the number of columns (ongoing edges)
        in the current revision. That is: -1 means one column removed;
        0 means no columns added or removed; 1 means one column added.
    """
    prev_n_columns_diff = 0
    prev_node_index = 0
    for (node_ch, node_lines, node_index, edges, n_columns, n_columns_diff) in grapher:

        if n_columns_diff == -1:
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
        add_padding_line = (len(node_lines) > 2 and
                            n_columns_diff == -1 and
                            [x for (x, y) in edges if x + 1 < y])

        # fix_nodeline_tail says whether to rewrite
        #
        #     | | o | |        | | o | |
        #     | | |/ /         | | |/ /
        #     | o | |    into  | o / /   # <--- fixed nodeline tail
        #     | |/ /           | |/ /
        #     o | |            o | |
        fix_nodeline_tail = len(node_lines) <= 2 and not add_padding_line

        # nodeline is the line containing the node character (typically o)
        nodeline = ["|", " "] * node_index
        nodeline.extend([node_ch, " "])

        nodeline.extend(
            get_nodeline_edges_tail(
                node_index, prev_node_index, n_columns, n_columns_diff,
                prev_n_columns_diff, fix_nodeline_tail))

        # shift_interline is the line containing the non-vertical
        # edges between this entry and the next
        shift_interline = ["|", " "] * node_index
        if n_columns_diff == -1:
            n_spaces = 1
            edge_ch = "/"
        elif n_columns_diff == 0:
            n_spaces = 2
            edge_ch = "|"
        else:
            n_spaces = 3
            edge_ch = "\\"
        shift_interline.extend(n_spaces * [" "])
        shift_interline.extend([edge_ch, " "] * (n_columns - node_index - 1))

        # draw edges from the current node to its parents
        draw_edges(edges, nodeline, shift_interline)

        # lines is the list of all graph lines to print
        lines = [nodeline]
        if add_padding_line:
            lines.append(get_padding_line(node_index, n_columns, edges))
        lines.append(shift_interline)

        # make sure that there are as many graph lines as there are
        # log strings
        while len(node_lines) < len(lines):
            node_lines.append("")
        if len(lines) < len(node_lines):
            extra_interline = ["|", " "] * (n_columns + n_columns_diff)
            while len(lines) < len(node_lines):
                lines.append(extra_interline)

        # print lines
        indentation_level = max(n_columns, n_columns + n_columns_diff)
        for (line, logstr) in zip(lines, node_lines):
            ln = "%-*s %s" % (2 * indentation_level, "".join(line), logstr)
            ui.write(ln.rstrip() + '\n')

        # ... and start over
        prev_node_index = node_index
        prev_n_columns_diff = n_columns_diff

def get_limit(limit_opt):
    if limit_opt:
        try:
            limit = int(limit_opt)
        except ValueError:
            raise Abort(_("limit must be a positive integer"))
        if limit <= 0:
            raise Abort(_("limit must be positive"))
    else:
        limit = sys.maxint
    return limit

def get_revs(repo, rev_opt):
    if rev_opt:
        revs = revrange(repo, rev_opt)
        return (max(revs), min(revs))
    else:
        return (len(repo) - 1, 0)

def graphlog(ui, repo, path=None, **opts):
    """show revision history alongside an ASCII revision graph

    Print a revision history alongside a revision graph drawn with
    ASCII characters.

    Nodes printed as an @ character are parents of the working
    directory.
    """

    limit = get_limit(opts["limit"])
    (start_rev, stop_rev) = get_revs(repo, opts["rev"])
    stop_rev = max(stop_rev, start_rev - limit + 1)
    if start_rev == nullrev:
        return
    if path:
        path = canonpath(repo.root, os.getcwd(), path)
    if path:
        revgrapher = filelog_grapher(repo, path, start_rev, stop_rev)
    else:
        revgrapher = revision_grapher(repo, start_rev, stop_rev)

    repo_parents = repo.dirstate.parents()
    cs_printer = show_changeset(ui, repo, opts)
    def grapher():
        for (rev, node, node_index, edges, n_columns, n_columns_diff) in revgrapher:
            # log_strings is the list of all log strings to draw alongside
            # the graph.
            ui.pushbuffer()
            cs_printer.show(rev, node)
            log_strings = ui.popbuffer().split("\n")[:-1]
            if node in repo_parents:
                node_ch = "@"
            else:
                node_ch = "o"
            yield (node_ch, log_strings, node_index, edges, n_columns, n_columns_diff)

    ascii(ui, grapher())

cmdtable = {
    "glog":
        (graphlog,
         [('l', 'limit', '', _('limit number of changes displayed')),
          ('p', 'patch', False, _('show patch')),
          ('r', 'rev', [], _('show the specified revision or range')),
         ] + templateopts,
         _('hg glog [OPTION]... [FILE]')),
}
