# debuginhibit.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""debug commands and instrumentation for the inhibit extension

Adds the `debuginhibit` and `debugdeinhibit` commands to manually inhibit
and deinhibit nodes for testing purposes. Also causes inhibit to print
out the nodes being inhihibited or deinhibited and/or a stack trace of each
call site with the following config options::

  [debuginhibit]
    printnodes = true
    printstack = true
    stackdepth = 4

If stackdepth is not specified, a full stack trace will be printed.
"""

import inspect
import os

from functools import partial
from operator import itemgetter

from mercurial import (
    cmdutil,
    extensions,
    error,
    scmutil,
)
from mercurial.i18n import _
from mercurial.node import short

testedwith = 'ships-with-fb-hgext'

cmdtable = {}
command = cmdutil.command(cmdtable)

inhibit = None

def extsetup(ui):
    global inhibit
    try:
        inhibit = extensions.find('inhibit')
    except KeyError:
        ui.debug("no inhibit extension detected - "
                 "disabling debuginhibit\n")
        return

    if ui.configbool('debuginhibit', 'printnodes'):
        extensions.wrapfunction(
            inhibit,
            '_inhibitmarkers',
            partial(printnodeswrapper, ui, label="Inhibiting")
        )
        extensions.wrapfunction(
            inhibit,
            '_deinhibitmarkers',
            partial(printnodeswrapper, ui, label="Deinhibiting")
        )

def printnodeswrapper(ui, orig, repo, nodes, label=None):
    """Wrapper function that prints the nodes being inhibited/dehinhibited."""
    if label is None:
        label = "Nodes"

    # `nodes` may be a generator, so collect it into a list first.
    nodes = list(nodes)
    ui.status(_("%s: %s\n") % (label, [short(n) for n in nodes]))

    # Print out a truncated stack trace at the callsite if specified.
    if ui.configbool('debuginhibit', 'printstack'):
        trace = [_printframe(fi) for fi in inspect.stack()]

        # Truncate the stack trace is specified by the user's config.
        # Always remove the first two entries since they correspond
        # to this wrapper function.
        depth = ui.config('debuginhibit', 'stackdepth')
        trace = trace[2:] if depth is None else trace[2:2 + int(depth)]

        ui.status(_("Context:\n\t%s\n") % "\n\t".join(trace))

    return orig(repo, nodes)

@command('debuginhibit', [
    ('r', 'rev', [], _("revisions to inhibit or deinhibit")),
    ('d', 'deinhibit', False, _("deinhibit the specified revs"))
])
def debuginhibit(ui, repo, *revs, **opts):
    """manually inhibit or deinhibit the specified revisions

    By default inhibits any obsolescence markers on the given revs.
    With no arguments prints out a list of inhibited nodes.
    """
    _checkenabled(repo)

    revs = list(revs) + opts.get('rev', [])

    # If no arguments were passed to the command, just print out all the
    # inhibited nodes and exit.
    if not revs:
        _prettyprintnodes(ui, repo, repo._obsinhibit)
        return

    revs = scmutil.revrange(repo, revs)
    nodes = (repo.changelog.node(rev) for rev in revs)

    with repo.wlock():
        with repo.lock():
            with repo.transaction('debuginhibit') as tr:
                if opts.get('deinhibit', False):
                    inhibit._deinhibitmarkers(repo, nodes)
                else:
                    inhibit._inhibitmarkers(repo, nodes)

                # Disable inhibit's post-transaction callback so that we only
                # affect the changesets specified by the user.
                del tr._postclosecallback['inhibitposttransaction']

def _prettyprintnodes(ui, repo, nodes):
    """Pretty print a list of nodes."""
    contexts = [repo[node] for node in nodes]
    showopts = {
        'template': '{rev}:{node} {if(bookmarks, "({bookmarks}) ")}'
                    '{desc|firstline}\n'
    }
    displayer = cmdutil.show_changeset(ui, repo, showopts)
    for ctx in contexts:
        displayer.show(ctx)

def _printframe(frameinfo):
    """Return a human-readable string representation of a FrameInfo object."""
    path, line, fn = itemgetter(1, 2, 3)(frameinfo)
    return "[%s:%d] %s()" % (os.path.basename(path), line, fn)

def _checkenabled(repo):
    """Abort if inhibit is unavailable or disabled."""
    if inhibit is None:
        raise error.Abort(_("no inhibit extension detected"))
    if not inhibit._inhibitenabled(repo):
        raise error.Abort(_("inhibit extension is present but disabled"))
