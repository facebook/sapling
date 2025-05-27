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

import io
import shlex
import sys
import time
from typing import Any, Dict, List, Optional

import bindings

import sapling
from sapling import ext, hgdemandimport, registrar, traceimport, util
from sapling.ext import commitcloud as cc
from sapling.i18n import _

cmdtable = {}
command = registrar.command(cmdtable)


def _assignobjects(objects, repo) -> None:
    objects.update(
        {
            # Shortcuts
            "b": bindings,
            "s": sapling,
            "x": ext,
            "td": bindings.tracing.tracingdata,
            # Modules
            "bindings": bindings,
            "sapling": sapling,
            # Utilities
            "util": sapling.util,
            "hex": sapling.node.hex,
            "bin": sapling.node.bin,
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
            # pyre-fixme[16]: Module `commitcloud` has no attribute `token`.
            token = cc.token.TokenLocator(ui).token
            if token is not None:
                # pyre-fixme[19]: Expected 1 positional argument.
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
    sys.argv = args

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

    start_ipython(env, ui, repo)


def start_ipython(env=None, ui=None, repo=None) -> None:
    """Launch a Python shell (IPython or code.interactive())
    env defines the variables available in the shell.
    ui and repo enable more features.
    """
    # Local environment variables
    env = env or sys._getframe(1).f_locals
    # IPython is incompatible with demandimport.
    with hgdemandimport.deactivated():
        _start_ipython(env, ui, repo)


def _start_ipython(env, ui, repo) -> None:
    # IPython requires time.clock. It is missing on Windows. Polyfill it.
    # pyre-fixme[16]: Module `time` has no attribute `clock`.
    if getattr(time, "clock", None) is None:
        time.clock = time.time

    bannermsg = ""
    if ui:
        bannermsg += "loaded repo:  %s\nusing source: %s" % (
            repo and repo.root or "(none)",
            sapling.__path__ and sapling.__path__[0],
        ) + (
            "\n\nAvailable variables:\n"
            " s:  sapling\n"
            " x:  sapling.ext\n"
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

    util.get_main_io().disable_progress()

    try:
        # Enable site-packages before loading IPython
        import site

        site.main()
    except Exception:
        pass

    have_ipython = False
    try:
        from IPython.terminal.embed import InteractiveShellEmbed
        from IPython.terminal.ipapp import load_default_config

        if ui:
            bannermsg += """
Available IPython magics (auto magic is on, `%` is optional):
 time:   measure time
 timeit: benchmark
 trace:  run and print ASCII trace (better with --trace command flag)
 hg:     run commands inline
"""
        have_ipython = True
    except ImportError:
        pass

    if not have_ipython:
        # Fallback to stdlib REPL
        import code

        readfunc = None
        try:
            import readline

            readline.parse_and_bind("tab: complete")
        except ImportError:
            pass

        code.interact(local=env, banner=bannermsg, readfunc=readfunc)
        return

    config = load_default_config()
    config.InteractiveShellEmbed = config.TerminalInteractiveShell
    config.InteractiveShell.automagic = True
    config.InteractiveShell.banner2 = bannermsg
    config.InteractiveShell.confirm_exit = False

    if util.istest():
        # Disable history during tests.
        config.HistoryAccessor.enabled = False

    # Insert a dummy SIGINT handler to workaround a prompt-toolkit bug.
    # See https://github.com/prompt-toolkit/python-prompt-toolkit/commit/6a24c99f7db0729d60d7d56f9759db617f266164
    import signal

    signal.signal(signal.SIGINT, signal.SIG_DFL)

    globals().update(env)
    shell = InteractiveShellEmbed.instance(
        config=config, user_ns=globals(), user_module=sys.modules[__name__]
    )
    if ui:
        _configipython(ui, shell)
    shell()


def c(args: List[str]) -> bytes:
    """Run command with args and take its output.

    Example::

        c(['log', '-r.'])
        c('log -r.')
        %trace c('log -r.')
    """
    if isinstance(args, str):
        args = shlex.split(args)
    ui = globals()["ui"]
    fin = io.BytesIO()
    fout = io.BytesIO()
    bindings.commands.run(["hg"] + args, fin, fout, ui.ferr)
    return fout.getvalue()


def _configipython(ui, ipython) -> None:
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


def _execwith(td, code, ns: Optional[Dict[str, Any]]) -> None:
    with td:
        exec(code, ns)
