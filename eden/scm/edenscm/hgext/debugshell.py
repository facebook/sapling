# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
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

import shlex
import sys
import time

import bindings
import edenscm
import edenscmnative
from edenscm import hgdemandimport, hgext, mercurial, traceimport
from edenscm.hgext import commitcloud as cc
from edenscm.mercurial import pycompat, registrar, util
from edenscm.mercurial.i18n import _


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
                "ml": repo.metalog(),
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

        # EdenAPI client
        try:
            objects["api"] = repo.edenapi
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

    env = globals()
    env["ui"] = ui
    _assignobjects(env, repo)
    sys.argv = pycompat.sysargv = args

    if command:
        exec(command, env, env)
        return 0
    if args:
        path = args[0]
        with open(path) as f:
            command = f.read()
        env["__file__"] = path
        exec(command, env, env)
        return 0
    elif not ui.interactive():
        command = ui.fin.read()
        exec(command, env, env)
        return 0

    # IPython is incompatible with demandimport.
    with hgdemandimport.deactivated():
        _startipython(ui, repo, env)


def _startipython(ui, repo, env):
    # IPython requires time.clock. It is missing on Windows. Polyfill it.
    if getattr(time, "clock", None) is None:
        time.clock = time.time

    from IPython.terminal.embed import InteractiveShellEmbed
    from IPython.terminal.ipapp import load_default_config

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
            " api: edenapi client\n"
            " cl: repo.changelog\n"
            " mf: repo.manifestlog\n"
            " ml: repo.svfs.metalog\n"
            " ms: repo._mutationstore\n"
        )
    bannermsg += """
Available IPython magics (auto magic is on, `%` is optional):
 time:   measure time
 timeit: benchmark
 trace:  run and print ASCII trace (better with --trace command flag)
 hg:     run commands inline
"""

    config = load_default_config()
    config.InteractiveShellEmbed = config.TerminalInteractiveShell
    config.InteractiveShell.automagic = True
    config.InteractiveShell.banner2 = bannermsg
    config.InteractiveShell.confirm_exit = False

    shell = InteractiveShellEmbed.instance(config=config)
    _configipython(ui, shell)

    locals().update(env)
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

    # get_ipython is used by register_line_magic
    get_ipython = ipython.get_ipython  # noqa: F841

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
        asciitrace = td.ascii(int(durationmicros / 100) + 50)
        ui.write_err("%s" % asciitrace)
        if not traceimport.enabled:
            ui.write_err("(use 'debugshell --trace' to enable more detailed trace)\n")
        return td


def _execwith(td, code, ns):
    with td:
        exec(code, ns)
