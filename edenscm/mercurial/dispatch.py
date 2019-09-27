# dispatch.py - command dispatching for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import difflib
import errno
import getopt
import os
import pdb
import re
import signal
import socket
import sys
import time
import traceback

import bindings

from . import (
    blackbox,
    cmdutil,
    color,
    commands,
    demandimport,
    encoding,
    error,
    extensions,
    help,
    hg,
    hintutil,
    hook,
    i18n,
    perftrace,
    profiling,
    pycompat,
    registrar,
    scmutil,
    ui as uimod,
    util,
)
from .i18n import _


cliparser = bindings.cliparser


unrecoverablewrite = registrar.command.unrecoverablewrite


class request(object):
    def __init__(
        self,
        args,
        ui=None,
        repo=None,
        fin=None,
        fout=None,
        ferr=None,
        prereposetups=None,
    ):
        self.args = args
        self.ui = ui
        self.repo = repo

        # input/output/error streams
        self.fin = fin
        self.fout = fout
        self.ferr = ferr

        # remember options pre-parsed by _earlyparseopts()
        self.earlyoptions = {}

        # reposetups which run before extensions, useful for chg to pre-fill
        # low-level repo state (for example, changelog) before extensions.
        self.prereposetups = prereposetups or []

    def _runexithandlers(self):
        # Silence potential EPIPE or SIGPIPE errors when writing to stdout or
        # stderr.
        if util.safehasattr(signal, "SIGPIPE"):
            signal.signal(signal.SIGPIPE, signal.SIG_IGN)

        class ignoreerrorui(self.ui.__class__):
            def _write(self, *args, **kwargs):
                try:
                    return super(ignoreerrorui, self)._write(*args, **kwargs)
                except (OSError, IOError):
                    pass

            def _write_err(self, *args, **kwargs):
                try:
                    return super(ignoreerrorui, self)._write_err(*args, **kwargs)
                except (OSError, IOError):
                    pass

        exc = None
        self.ui.__class__ = ignoreerrorui

        handlers = self.ui._exithandlers
        try:
            while handlers:
                func, args, kwargs = handlers.pop()
                try:
                    func(*args, **kwargs)
                except:  # re-raises below
                    if exc is None:
                        exc = sys.exc_info()[1]
                    self.ui.warn(("error in exit handlers:\n"))
                    self.ui.traceback(force=True)
        finally:
            if exc is not None:
                raise exc


def run(args=None, fin=None, fout=None, ferr=None):
    "run the command in sys.argv"
    _initstdio()
    if args is None:
        args = pycompat.sysargv
    req = request(args[1:], fin=fin, fout=fout, ferr=ferr)
    err = None
    try:
        status = (dispatch(req) or 0) & 255
    except error.StdioError as e:
        err = e
        status = -1
    if util.safehasattr(req.ui, "fout"):
        try:
            req.ui.fout.flush()
        except IOError as e:
            err = e
            status = -1
    if util.safehasattr(req.ui, "ferr"):
        if err is not None and err.errno != errno.EPIPE:
            req.ui.ferr.write("abort: %s\n" % encoding.strtolocal(err.strerror))
        req.ui.ferr.flush()
    sys.exit(status & 255)


