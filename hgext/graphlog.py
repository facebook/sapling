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
from mercurial import cmdutil, commands, extensions
from mercurial import hg, util, graphmod

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def _checkunsupportedflags(pats, opts):
    for op in ["newest_first"]:
        if op in opts and opts[op]:
            raise util.Abort(_("-G/--graph option is incompatible with --%s")
                             % op.replace("_", "-"))

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
    return cmdutil.graphlog(ui, repo, *pats, **opts)

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

    _checkunsupportedflags([], opts)
    o = hg._outgoing(ui, repo, dest, opts)
    if o is None:
        return

    revdag = graphrevs(repo, o, opts)
    displayer = show_changeset(ui, repo, opts, buffered=True)
    showparents = [ctx.node() for ctx in repo[None].parents()]
    cmdutil.displaygraph(ui, revdag, displayer, showparents,
                         graphmod.asciiedges)

def gincoming(ui, repo, source="default", **opts):
    """show the incoming changesets alongside an ASCII revision graph

    Print the incoming changesets alongside a revision graph drawn with
    ASCII characters.

    Nodes printed as an @ character are parents of the working
    directory.
    """
    def subreporecurse():
        return 1

    _checkunsupportedflags([], opts)
    def display(other, chlist, displayer):
        revdag = graphrevs(other, chlist, opts)
        showparents = [ctx.node() for ctx in repo[None].parents()]
        cmdutil.displaygraph(ui, revdag, displayer, showparents,
                             graphmod.asciiedges)

    hg._incoming(display, subreporecurse, ui, repo, source, opts, buffered=True)

def uisetup(ui):
    '''Initialize the extension.'''
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
