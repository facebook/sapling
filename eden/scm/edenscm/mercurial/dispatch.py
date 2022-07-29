# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# dispatch.py - command dispatching for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
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
import sys
import time

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
    uiconfig,
    util,
)
from .i18n import _, _x


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

        # The repo, if any, that ends up being used for command execution.
        self.cmdrepo = None

        # input/output/error streams
        if fin and not isinstance(fin, util.refcell):
            fin = util.refcell(fin)
        if fout and not isinstance(fout, util.refcell):
            fout = util.refcell(fout)
        if ferr and not isinstance(ferr, util.refcell):
            ferr = util.refcell(ferr)

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
            try:
                util.signal(signal.SIGPIPE, signal.SIG_IGN)
            except ValueError:
                # This can happen if the command runs as a library from a
                # non-main thread.
                if not util.istest():
                    raise

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
                    self.ui.warn(_x("error in exit handlers:\n"))
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
            errormsg = pycompat.encodeutf8(err.strerror)
            req.ui.ferr.write(b"abort: %s\n" % errormsg)
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
        "destutil",
        "dirstate",
        "dirstateguard",
        "discovery",
        "eden_dirstate",
        "exchange",
        "filelog",
        "filemerge",
        "fileset",
        "formatter",
        "graphmod",
        "hbisect",
        "httpclient",
        "httpconnection",
        "localrepo",
        "lock",
        "mail",
        "manifest",
        "match",
        "mdiff",
        "merge",
        "mergeutil",
        "minirst",
        "mononokepeer",
        "namespaces",
        "node",
        "obsolete",
        "parser",
        "patch",
        "pathutil",
        "peer",
        "phases",
        "progress",
        "pushkey",
        "rcutil",
        "repository",
        "revlog",
        "revset",
        "revsetlang",
        "rewriteutil",
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
        "statprof",
        "store",
        "streamclone",
        "templatefilters",
        "templatekw",
        "templater",
        "transaction",
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
        "commitcloud",
        "conflictinfo",
        "copytrace",
        "crdump",
        "debugcommitmessage",
        "debugshell",
        "dialect",
        "dirsync",
        "extlib",
        "extorder",
        "extutil",
        "fastannotate",
        "fastlog",
        "fbscmquery",
        "fbhistedit",
        "fsmonitor",
        "githelp",
        "grpcheck",
        "hgevents",
        "histedit",
        "infinitepush",
        "journal",
        "lfs",
        "logginghelper",
        "lz4revlog",
        "mergedriver",
        "morestatus",
        "myparent",
        "phabdiff",
        "phabstatus",
        "phrevset",
        "progressfile",
        "pullcreatemarkers",
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
        "snapshot",
        "sparse",
        "sshaskpass",
        "stablerev",
        "stat",
        "traceprof",
        "treemanifest",
        "tweakdefaults",
        "undo",
    ]
    modnames = ["edenscm.mercurial.%s" % name for name in coremods] + [
        "edenscm.hgext.fastannotate.support"
    ]
    for name in modnames:
        __import__(name)
    for extname in extmods:
        extensions.preimport(extname)


ischgserver = False