def _preimportmodules():
    """pre-import modules that are side-effect free (used by chg server)"""
    coremods = [
        "ancestor",
        "archival",
        "bookmarks",
        "branchmap",
        "bundle2",
        "bundlerepo",
        "byterange",
        "changegroup",
        "changelog",
        "color",
        "config",
        "configitems",
        "connectionpool",
        "context",
        "copies",
        "crecord",
        "dagop",
        "dagparser",
        "dagutil",
        "destutil",
        "dirstate",
        "dirstateguard",
        "discovery",
        "exchange",
        "filelog",
        "filemerge",
        "fileset",
        "formatter",
        "graphmod",
        "hbisect",
        "httpclient",
        "httpconnection",
        "httppeer",
        "localrepo",
        "lock",
        "mail",
        "manifest",
        "match",
        "mdiff",
        "merge",
        "mergeutil",
        "minirst",
        "namespaces",
        "node",
        "obsolete",
        "obsutil",
        "parser",
        "patch",
        "pathutil",
        "peer",
        "phases",
        "policy",
        "progress",
        "pushkey",
        "rcutil",
        "repository",
        "repoview",
        "revlog",
        "revset",
        "revsetlang",
        "rewriteutil",
        "rust",
        "scmposix",
        "scmutil",
        "server",
        "setdiscovery",
        "similar",
        "simplemerge",
        "smartset",
        "sshpeer",
        "sshserver",
        "sslutil",
        "store",
        "streamclone",
        "tags",
        "templatefilters",
        "templatekw",
        "templater",
        "transaction",
        "treediscovery",
        "txnutil",
        "url",
        "urllibcompat",
        "vfs",
        "wireproto",
        "worker",
        "__version__",
    ]
    extmods = [
        "absorb",
        "amend",
        "arcdiff",
        "automv",
        "blackbox",
        "checkmessagehook",
        "chistedit",
        "clienttelemetry",
        "clindex",
        "conflictinfo",
        "convert",
        "copytrace",
        "crdump",
        "debugcommitmessage",
        "debugshell",
        "dialect",
        "directaccess",
        "dirsync",
        "extlib",
        "extorder",
        "extutil",
        "fastannotate",
        "fastlog",
        "fastmanifest",
        "fbconduit",
        "fbhistedit",
        "fixcorrupt",
        "fsmonitor",
        "githelp",
        "gitlookup",
        "grpcheck",
        "hgevents",
        "hgsubversion",
        "hiddenerror",
        "histedit",
        "infinitepush",
        "journal",
        "lfs",
        "logginghelper",
        "lz4revlog",
        "mergedriver",
        "morecolors",
        "morestatus",
        "patchrmdir",
        "phabdiff",
        "phabstatus",
        "phrevset",
        "progressfile",
        "pullcreatemarkers",
        "purge",
        "pushrebase",
        "rage",
        "rebase",
        "remotefilelog",
        "remotenames",
        "reset",
        "sampling",
        "schemes",
        "share",
        "shelve",
        "sigtrace",
        "simplecache",
        "smartlog",
        "sparse",
        "sshaskpass",
        "stat",
        "traceprof",
        "treemanifest",
        "tweakdefaults",
        "undo",
    ]
    for name in coremods:
        __import__("edenscm.mercurial.%s" % name)
    for extname in extmods:
        extensions.preimport(extname)


def runchgserver():
    """start the chg server, pre-import bundled extensions"""
    # Clean server - do not load any config files or repos.
    _initstdio()
    ui = uimod.ui()
    repo = None
    args = sys.argv[1:]
    cmd, func, args, globalopts, cmdopts, _foundaliases = _parse(ui, args)
    if not (cmd == "serve" and cmdopts["cmdserver"] == "chgunix2"):
        raise error.ProgrammingError("runchgserver called without chg command")
    from . import chgserver, server

    _preimportmodules()
    service = chgserver.chgunixservice(ui, repo, cmdopts)
    server.runservice(cmdopts, initfn=service.init, runfn=service.run)


def _initstdio():
    for fp in (sys.stdin, sys.stdout, sys.stderr):
        util.setbinary(fp)


def _getsimilar(symbols, value):
    sim = lambda x: difflib.SequenceMatcher(None, value, x).ratio()
    # The cutoff for similarity here is pretty arbitrary. It should
    # probably be investigated and tweaked.
    return [s for s in symbols if sim(s) > 0.6]


def _reportsimilar(write, similar):
    if len(similar) == 1:
        write(_("(did you mean %s?)\n") % similar[0])
    elif similar:
        ss = ", ".join(sorted(similar))
        write(_("(did you mean one of %s?)\n") % ss)


def _formatparse(write, inst):
    similar = []
    if isinstance(inst, error.UnknownIdentifier):
        # make sure to check fileset first, as revset can invoke fileset
        similar = _getsimilar(inst.symbols, inst.function)
    if len(inst.args) > 1:
        write(_("hg: parse error at %s: %s\n") % (inst.args[1], inst.args[0]))
        if inst.args[0][0] == " ":
            write(_("unexpected leading whitespace\n"))
    else:
        write(_("hg: parse error: %s\n") % inst.args[0])
        _reportsimilar(write, similar)
    if inst.hint:
        write(_("(%s)\n") % inst.hint)


def _formatargs(args):
    return " ".join(util.shellquote(a) for a in args)


