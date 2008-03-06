# dispatch.py - command dispatching for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
from repo import RepoError
import os, sys, atexit, signal, pdb, traceback, socket, errno, shlex, time
import util, commands, hg, lock, fancyopts, revlog, version, extensions, hook
import cmdutil
import ui as _ui

class ParseError(Exception):
    """Exception raised on errors in parsing the command line."""

def run():
    "run the command in sys.argv"
    sys.exit(dispatch(sys.argv[1:]))

def dispatch(args):
    "run the command specified in args"
    try:
        u = _ui.ui(traceback='--traceback' in args)
    except util.Abort, inst:
        sys.stderr.write(_("abort: %s\n") % inst)
        return -1
    return _runcatch(u, args)

def _runcatch(ui, args):
    def catchterm(*args):
        raise util.SignalInterrupt

    for name in 'SIGBREAK', 'SIGHUP', 'SIGTERM':
        num = getattr(signal, name, None)
        if num: signal.signal(num, catchterm)

    try:
        try:
            # enter the debugger before command execution
            if '--debugger' in args:
                pdb.set_trace()
            try:
                return _dispatch(ui, args)
            finally:
                ui.flush()
        except:
            # enter the debugger when we hit an exception
            if '--debugger' in args:
                pdb.post_mortem(sys.exc_info()[2])
            ui.print_exc()
            raise

    except ParseError, inst:
        if inst.args[0]:
            ui.warn(_("hg %s: %s\n") % (inst.args[0], inst.args[1]))
            commands.help_(ui, inst.args[0])
        else:
            ui.warn(_("hg: %s\n") % inst.args[1])
            commands.help_(ui, 'shortlist')
    except cmdutil.AmbiguousCommand, inst:
        ui.warn(_("hg: command '%s' is ambiguous:\n    %s\n") %
                (inst.args[0], " ".join(inst.args[1])))
    except cmdutil.UnknownCommand, inst:
        ui.warn(_("hg: unknown command '%s'\n") % inst.args[0])
        commands.help_(ui, 'shortlist')
    except RepoError, inst:
        ui.warn(_("abort: %s!\n") % inst)
    except lock.LockHeld, inst:
        if inst.errno == errno.ETIMEDOUT:
            reason = _('timed out waiting for lock held by %s') % inst.locker
        else:
            reason = _('lock held by %s') % inst.locker
        ui.warn(_("abort: %s: %s\n") % (inst.desc or inst.filename, reason))
    except lock.LockUnavailable, inst:
        ui.warn(_("abort: could not lock %s: %s\n") %
               (inst.desc or inst.filename, inst.strerror))
    except revlog.RevlogError, inst:
        ui.warn(_("abort: %s!\n") % inst)
    except util.SignalInterrupt:
        ui.warn(_("killed!\n"))
    except KeyboardInterrupt:
        try:
            ui.warn(_("interrupted!\n"))
        except IOError, inst:
            if inst.errno == errno.EPIPE:
                if ui.debugflag:
                    ui.warn(_("\nbroken pipe\n"))
            else:
                raise
    except socket.error, inst:
        ui.warn(_("abort: %s\n") % inst[1])
    except IOError, inst:
        if hasattr(inst, "code"):
            ui.warn(_("abort: %s\n") % inst)
        elif hasattr(inst, "reason"):
            try: # usually it is in the form (errno, strerror)
                reason = inst.reason.args[1]
            except: # it might be anything, for example a string
                reason = inst.reason
            ui.warn(_("abort: error: %s\n") % reason)
        elif hasattr(inst, "args") and inst[0] == errno.EPIPE:
            if ui.debugflag:
                ui.warn(_("broken pipe\n"))
        elif getattr(inst, "strerror", None):
            if getattr(inst, "filename", None):
                ui.warn(_("abort: %s: %s\n") % (inst.strerror, inst.filename))
            else:
                ui.warn(_("abort: %s\n") % inst.strerror)
        else:
            raise
    except OSError, inst:
        if getattr(inst, "filename", None):
            ui.warn(_("abort: %s: %s\n") % (inst.strerror, inst.filename))
        else:
            ui.warn(_("abort: %s\n") % inst.strerror)
    except util.UnexpectedOutput, inst:
        ui.warn(_("abort: %s") % inst[0])
        if not isinstance(inst[1], basestring):
            ui.warn(" %r\n" % (inst[1],))
        elif not inst[1]:
            ui.warn(_(" empty string\n"))
        else:
            ui.warn("\n%r\n" % util.ellipsis(inst[1]))
    except ImportError, inst:
        m = str(inst).split()[-1]
        ui.warn(_("abort: could not import module %s!\n") % m)
        if m in "mpatch bdiff".split():
            ui.warn(_("(did you forget to compile extensions?)\n"))
        elif m in "zlib".split():
            ui.warn(_("(is your Python install correct?)\n"))

    except util.Abort, inst:
        ui.warn(_("abort: %s\n") % inst)
    except MemoryError:
        ui.warn(_("abort: out of memory\n"))
    except SystemExit, inst:
        # Commands shouldn't sys.exit directly, but give a return code.
        # Just in case catch this and and pass exit code to caller.
        return inst.code
    except:
        ui.warn(_("** unknown exception encountered, details follow\n"))
        ui.warn(_("** report bug details to "
                 "http://www.selenic.com/mercurial/bts\n"))
        ui.warn(_("** or mercurial@selenic.com\n"))
        ui.warn(_("** Mercurial Distributed SCM (version %s)\n")
               % version.get_version())
        raise

    return -1

