# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2013 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .. import changelog2, revset, scmutil, util
from ..i18n import _
from ..node import hex
from .cmdtable import command


@command(
    "debugbenchmarkrevsets",
    [
        ("x", "rev-x", "", "picking the x set"),
        ("Y", "rev-y", "", "picking the y set"),
        ("e", "expr", [], "additional revset expressions to test"),
        ("d", "default", True, "include default revset expressions"),
        ("m", "multi-backend", False, "run the test with multiple backends"),
    ],
)
def benchmarkrevsets(ui, repo, *args, **opts):
    """benchmark revsets

    Runs simple benchmark on revsets.
    Benchmarks run 3 to 30 times. The fastest wall clock time is picked.
    """
    cl = repo.changelog

    # Backends to test
    origbackend = cl.algorithmbackend
    if opts.get("multi_backend"):
        backends = ["segments", "revlog", "revlog-cpy"]
    else:
        backends = [origbackend]

    # Prepare revsets
    xname = opts.get("rev_x") or "min(bookmark())"
    yname = opts.get("rev_y") or "max(bookmark())"
    xnode = scmutil.revsingle(repo, xname).node()
    ynode = scmutil.revsingle(repo, yname).node()
    alias = {"x": hex(xnode), "y": hex(ynode)}
    ui.write(_("# x:  %s  (%s)\n") % (hex(xnode), xname))
    ui.write(_("# y:  %s  (%s)\n\n") % (hex(ynode), yname))

    specs = opts.get("expr") or []
    if opts.get("default"):
        specs += [
            "ancestor(x, x)",
            "ancestor(x, y)",
            "ancestors(x)",
            "ancestors(y)",
            "children(x)",
            "children(y)",
            "descendants(x)",
            "descendants(y)",
            "y % x",
            "x::y",
            "heads(_all())",
            "roots(_all())",
        ]

    # Result table: [(name, *backendresult)]
    table = [["revset \\ backend"] + backends] + [
        [spec] + [""] * len(backends) for spec in specs
    ]

    try:
        dynamic = dynamiccontent()
        for backendindex, backend in enumerate(backends):
            migrate(repo, backend)
            # invalidate phase cache
            repo.invalidatechangelog()
            for specindex, spec in enumerate(specs):
                # parse the revset expression once
                m = revset.matchany(None, [spec], localalias=alias)
                # execute the revset multiple times
                seconds = bench(lambda: len(m(repo)))
                table[specindex + 1][backendindex + 1] = descseconds(seconds)
                rendered = rendertable(table)
                dynamic.render(rendered, ui.write)
    finally:
        migrate(repo, origbackend)
    return 0


class dynamiccontent(object):
    """Render dynamic content to ANSI terminal"""

    def __init__(self):
        self.content = ""

    def render(self, content, writefunc):
        # ANSI
        # ESC[#A : up # lines
        # ESC[K : clear to end of line
        lines = self.content.count("\n")
        ansi = "\033[K" + "\033[1A\033[K" * lines
        writefunc(ansi + content)
        self.content = content


def rendertable(table):
    """Render a table to a string"""
    rowcount = len(table)
    if not rowcount:
        return
    header = table[0]
    widths = [
        max(len(table[j][i]) for j in range(rowcount)) for i in range(len(header))
    ]
    result = ""
    for rowindex, row in enumerate(table):
        if rowindex == 1:
            for width in widths:
                result += "|"
                result += "-" * (width + 2)
            result += "|\n"
        for columnindex, cell in enumerate(row):
            width = widths[columnindex]
            if columnindex == 0:
                text = cell.ljust(width)
            else:
                text = cell.rjust(width)
            result += "| %s " % text
        result += "|\n"
    return result


def migrate(repo, backend):
    if backend == repo.changelog.algorithmbackend:
        return
    elif backend == "segments":
        changelog2.migratetodoublewrite(repo)
    elif backend == "revlog":
        changelog2.migratetorevlog(repo, rust=True)
    elif backend == "revlog-cpy":
        changelog2.migratetorevlog(repo, python=True)


def bench(func):
    """Return shortest wall clock time calling func, in seconds"""
    best = None
    elapsed = 0
    stable = 0
    while elapsed < 10 and stable < 10:
        start = util.timer()
        func()
        duration = util.timer() - start
        if best is None or duration < best:
            best = duration
            stable = 0
        else:
            stable += 1
        elapsed += duration
    return best


def descseconds(seconds):
    """Describe seconds. Returns a string."""
    if seconds < 0.01:
        return "%.1fms" % (seconds * 1000)
    elif seconds < 1:
        return "%.0fms" % (seconds * 1000)
    elif seconds < 10:
        return "%.1f s" % seconds
    else:
        return "%.0f s" % seconds