def dispatch(req):
    "run the command specified in req.args"
    if req.ferr:
        ferr = req.ferr
    elif req.ui:
        ferr = req.ui.ferr
    else:
        ferr = util.stderr

    try:
        if not req.ui:
            req.ui = uimod.ui.load()
        req.earlyoptions.update(_earlyparseopts(req.ui, req.args))
        if req.earlyoptions["traceback"]:
            req.ui.setconfig("ui", "traceback", "on", "--traceback")

        # set ui streams from the request
        if req.fin:
            req.ui.fin = req.fin
        if req.fout:
            req.ui.fout = req.fout
        if req.ferr:
            req.ui.ferr = req.ferr
    except error.Abort as inst:
        ferr.write(_("abort: %s\n") % inst)
        if inst.hint:
            ferr.write(_("(%s)\n") % inst.hint)
        return -1
    except error.ParseError as inst:
        _formatparse(ferr.write, inst)
        return -1

    cmdmsg = _formatargs(req.args)
    starttime = util.timer()
    ret = None
    retmask = 255

    def logatexit():
        ui = req.ui
        if ui.logmeasuredtimes:
            ui.log("measuredtimes", **pycompat.strkwargs(ui._measuredtimes))
        if ui.metrics.stats:
            # Re-arrange metrics so "a_b_c", "a_b_d", "a_c" becomes
            # {'a': {'b': {'c': ..., 'd': ...}, 'c': ...}
            metrics = {}
            for key, value in ui.metrics.stats.items():
                cur = metrics
                names = key.split("_")
                for name in names[:-1]:
                    cur = cur.setdefault(name, {})
                cur[names[-1]] = value
            # pprint.pformat stablizes the output
            from pprint import pformat

            # developer config: devel.print-metrics
            if ui.configbool("devel", "print-metrics"):
                # Print it out.
                msg = "%s\n" % pformat({"metrics": metrics}).replace("'", " ")
                ui.flush()
                ui.write_err(msg, label="ui.metrics")
            # Write to blackbox, and sampling
            ui.log(
                "metrics", pformat({"metrics": metrics}, width=1024), **ui.metrics.stats
            )
        blackbox.sync()

    # by registering this exit handler here, we guarantee that it runs
    # after other exithandlers, like the killpager one
    req.ui.atexit(logatexit)

    try:
        ret = _runcatch(req)
    except error.ProgrammingError as inst:
        req.ui.warn(_("** ProgrammingError: %s\n") % inst)
        if inst.hint:
            req.ui.warn(_("** (%s)\n") % inst.hint)
        raise
    except KeyboardInterrupt as inst:
        try:
            if isinstance(inst, error.SignalInterrupt):
                msg = _("killed!\n")
            else:
                msg = _("interrupted!\n")
            req.ui.warn(msg)
        except error.SignalInterrupt:
            # maybe pager would quit without consuming all the output, and
            # SIGPIPE was raised. we cannot print anything in this case.
            pass
        except IOError as inst:
            if inst.errno != errno.EPIPE:
                raise
        ret = -1
    except IOError as inst:
        # Windows does not have SIGPIPE, so pager exit does not
        # get raised as a SignalInterrupt. Let's handle the error
        # explicitly here
        if not pycompat.iswindows or inst.errno != errno.EINVAL:
            raise
        ret = -1
    finally:
        duration = util.timer() - starttime
        req.ui.flush()
        req.ui.log(
            "command_finish",
            "%s exited %d after %0.2f seconds\n",
            cmdmsg,
            ret or 0,
            duration,
        )

        traces = perftrace.traces()
        if traces:
            threshold = req.ui.configint("tracing", "threshold")
            for trace in traces:
                if trace.duration() > threshold:
                    output = perftrace.asciirender(trace)
                    if req.ui.configbool("tracing", "stderr"):
                        req.ui.warn("%s\n" % output)

                    key = "flat/perftrace-%(host)s-%(pid)s-%(time)s" % {
                        "host": socket.gethostname(),
                        "pid": os.getpid(),
                        "time": time.time(),
                    }
                    splitoutput = output.split("\n")
                    if len(splitoutput) > 200:
                        readableoutput = "\n".join(
                            splitoutput[:200]
                            + ["Perftrace truncated at 200 lines, see {}".format(key)]
                        )
                    else:
                        readableoutput = output
                    req.ui.log(
                        "perftrace",
                        "Trace:\n%s\n",
                        readableoutput,
                        key=key,
                        payload=output,
                    )
                    req.ui.log("perftracekey", "Trace key:%s\n", key, perftracekey=key)

        req.ui._measuredtimes["command_duration"] = duration * 1000
        retmask = req.ui.configint("ui", "exitcodemask")

        try:
            req._runexithandlers()
        except:  # exiting, so no re-raises
            ret = ret or -1
    if ret is None:
        ret = 0
    return ret & retmask