def _findrepo(p):
    while not os.path.isdir(os.path.join(p, ".hg")):
        oldp, p = p, os.path.dirname(p)
        if p == oldp:
            return None

    return p

def _parse(ui, args):
    options = {}
    cmdoptions = {}

    try:
        args = fancyopts.fancyopts(args, commands.globalopts, options)
    except fancyopts.getopt.GetoptError, inst:
        raise ParseError(None, inst)

    if args:
        cmd, args = args[0], args[1:]
        aliases, i = cmdutil.findcmd(ui, cmd, commands.table)
        cmd = aliases[0]
        defaults = ui.config("defaults", cmd)
        if defaults:
            args = shlex.split(defaults) + args
        c = list(i[1])
    else:
        cmd = None
        c = []

    # combine global options into local
    for o in commands.globalopts:
        c.append((o[0], o[1], options[o[1]], o[3]))

    try:
        args = fancyopts.fancyopts(args, c, cmdoptions)
    except fancyopts.getopt.GetoptError, inst:
        raise ParseError(cmd, inst)

    # separate global options back out
    for o in commands.globalopts:
        n = o[1]
        options[n] = cmdoptions[n]
        del cmdoptions[n]

    return (cmd, cmd and i[0] or None, args, options, cmdoptions)

def _parseconfig(config):
    """parse the --config options from the command line"""
    parsed = []
    for cfg in config:
        try:
            name, value = cfg.split('=', 1)
            section, name = name.split('.', 1)
            if not section or not name:
                raise IndexError
            parsed.append((section, name, value))
        except (IndexError, ValueError):
            raise util.Abort(_('malformed --config option: %s') % cfg)
    return parsed

def _earlygetopt(aliases, args):
    """Return list of values for an option (or aliases).

    The values are listed in the order they appear in args.
    The options and values are removed from args.
    """
    try:
        argcount = args.index("--")
    except ValueError:
        argcount = len(args)
    shortopts = [opt for opt in aliases if len(opt) == 2]
    values = []
    pos = 0
    while pos < argcount:
        if args[pos] in aliases:
            if pos + 1 >= argcount:
                # ignore and let getopt report an error if there is no value
                break
            del args[pos]
            values.append(args.pop(pos))
            argcount -= 2
        elif args[pos][:2] in shortopts:
            # short option can have no following space, e.g. hg log -Rfoo
            values.append(args.pop(pos)[2:])
            argcount -= 1
        else:
            pos += 1
    return values