def runchgserver():
    """start the chg server, pre-import bundled extensions"""
    global ischgserver
    ischgserver = True
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
        if req.earlyoptions["traceback"] or req.earlyoptions["trace"]:
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

    if req.ui.configbool("ui", "threaded"):
        util.preregistersighandlers()

    cmdmsg = _formatargs(req.args)
    starttime = util.timer()
    ret = None
    retmask = 255

    def logatexit():
        ui = req.ui
        if ui.logmeasuredtimes:
            ui.log("measuredtimes", **(ui._measuredtimes))
        hgmetrics = bindings.hgmetrics.summarize()
        if hgmetrics:
            # Comma-separated list of metric prefixes to skip
            # TODO(meyer): Just skip printing metrics, rather than skipping logging them entirely.
            skip = ui.configlist("devel", "skip-metrics", [])
            # Re-arrange metrics so "a_b_c", "a_b_d", "a_c" becomes
            # {'a': {'b': {'c': ..., 'd': ...}, 'c': ...}
            metrics = {}
            splitre = re.compile(r"_|/|\.")
            for key, value in hgmetrics.items():
                for prefix in skip:
                    if key.startswith(prefix):
                        break
                else:
                    cur = metrics
                    names = splitre.split(key)
                    for name in names[:-1]:
                        cur = cur.setdefault(name, {})
                    cur[names[-1]] = value
            # pprint.pformat stablizes the output
            from pprint import pformat

            if metrics:
                # developer config: devel.print-metrics
                if ui.configbool("devel", "print-metrics"):
                    # Print it out.
                    msg = "%s\n" % pformat({"metrics": metrics}).replace("'", " ")
                    ui.flush()
                    ui.write_err(msg, label="hgmetrics")

                # Write to blackbox, and sampling
                ui.log(
                    "metrics", pformat({"metrics": metrics}, width=1024), **hgmetrics
                )
        blackbox.sync()

    versionthresholddays = req.ui.configint("ui", "version-age-threshold-days")
    versionagedays = util.versionagedays()
    if versionthresholddays and versionagedays > versionthresholddays:
        hintutil.trigger("old-version", versionagedays)

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
                util.signal(num, catchterm)
    except ValueError:
        pass  # happens if called in a thread
    # In some cases, SIGINT handler is set to SIG_IGN on OSX.
    # Reset it to raise KeyboardInterrupt.
    sigint = getattr(signal, "SIGINT", None)
    if sigint is not None and util.getsignal(sigint) == signal.SIG_IGN:
        util.signal(sigint, signal.default_int_handler)

    realcmd = None
    try:
        cmdargs = cliparser.parseargs(req.args[:])
        cmd = cmdargs[0]
        aliases, entry = cmdutil.findcmd(cmd, commands.table)
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

    def _runcatchfunc():
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
            if "SSH_CLIENT" in encoding.environ and req.ui.configbool(
                "experimental", "disable-narrow-heads-ssh-server"
            ):
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

            # read --config before doing anything else
            # (e.g. to change trust settings for reading .hg/hgrc)
            _parseconfig(req.ui, req.earlyoptions)
            if req.repo:
                _parseconfig(req.repo.ui, req.earlyoptions)

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

                ui.write_err(util.smartformatexc())
                ipdb.post_mortem(sys.exc_info()[2])
                os._exit(255)
            raise

    # IPython (debugshell) has some background (sqlite?) threads that is incompatible
    # with util.threaded. But "debugshell -c" should use util.threaded.
    if (
        ui.configbool("ui", "threaded")
        and req.args not in [["dbsh"], ["debugsh"], ["debugshell"]]
        and realcmd != "serve"
    ):
        # Run command (and maybe start ipdb) in a background thread for
        # better Ctrl+C handling of long native logic.
        return util.threaded(_callcatch)(ui, req, _runcatchfunc)
    else:
        # Run in the main thread.
        return _callcatch(ui, req, _runcatchfunc)


def _callcatch(ui, req, func):
    """like scmutil.callcatch but handles more high-level exceptions about
    config parsing and commands. besides, use handlecommandexception to handle
    uncaught exceptions.
    """
    try:
        return scmutil.callcatch(ui, req, func)
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
        cmd, subcmd = inst.args[:2]
        if subcmd is not None:
            nosubcmdmsg = _("hg %s: unknown subcommand '%s'\n") % (cmd, subcmd)
        else:
            nosubcmdmsg = _("hg %s: subcommand required\n") % cmd
        ui.warn(nosubcmdmsg)
    except IOError:
        raise
    except KeyboardInterrupt:
        raise
    except:  # probably re-raises
        # Potentially enter ipdb debugger when we hit an uncaught exception
        ex = sys.exc_info()[1]
        isipdb = isinstance(ex, NameError) and "'ipdb'" in str(ex)
        if (
            (ui.configbool("devel", "debugger") or isipdb)
            and ui.interactive()
            and not ui.pageractive
            and not ui.plain()
            and ui.formatted
        ):
            if isipdb:
                ui.write_err(_("Starting ipdb for 'ipdb'\n"))
            else:
                ui.write_err(
                    _(
                        "Starting ipdb for this exception\nIf you don't want the behavior, set devel.debugger to False\n"
                    )
                )

            with demandimport.deactivated():
                import ipdb

            if not ui.tracebackflag:
                ui.write_err(util.smartformatexc())
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
        cmd, _args, a, entry, level = cmdutil.findsubcmd(replacement, commands.table)
        c = list(entry[1])
    else:
        aliases = []
        cmd = None
        level = 0
        c = []

    # combine global options into local
    c += commands.globalopts

    try:
        args, cmdoptions = cliparser.parsecommand(fullargs, c)
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
    for o in commands.globalopts:
        n = o[1]
        options[n] = cmdoptions[n]
        del cmdoptions[n]

    return (cmd, cmd and entry[0] or None, args, options, cmdoptions, aliases)