def _runcatch(req):
    def catchterm(*args):
        raise error.SignalInterrupt

    ui = req.ui
    try:
        for name in "SIGBREAK", "SIGHUP", "SIGTERM":
            num = getattr(signal, name, None)
            if num:
                signal.signal(num, catchterm)
    except ValueError:
        pass  # happens if called in a thread

    def _runcatchfunc():
        realcmd = None
        try:
            cmdargs = cliparser.parseargs(req.args[:])
            cmd = cmdargs[0]
            aliases, entry = cmdutil.findcmd(cmd, commands.table, False)
            realcmd = aliases[0]
        except (
            error.UnknownCommand,
            error.AmbiguousCommand,
            IndexError,
            getopt.GetoptError,
            UnicodeDecodeError,
        ):
            # Don't handle this here. We know the command is
            # invalid, but all we're worried about for now is that
            # it's not a command that server operators expect to
            # be safe to offer to users in a sandbox.
            pass

        if realcmd == "serve" and "--read-only" in req.args:
            req.args.remove("--read-only")

            if not req.ui:
                req.ui = uimod.ui.load()
            req.ui.setconfig(
                "hooks", "pretxnopen.readonlyrejectpush", rejectpush, "dispatch"
            )
            req.ui.setconfig(
                "hooks", "prepushkey.readonlyrejectpush", rejectpush, "dispatch"
            )

        if realcmd == "serve" and "--stdio" in cmdargs:
            # Uncontionally turn off narrow-heads. The hg servers use
            # full, revlog-based repos. Keep them using old revlog-based
            # algorithms and do not risk switching to new algorithms.
            if "SSH_CLIENT" in encoding.environ:
                req.ui.setconfig("experimental", "narrow-heads", "false", "serve")

            # We want to constrain 'hg serve --stdio' instances pretty
            # closely, as many shared-ssh access tools want to grant
            # access to run *only* 'hg -R $repo serve --stdio'. We
            # restrict to exactly that set of arguments, and prohibit
            # any repo name that starts with '--' to prevent
            # shenanigans wherein a user does something like pass
            # --debugger or --config=ui.debugger=1 as a repo
            # name. This used to actually run the debugger.
            if (
                len(req.args) != 4
                or req.args[0] != "-R"
                or req.args[1].startswith("--")
                or req.args[2] != "serve"
                or req.args[3] != "--stdio"
            ):
                raise error.Abort(
                    _("potentially unsafe serve --stdio invocation: %r") % (req.args,)
                )

        try:
            debugger = "pdb"
            debugtrace = {"pdb": pdb.set_trace}
            debugmortem = {"pdb": pdb.post_mortem}

            # --config takes prescendence over --configfile, so process
            # --configfile first --config second.
            for configfile in req.earlyoptions["configfile"]:
                req.ui.readconfig(configfile)

            # read --config before doing anything else
            # (e.g. to change trust settings for reading .hg/hgrc)
            cfgs = _parseconfig(req.ui, req.earlyoptions["config"])

            if req.repo:
                for configfile in req.earlyoptions["configfile"]:
                    req.repo.ui.readconfig(configfile)
                # copy configs that were passed on the cmdline (--config) to
                # the repo ui
                for sec, name, val in cfgs:
                    req.repo.ui.setconfig(sec, name, val, source="--config")

            # developer config: ui.debugger
            debugger = ui.config("ui", "debugger")
            debugmod = pdb
            if not debugger or ui.plain():
                # if we are in HGPLAIN mode, then disable custom debugging
                debugger = "pdb"
            elif req.earlyoptions["debugger"]:
                # This import can be slow for fancy debuggers, so only
                # do it when absolutely necessary, i.e. when actual
                # debugging has been requested
                with demandimport.deactivated():
                    try:
                        debugmod = __import__(debugger)
                    except ImportError:
                        pass  # Leave debugmod = pdb

            debugtrace[debugger] = debugmod.set_trace
            debugmortem[debugger] = debugmod.post_mortem

            # enter the debugger before command execution
            if req.earlyoptions["debugger"]:
                ui.disablepager()
                ui.warn(
                    _(
                        "entering debugger - "
                        "type c to continue starting hg or h for help\n"
                    )
                )

                if debugger != "pdb" and debugtrace[debugger] == debugtrace["pdb"]:
                    ui.warn(
                        _("%s debugger specified " "but its module was not found\n")
                        % debugger
                    )
                with demandimport.deactivated():
                    debugtrace[debugger]()
            try:
                return _dispatch(req)
            finally:
                ui.flush()
        except:  # re-raises
            # Potentially enter the debugger when we hit an exception
            startdebugger = req.earlyoptions["debugger"]
            if startdebugger:
                # Enforce the use of ipdb, since it's always available and
                # we can afford the import overhead here.
                with demandimport.deactivated():
                    import ipdb

                traceback.print_exc()
                ipdb.post_mortem(sys.exc_info()[2])
                os._exit(255)
            raise

    return _callcatch(ui, _runcatchfunc)


