# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2010 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# debugshell extension
"""a python shell with repo, changelog & manifest objects"""

from __future__ import absolute_import

import os
import shlex
import sys

import bindings
import edenscm
import edenscmnative
from edenscm import hgext, mercurial
from edenscm.hgext import commitcloud as cc
from edenscm.mercurial import pycompat, registrar, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.pycompat import decodeutf8


cmdtable = {}
command = registrar.command(cmdtable)


def _assignobjects(objects, repo):
    objects.update(
        {
            # Shortcuts
            "b": bindings,
            "m": mercurial,
            "x": hgext,
            "td": bindings.tracing.tracingdata,
            # Modules
            "bindings": bindings,
            "edenscm": edenscm,
            "edenscmnative": edenscmnative,
            "mercurial": mercurial,
            # Utilities
            "util": mercurial.util,
            "hex": mercurial.node.hex,
            "bin": mercurial.node.bin,
        }
    )
    if repo:
        objects.update(
            {
                "repo": repo,
                "cl": repo.changelog,
                "mf": repo.manifestlog,
                # metalog is not available on hg server-side repos. Consider making it
                # available unconditionally once we get rid of hg servers.
                "ml": getattr(repo.svfs, "metalog", None),
                "ms": getattr(repo, "_mutationstore", None),
            }
        )

        # Commit cloud service.
        ui = repo.ui
        try:
            token = cc.token.TokenLocator(ui).token
            if token is not None:
                objects["serv"] = cc.service.get(ui, token)
        except Exception:
            pass

    # Import other handy modules
    for name in ["os", "sys", "subprocess", "re"]:
        objects[name] = __import__(name)


@command(
    "debugshell|dbsh|debugsh",
    [("c", "command", "", _("program passed in as string"), _("CMD"))],
    optionalrepo=True,
)
def debugshell(ui, repo, *args, **opts):
    command = opts.get("command")

    _assignobjects(locals(), repo)
    globals().update(locals())
    sys.argv = pycompat.sysargv = args

    if command:
        exec(command)
        return 0
    if args:
        path = args[0]
        with open(path) as f:
            command = f.read()
        globalvars = dict(globals())
        localvars = dict(locals())
        globalvars["__file__"] = path
        exec(command, globalvars, localvars)
        return 0
    elif not ui.interactive():
        command = ui.fin.read()
        exec(command)
        return 0

    _startipython(ui, repo)


def _startipython(ui, repo):
    from IPython.terminal.ipapp import load_default_config
    from IPython.terminal.embed import InteractiveShellEmbed

    bannermsg = "loaded repo:  %s\n" "using source: %s" % (
        repo and repo.root or "(none)",
        mercurial.__path__[0],
    ) + (
        "\n\nAvailable variables:\n"
        " m:  edenscm.mercurial\n"
        " x:  edenscm.hgext\n"
        " b:  bindings\n"
        " ui: the ui object\n"
        " c:  run command and take output\n"
    )
    if repo:
        bannermsg += (
            " repo: the repo object\n"
            " serv: commitcloud service\n"
            " cl: repo.changelog\n"
            " mf: repo.manifestlog\n"
            " ml: repo.svfs.metalog\n"
            " ms: repo._mutationstore\n"
        )
    bannermsg += """
Available IPython magics (auto magic is on, `%` is optional):
 time:   measure time
 timeit: benchmark
 trace:  run and print ASCII trace (better with --tracing command flag)
 hg:     run commands inline
"""

    config = load_default_config()
    config.InteractiveShellEmbed = config.TerminalInteractiveShell
    config.InteractiveShell.automagic = True
    config.InteractiveShell.banner2 = bannermsg
    config.InteractiveShell.confirm_exit = False

    shell = InteractiveShellEmbed.instance(config=config)
    _configipython(ui, shell)

    shell()


def c(args):
    """Run command with args and take its output.

    Example::

        c(['log', '-r.'])
        c('log -r.')
        %trace c('log -r.')
    """
    if isinstance(args, str):
        args = shlex.split(args)
    ui = globals()["ui"]
    fin = util.stringio()
    fout = util.stringio()
    bindings.commands.run(["hg"] + args, fin, fout, ui.ferr)
    return fout.getvalue()


def _configipython(ui, ipython):
    """Set up IPython features like magics"""
    from IPython.core.magic import register_line_magic

    @register_line_magic
    def hg(line):
        args = ["hg"] + shlex.split(line)
        return bindings.commands.run(args, ui.fin, ui.fout, ui.ferr)

    @register_line_magic
    def trace(line, ui=ui, shell=ipython):
        """run and print ASCII trace"""
        code = compile(line, "<magic-trace>", "exec")

        td = bindings.tracing.tracingdata()
        ns = shell.user_ns
        ns.update(globals())
        start = util.timer()
        _execwith(td, code, ns)
        durationmicros = (util.timer() - start) * 1e6
        # hide spans less than 50 microseconds, or 1% of the total time
        ui.write_err("%s" % td.ascii(int(durationmicros / 100) + 50))
        return td


def _execwith(td, code, ns):
    with td:
        exec(code, ns)