def _parseconfig(ui, earlyoptions):
    """parse the --config options from the command line"""

    configfiles = ui.configlist("_configs", "configfiles", [])

    # --config takes prescendence over --configfile, so process
    # --configfile first then --config second.
    for configfile in earlyoptions["configfile"]:
        configfiles.append(configfile)
        tempconfig = uiconfig.uiconfig()
        tempconfig.readconfig(configfile)
        # Set the configfile values one-by-one so they get put in the internal
        # _pinnedconfigs list and don't get overwritten in the future.
        for section, name, value in tempconfig.walkconfig():
            ui.setconfig(section, name, value, configfile)

    for cfg in earlyoptions["config"]:
        try:
            name, value = [cfgelem.strip() for cfgelem in cfg.split("=", 1)]
            section, name = name.split(".", 1)
            if not section or not name:
                raise IndexError
            ui.setconfig(section, name, value, "--config")
        except (IndexError, ValueError):
            raise error.Abort(
                _("malformed --config option: %r " "(use --config section.name=value)")
                % cfg
            )

    if configfiles:
        ui.setconfig("_configs", "configfiles", configfiles)


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

        # Special case clone return value so we have access to the new repo.
        if cmd == "clone":
            repo = ret
            ret = 0 if repo else 1

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
    if getattr(repo, "_txnreleased", False):
        hook.hook(lui, repo, "postwritecommand", False)
    util.printrecordedtracebacks()
    return ret


def _log_exception(lui, e):
    try:
        lui.log("exceptions", exception_type=type(e).__name__, exception_msg=str(e))
    except Exception as e:
        try:
            wrapped = error.ProgrammingError("failed to log exception: {!r}".format(e))
            lui.log(
                "exceptions",
                exception_type=type(wrapped).__name__,
                exception_msg=str(wrapped),
            )
        except Exception:
            # If there's an unrecoverable error in the exception handling flow
            # we'd rather ignore it and not shadow the original exception
            # (which wouldn't be re-raised if we throw an exception).
            pass


def _getlocal(ui, rpath):
    """Return (path, local ui object) for the given target path.

    Takes paths in [cwd]/.hg/hgrc into account."
    """
    if rpath:
        path = ui.expandpath(rpath)
        lui = ui.copy()
    else:
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

    # Don't load repo configs if the path is a bundle file.
    if path and os.path.isdir(path):
        lui.reloadconfigs(path)

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
        lui.cmdname = ui.cmdname = cmd

        # Do not profile the 'debugshell' command.
        if cmd == "debugshell":
            profiler.stop()

        if cmd == "help" and len(foundaliases) > 0:
            cmd = foundaliases[0]
            options["help"] = True

        if options["encoding"]:
            encoding.encoding = options["encoding"]
        if options["encodingmode"]:
            encoding.encodingmode = options["encodingmode"]
        if (
            options["outputencoding"]
            and options["outputencoding"] != "same as encoding"
        ):
            encoding.outputencoding = options["outputencoding"]
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
                    if sys.version_info[0] >= 3:
                        x = time.perf_counter()
                    else:
                        x = time.clock()
                    t = (t[0], t[1], t[2], t[3], x)
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
                for ui_ in uis:
                    ui_.setconfig("ui", opt, val, "--" + opt)

        if options["traceback"]:
            for ui_ in uis:
                ui_.setconfig("ui", "traceback", "on", "--traceback")

        if options["noninteractive"]:
            for ui_ in uis:
                ui_.setconfig("ui", "interactive", "off", "-y")

        if options["insecure"]:
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
        with perftrace.trace("Main Python Command"):
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
                        scmutil.setup(repo.ui)
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
                        repo.ui.setconfig("visibility", "all-heads", "true", "--hidden")
                    if repo != req.repo:
                        ui.atexit(repo.close)

                    # Stuff this in to the request so exception handling code has access to repo/repo.ui.
                    req.cmdrepo = repo
                args.insert(0, repo)
            elif rpath:
                ui.warn(_("warning: --repository ignored\n"))

            from . import match as matchmod, mdiff

            mdiff.init(ui)
            matchmod.init(ui)

            ui.log("command", "%s\n", msg)
            if repo:
                repo.dirstate.loginfo(ui, "pre")
            strcmdopt = cmdoptions
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
    ui.log("command_exception", "%s\n%s\n", warning, util.smartformatexc())
    ui.warn(warning)
    return False  # re-raise the exception


def rejectpush(ui, **kwargs):
    ui.warn(_x("Permission denied - blocked by readonlyrejectpush hook\n"))
    # mercurial hooks use unix process conventions for hook return values
    # so a truthy return means failure
    return True