def _callcatch(ui, func):
    """like scmutil.callcatch but handles more high-level exceptions about
    config parsing and commands. besides, use handlecommandexception to handle
    uncaught exceptions.
    """
    try:
        return scmutil.callcatch(ui, func)
    except error.AmbiguousCommand as inst:

        ui.warn(_("hg: command '%s' is ambiguous:\n") % inst.args[0])

        for match in inst.args[1]:
            cmds = match.split(" or ")
            parts = [cmd.partition(inst.args[0]) for cmd in cmds]
            msg = " or ".join(
                ui.label(part[1], "ui.prefix.component") + part[2] for part in parts
            )

            ui.write("\t%s\n" % msg)

    except error.CommandError as inst:
        if inst.args[0]:
            msgbytes = pycompat.bytestr(inst.args[1])
            ui.warn(_("hg %s: %s\n") % (inst.args[0], msgbytes))
            ui.warn(_("(use 'hg %s -h' to get help)\n") % (inst.args[0],))
        else:
            ui.warn(_("hg: %s\n") % inst.args[1])
            ui.warn(_("(use 'hg -h' to get help)\n"))
    except error.ParseError as inst:
        _formatparse(ui.warn, inst)
        return -1
    except error.UnknownCommand as inst:
        ui.warn(_("unknown command %r\n(use 'hg help' to get help)\n") % inst.args[0])
    except error.UnknownSubcommand as inst:
        cmd, subcmd, allsubcmds = inst.args
        suggested = False
        if subcmd is not None:
            nosubcmdmsg = _("hg %s: unknown subcommand '%s'\n") % (cmd, subcmd)
            sim = _getsimilar(allsubcmds, subcmd)
            if sim:
                ui.warn(nosubcmdmsg)
                _reportsimilar(ui.warn, sim)
                suggested = True
        else:
            nosubcmdmsg = _("hg %s: subcommand required\n") % cmd
        if not suggested:
            ui.pager("help")
            ui.warn(nosubcmdmsg)
            commands.help_(ui, cmd)
    except IOError:
        raise
    except KeyboardInterrupt:
        raise
    except:  # probably re-raises
        # Potentially enter ipdb debugger when we hit an uncaught exception
        if (
            ui.configbool("devel", "debugger")
            and ui.interactive()
            and not ui.pageractive
            and not ui.plain()
            and ui.formatted()
        ):
            ui.write_err(
                _(
                    "Starting ipdb for this exception\nIf you don't want the behavior, set devel.debugger to False\n"
                )
            )

            with demandimport.deactivated():
                import ipdb

            if not ui.tracebackflag:
                traceback.print_exc()
            ipdb.post_mortem(sys.exc_info()[2])
            os._exit(255)
        if not handlecommandexception(ui):
            raise

    return -1


class cmdtemplatestate(object):
    """Template-related state for a command.

    Used together with cmdtemplate=True.

    In MVC's sense, this is the "M". The template language takes the "M" and
    renders the "V".
    """

    def __init__(self, ui, opts):
        self._ui = ui
        self._templ = opts.get("template")
        if self._templ:
            # Suppress traditional outputs.
            ui.pushbuffer()
        self._props = {}

    def setprop(self, name, value):
        self._props[name] = value

    def end(self):
        if self._templ:
            ui = self._ui
            ui.popbuffer()
            text = cmdutil.rendertemplate(self._ui, self._templ, self._props)
            self._ui.write(text)