_loaded = {}
def _dispatch(ui, args):
    # read --config before doing anything else
    # (e.g. to change trust settings for reading .hg/hgrc)
    config = _earlygetopt(['--config'], args)
    if config:
        ui.updateopts(config=_parseconfig(config))

    # check for cwd
    cwd = _earlygetopt(['--cwd'], args)
    if cwd:
        os.chdir(cwd[-1])

    # read the local repository .hgrc into a local ui object
    path = _findrepo(os.getcwd()) or ""
    if not path:
        lui = ui
    if path:
        try:
            lui = _ui.ui(parentui=ui)
            lui.readconfig(os.path.join(path, ".hg", "hgrc"))
        except IOError:
            pass

    # now we can expand paths, even ones in .hg/hgrc
    rpath = _earlygetopt(["-R", "--repository", "--repo"], args)
    if rpath:
        path = lui.expandpath(rpath[-1])
        lui = _ui.ui(parentui=ui)
        lui.readconfig(os.path.join(path, ".hg", "hgrc"))

    extensions.loadall(lui)
    for name, module in extensions.extensions():
        if name in _loaded:
            continue

        # setup extensions
        # TODO this should be generalized to scheme, where extensions can
        #      redepend on other extensions.  then we should toposort them, and
        #      do initialization in correct order
        extsetup = getattr(module, 'extsetup', None)
        if extsetup:
            extsetup()

        cmdtable = getattr(module, 'cmdtable', {})
        overrides = [cmd for cmd in cmdtable if cmd in commands.table]
        if overrides:
            ui.warn(_("extension '%s' overrides commands: %s\n")
                    % (name, " ".join(overrides)))
        commands.table.update(cmdtable)
        _loaded[name] = 1
    # check for fallback encoding
    fallback = lui.config('ui', 'fallbackencoding')
    if fallback:
        util._fallbackencoding = fallback

    fullargs = args
    cmd, func, args, options, cmdoptions = _parse(lui, args)

    if options["config"]:
        raise util.Abort(_("Option --config may not be abbreviated!"))
    if options["cwd"]:
        raise util.Abort(_("Option --cwd may not be abbreviated!"))
    if options["repository"]:
        raise util.Abort(_(
            "Option -R has to be separated from other options (i.e. not -qR) "
            "and --repository may only be abbreviated as --repo!"))

    if options["encoding"]:
        util._encoding = options["encoding"]
    if options["encodingmode"]:
        util._encodingmode = options["encodingmode"]
    if options["time"]:
        def get_times():
            t = os.times()
            if t[4] == 0.0: # Windows leaves this as zero, so use time.clock()
                t = (t[0], t[1], t[2], t[3], time.clock())
            return t
        s = get_times()
        def print_time():
            t = get_times()
            ui.warn(_("Time: real %.3f secs (user %.3f+%.3f sys %.3f+%.3f)\n") %
                (t[4]-s[4], t[0]-s[0], t[2]-s[2], t[1]-s[1], t[3]-s[3]))
        atexit.register(print_time)

    ui.updateopts(options["verbose"], options["debug"], options["quiet"],
                 not options["noninteractive"], options["traceback"])

    if options['help']:
        return commands.help_(ui, cmd, options['version'])
    elif options['version']:
        return commands.version_(ui)
    elif not cmd:
        return commands.help_(ui, 'shortlist')

    repo = None
    if cmd not in commands.norepo.split():
        try:
            repo = hg.repository(ui, path=path)
            ui = repo.ui
            if not repo.local():
                raise util.Abort(_("repository '%s' is not local") % path)
            ui.setconfig("bundle", "mainreporoot", repo.root)
        except RepoError:
            if cmd not in commands.optionalrepo.split():
                if args and not path: # try to infer -R from command args
                    repos = map(_findrepo, args)
                    guess = repos[0]
                    if guess and repos.count(guess) == len(repos):
                        return _dispatch(ui, ['--repository', guess] + fullargs)
                if not path:
                    raise RepoError(_("There is no Mercurial repository here"
                                      " (.hg not found)"))
                raise
        d = lambda: func(ui, repo, *args, **cmdoptions)
    else:
        d = lambda: func(ui, *args, **cmdoptions)

    # run pre-hook, and abort if it fails
    ret = hook.hook(lui, repo, "pre-%s" % cmd, False, args=" ".join(fullargs))
    if ret:
        return ret
    ret = _runcommand(ui, options, cmd, d)
    # run post-hook, passing command result
    hook.hook(lui, repo, "post-%s" % cmd, False, args=" ".join(fullargs),
              result = ret)
    return ret

def _runcommand(ui, options, cmd, cmdfunc):
    def checkargs():
        try:
            return cmdfunc()
        except TypeError, inst:
            # was this an argument error?
            tb = traceback.extract_tb(sys.exc_info()[2])
            if len(tb) != 2: # no
                raise
            raise ParseError(cmd, _("invalid arguments"))

    if options['profile']:
        import hotshot, hotshot.stats
        prof = hotshot.Profile("hg.prof")
        try:
            try:
                return prof.runcall(checkargs)
            except:
                try:
                    ui.warn(_('exception raised - generating '
                             'profile anyway\n'))
                except:
                    pass
                raise
        finally:
            prof.close()
            stats = hotshot.stats.load("hg.prof")
            stats.strip_dirs()
            stats.sort_stats('time', 'calls')
            stats.print_stats(40)
    elif options['lsprof']:
        try:
            from mercurial import lsprof
        except ImportError:
            raise util.Abort(_(
                'lsprof not available - install from '
                'http://codespeak.net/svn/user/arigo/hack/misc/lsprof/'))
        p = lsprof.Profiler()
        p.enable(subcalls=True)
        try:
            return checkargs()
        finally:
            p.disable()
            stats = lsprof.Stats(p.getstats())
            stats.sort()
            stats.pprint(top=10, file=sys.stderr, climit=5)
    else:
        return checkargs()