def _parse(ui, args):
    options = {}
    cmdoptions = {}

    # -cm foo: --config m foo
    fullargs = list(args)
    commandnames = [command for command in commands.table]

    args, options, replace = cliparser.parse(args, True)
    strict = ui.configbool("ui", "strict")
    if args:
        try:
            replacement, aliases = cliparser.expandargs(
                ui._rcfg, commandnames, args, strict
            )
        except cliparser.AmbiguousCommand as e:
            e.args[2].sort()
            possibilities = e.args[2]
            raise error.AmbiguousCommand(e.args[1], possibilities)
        except cliparser.CircularReference as e:
            alias = e.args[1]
            raise error.Abort(_("circular alias: %s") % alias)
        except cliparser.MalformedAlias as e:
            msg = e.args[0]
            raise error.Abort(msg)

    else:
        replacement = []

    if len(replacement) > 0:
        # FIXME: Find a way to avoid calling expandargs twice.
        fullargs = (
            fullargs[:replace]
            + cliparser.expandargs(ui._rcfg, commandnames, fullargs[replace:], strict)[
                0
            ]
        )

        # Only need to figure out the command name. Parse result is dropped.
        cmd, _args, a, entry, level = cmdutil.findsubcmd(
            replacement, commands.table, strict
        )
        c = list(entry[1])
    else:
        aliases = []
        cmd = None
        level = 0
        c = []

    # combine global options into local
    c += commands.globalopts
    # for o in commands.globalopts:
    #    c.append((o[0], o[1], options[o[1]], o[3]))

    try:
        flagdefs = [(flagdef[0], flagdef[1], flagdef[2]) for flagdef in c]

        args, cmdoptions = cliparser.parsecommand(fullargs, flagdefs)
        args = args[level:]
    except (
        cliparser.OptionNotRecognized,
        cliparser.OptionRequiresArgument,
        cliparser.OptionAmbiguous,
    ) as e:
        raise error.CommandError(cmd, e.args[0])
    except cliparser.OptionArgumentInvalid as e:
        raise error.Abort(e.args[0])

    # separate global options back out
    for (k, v) in cmdoptions.items():
        if "-" in k:
            orig = k
            k = k.replace("-", "_")
            cmdoptions[k] = v
            del cmdoptions[orig]
        else:
            cmdoptions[k] = v

    for o in commands.globalopts:
        n = o[1]
        options[n] = cmdoptions[n]
        del cmdoptions[n]

    return (cmd, cmd and entry[0] or None, args, options, cmdoptions, aliases)


def _parseconfig(ui, config):
    """parse the --config options from the command line"""
    configs = []

    for cfg in config:
        try:
            name, value = [cfgelem.strip() for cfgelem in cfg.split("=", 1)]
            section, name = name.split(".", 1)
            if not section or not name:
                raise IndexError
            ui.setconfig(section, name, value, "--config")
            configs.append((section, name, value))
        except (IndexError, ValueError):
            raise error.Abort(
                _("malformed --config option: %r " "(use --config section.name=value)")
                % cfg
            )

    return configs


def _earlyparseopts(ui, args):
    try:
        return cliparser.earlyparse(args)
    except UnicodeDecodeError:
        raise error.Abort(_("cannot decode command line arguments"))


def _joinfullargs(fullargs):
    fullargs = [util.shellquote(arg) for arg in fullargs]
    return " ".join(fullargs)


def runcommand(lui, repo, cmd, fullargs, ui, options, d, cmdpats, cmdoptions):
    # run pre-hook, and abort if it fails
    hook.hook(
        lui,
        repo,
        "pre-%s" % cmd,
        True,
        args=_joinfullargs(fullargs),
        pats=cmdpats,
        opts=cmdoptions,
    )
    try:
        hintutil.loadhintconfig(lui)
        ui.log("jobid", jobid=encoding.environ.get("HG_JOB_ID", "unknown"))
        ret = _runcommand(ui, options, cmd, d)
        # run post-hook, passing command result
        hook.hook(
            lui,
            repo,
            "post-%s" % cmd,
            False,
            args=" ".join(fullargs),
            result=ret,
            pats=cmdpats,
            opts=cmdoptions,
        )
    except Exception as e:
        # run failure hook and re-raise
        hook.hook(
            lui,
            repo,
            "fail-%s" % cmd,
            False,
            args=" ".join(fullargs),
            pats=cmdpats,
            opts=cmdoptions,
        )
        _log_exception(lui, e)
        raise
    return ret


def _log_exception(lui, e):
    try:
        lui.log("exceptions", exception_type=type(e).__name__, exception_msg=str(e))
    except Exception:
        pass


def _getlocal(ui, rpath, wd=None):
    """Return (path, local ui object) for the given target path.

    Takes paths in [cwd]/.hg/hgrc into account."
    """
    if wd is None:
        try:
            wd = pycompat.getcwd()
        except OSError as e:
            if e.errno == errno.ENOTCONN:
                ui.warn(_("current working directory is not connected\n"))
                ui.warn(
                    _(
                        "(for virtual checkouts, run '@prog@ fs doctor' to diagnose issues with edenfs)\n"
                    )
                )
                return "", ui
            raise error.Abort(
                _("error getting current working directory: %s")
                % encoding.strtolocal(e.strerror)
            )
    path = cmdutil.findrepo(wd) or ""
    if not path:
        lui = ui
    else:
        lui = ui.copy()
        lui.readconfig(os.path.join(path, ".hg", "hgrc"), path)

    if rpath:
        path = lui.expandpath(rpath)
        lui = ui.copy()
        lui.readconfig(os.path.join(path, ".hg", "hgrc"), path)

    return path, lui


def _dispatch(req):
    args = req.args
    ui = req.ui

    # check for cwd
    cwd = req.earlyoptions["cwd"]
    if cwd:
        os.chdir(cwd)

    rpath = req.earlyoptions["repository"]
    path, lui = _getlocal(ui, rpath)

    uis = {ui, lui}

    if req.repo:
        uis.add(req.repo.ui)

    if req.earlyoptions["profile"]:
        for ui_ in uis:
            ui_.setconfig("profiling", "enabled", "true", "--profile")

    with profiling.profile(lui) as profiler:
        # Configure extensions in phases: uisetup, extsetup, cmdtable, and
        # reposetup
        extensions.loadall(lui)
        # Propagate any changes to lui.__class__ by extensions
        ui.__class__ = lui.__class__

        # setup color handling before pager, because setting up pager
        # might cause incorrect console information
        coloropt = req.earlyoptions.get("color", False)
        for ui_ in uis:
            if coloropt:
                ui_.setconfig("ui", "color", coloropt, "--color")
            color.setup(ui_)

        # (uisetup and extsetup are handled in extensions.loadall)

        # (reposetup is handled in hg.repository)

        # check for fallback encoding
        fallback = lui.config("ui", "fallbackencoding")
        if fallback:
            encoding.fallbackencoding = fallback

        fullargs = args

        cmd, func, args, options, cmdoptions, foundaliases = _parse(lui, args)

        if cmd == "help" and len(foundaliases) > 0:
            cmd = foundaliases[0]
            options["help"] = True

        if options["encoding"]:
            encoding.encoding = options["encoding"]
        if options["encodingmode"]:
            encoding.encodingmode = options["encodingmode"]
        i18n.init()

        if options["config"] != req.earlyoptions["config"]:
            raise error.Abort(_("option --config may not be abbreviated!"))
        if options["configfile"] != req.earlyoptions["configfile"]:
            raise error.Abort(_("option --configfile may not be abbreviated!"))
        if options["cwd"] != req.earlyoptions["cwd"]:
            raise error.Abort(_("option --cwd may not be abbreviated!"))
        if options["repository"] != req.earlyoptions["repository"]:
            raise error.Abort(
                _(
                    "option -R has to be separated from other options (e.g. not "
                    "-qR) and --repository may only be abbreviated as --repo!"
                )
            )
        if options["debugger"] != req.earlyoptions["debugger"]:
            raise error.Abort(_("option --debugger may not be abbreviated!"))
        # don't validate --profile/--traceback, which can be enabled from now

        if options["time"]:

            def get_times():
                t = os.times()
                if t[4] == 0.0:
                    # Windows leaves this as zero, so use time.clock()
                    t = (t[0], t[1], t[2], t[3], time.clock())
                return t

            s = get_times()

            def print_time():
                t = get_times()
                ui.warn(
                    _("time: real %.3f secs (user %.3f+%.3f sys %.3f+%.3f)\n")
                    % (t[4] - s[4], t[0] - s[0], t[2] - s[2], t[1] - s[1], t[3] - s[3])
                )

            ui.atexit(print_time)
        if options["profile"]:
            profiler.start()

        if options["verbose"] or options["debug"] or options["quiet"]:
            for opt in ("verbose", "debug", "quiet"):
                val = str(bool(options[opt]))
                if pycompat.ispy3:
                    val = val.encode("ascii")
                for ui_ in uis:
                    ui_.setconfig("ui", opt, val, "--" + opt)

        if options["traceback"]:
            for ui_ in uis:
                ui_.setconfig("ui", "traceback", "on", "--traceback")

        if options["noninteractive"]:
            for ui_ in uis:
                ui_.setconfig("ui", "interactive", "off", "-y")

        if cmdoptions.get("insecure", False):
            for ui_ in uis:
                ui_.insecureconnections = True

        if util.parsebool(options["pager"]):
            # ui.pager() expects 'internal-always-' prefix in this case
            ui.pager("internal-always-" + cmd)
        elif options["pager"] != "auto":
            for ui_ in uis:
                ui_.disablepager()

        if options["version"]:
            return commands.version_(ui)
        if options["help"]:
            if len(foundaliases) > 0:
                aliascmd = foundaliases[0]
                return commands.help_(ui, aliascmd, command=cmd is not None)

            return commands.help_(ui, cmd, command=cmd is not None)
        elif not cmd:
            return commands.help_(ui)

        msg = _formatargs(fullargs)
        with perftrace.trace("hg " + msg):
            repo = None
            if func.cmdtemplate:
                templ = cmdtemplatestate(ui, cmdoptions)
                args.insert(0, templ)
                ui.atexit(templ.end)
            cmdpats = args[:]
            if not func.norepo:
                # use the repo from the request only if we don't have -R
                if not rpath and not cwd:
                    repo = req.repo

                if repo:
                    # set the descriptors of the repo ui to those of ui
                    repo.ui.fin = ui.fin
                    repo.ui.fout = ui.fout
                    repo.ui.ferr = ui.ferr
                else:
                    try:
                        repo = hg.repository(
                            ui, path=path, presetupfuncs=req.prereposetups
                        )
                        if not repo.local():
                            raise error.Abort(_("repository '%s' is not local") % path)
                        _initblackbox(req, repo, func.cmdtype)
                        repo.ui.setconfig("bundle", "mainreporoot", repo.root, "repo")
                    except error.RequirementError:
                        raise
                    except error.RepoError:
                        if rpath:  # invalid -R path
                            raise
                        if not func.optionalrepo:
                            if func.inferrepo and args and not path:
                                # try to infer -R from command args
                                repos = pycompat.maplist(cmdutil.findrepo, args)
                                guess = repos[0]
                                if guess and repos.count(guess) == len(repos):
                                    req.args = ["--repository", guess] + fullargs
                                    req.earlyoptions["repository"] = guess
                                    return _dispatch(req)
                            if not path:
                                raise error.RepoError(
                                    _(
                                        "'%s' is not inside a repository, but this command requires a repository"
                                    )
                                    % pycompat.getcwd(),
                                    hint=_(
                                        "use 'cd' to go to a directory inside a repository and try again"
                                    ),
                                )
                            raise
                if repo:
                    ui = repo.ui
                    if options["hidden"]:
                        repo = repo.unfiltered()
                        repo.ui.setconfig("visibility", "all-heads", "true", "--hidden")
                    if repo != req.repo:
                        ui.atexit(repo.close)
                args.insert(0, repo)
            elif rpath:
                ui.warn(_("warning: --repository ignored\n"))

            from . import mdiff

            mdiff.init(ui)

            ui.log("command", "%s\n", msg)
            if repo:
                repo.dirstate.loginfo(ui, "pre")
            strcmdopt = pycompat.strkwargs(cmdoptions)
            d = lambda: util.checksignature(func)(ui, *args, **strcmdopt)
            ret = runcommand(
                lui, repo, cmd, fullargs, ui, options, d, cmdpats, cmdoptions
            )
            hintutil.show(lui)
            if repo:
                repo.dirstate.loginfo(ui, "post")
            return ret


def _initblackbox(req, repo, cmdtype):
    """Initialize the native blackbox logging at the shared repo path.

    This might choose to disable logging if the blackbox extension is disabled
    via '--config=extensions.blackbox=!' or '--config=blackbox.track=', and
    the command is read-only. (In other words, read-write commands will always
    be logged)
    """
    # See "class command" in registrar.py for valid command types.
    # Enforce blackbox logging for non-readonly commands, so if an automation
    # runs commands like `hg commit --config extensions.blackbox=!`, we still
    # log it.
    if cmdtype == "readonly":
        config = req.earlyoptions["config"]
        if "extensions.blackbox=!" in config or "blackbox.track=" in config:
            # Explicitly disabled via command line. Do not initialize blackbox.
            return

    # Create the log in sharedvfs.
    path = repo.sharedvfs.join("blackbox", "v1")
    size = repo.ui.configbytes("blackbox", "maxsize")
    count = repo.ui.configint("blackbox", "maxfiles")
    try:
        blackbox.init(path, count, size)
    except IOError:
        # Likely permission errors. Not fatal.
        pass


def _runcommand(ui, options, cmd, cmdfunc):
    """Run a command function, possibly with profiling enabled."""
    try:
        return cmdfunc()
    except error.SignatureError:
        raise error.CommandError(cmd, _("invalid arguments"))


def _exceptionwarning(ui):
    """Produce a warning message for the current active exception"""

    return _("** @LongProduct@ (version %s) has crashed:\n") % util.version()


def handlecommandexception(ui):
    """Produce a warning message for broken commands

    Called when handling an exception; the exception is reraised if
    this function returns False, ignored otherwise.
    """
    warning = _exceptionwarning(ui)
    ui.log("command_exception", "%s\n%s\n", warning, traceback.format_exc())
    ui.warn(warning)
    return False  # re-raise the exception


def rejectpush(ui, **kwargs):
    ui.warn(("Permission denied - blocked by readonlyrejectpush hook\n"))
    # mercurial hooks use unix process conventions for hook return values
    # so a truthy return means failure
    return True
