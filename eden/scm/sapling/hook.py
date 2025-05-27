# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# hook.py - hook support for mercurial
#
# Copyright 2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import os
import sys

import bindings

from sapling import hgdemandimport as demandimport

from . import encoding, error, extensions, util
from .i18n import _


def _pythonhook(ui, repo, htype, hname, funcname, args, throw):
    """call python hook. hook is callable object, looked up as
    name in python module. if callable returns "true", hook
    fails, else passes. if hook raises exception, treated as
    hook failure. exception propagates if throw is "true".

    reason for "true" meaning "hook failed" is so that
    unmodified commands (e.g. commands.update) can
    be run as hooks without wrappers to convert return values.

    This function is used by ext/mergedriver.
    """

    if callable(funcname):
        obj = funcname
        funcname = obj.__module__ + r"." + obj.__name__
    else:
        d = funcname.rfind(".")
        if d == -1:
            raise error.HookLoadError(
                _('%s hook is invalid: "%s" not in a module') % (hname, funcname)
            )
        modname = funcname[:d]
        oldpaths = sys.path
        with demandimport.deactivated():
            try:
                obj = __import__(modname)
            except (ImportError, SyntaxError):
                e1 = sys.exc_info()
                try:
                    # extensions are loaded with ext_ prefix
                    obj = __import__("sapling_ext_%s" % modname)
                except (ImportError, SyntaxError):
                    e2 = sys.exc_info()
                    ui.warn(_("exception from first failed import attempt:\n"))
                    ui.traceback(e1, force=True)
                    ui.warn(_("exception from second failed import attempt:\n"))
                    ui.traceback(e2, force=True)

                    raise error.HookLoadError(
                        _('%s hook is invalid: import of "%s" failed')
                        % (hname, modname),
                    )
        sys.path = oldpaths
        try:
            for p in funcname.split(".")[1:]:
                obj = getattr(obj, p)
        except AttributeError:
            raise error.HookLoadError(
                _('%s hook is invalid: "%s" is not defined') % (hname, funcname)
            )
        if not callable(obj):
            raise error.HookLoadError(
                _('%s hook is invalid: "%s" is not callable') % (hname, funcname)
            )

    ui.note(_("calling hook %s: %s\n") % (hname, funcname))
    starttime = util.timer()

    try:
        with util.traced("pythonhook", hookname=hname, cat="blocked"):
            r = obj(ui=ui, repo=repo, hooktype=htype, **args)
    except Exception as exc:
        if isinstance(exc, error.Abort):
            ui.warn(_("error: %s hook failed: %s\n") % (hname, exc.args[0]))
        else:
            ui.warn(_("error: %s hook raised an exception: %s\n") % (hname, str(exc)))
        if throw:
            raise
        ui.traceback(force=True)
        return True, True
    finally:
        duration = util.timer() - starttime
        ui.log(
            "pythonhook",
            "pythonhook-%s: %s finished in %0.2f seconds\n",
            htype,
            funcname,
            duration,
        )
    if r:
        if throw:
            raise error.HookAbort(_("%s hook failed") % hname)
        ui.warn(_("warning: %s hook failed\n") % hname)
    return r, False


def _pythonhook_via_pyhook(ui, repo, htype, hname, funcname, args, throw):
    ui.note(_("calling pyhook %s: %s\n") % (hname, funcname))
    starttime = util.timer()
    try:
        with util.traced("pythonhook", hookname=hname, cat="blocked"):
            r = bindings.hook.run_python_hook(repo._rsrepo, funcname, hname, args)
    except Exception as exc:
        if isinstance(exc, error.Abort):
            ui.warn(_("error: %s hook failed: %s\n") % (hname, exc.args[0]))
        else:
            ui.warn(_("error: %s hook raised an exception: %s\n") % (hname, str(exc)))
        if throw:
            raise
        ui.traceback(force=True)
        return True, True
    finally:
        duration = util.timer() - starttime
        ui.log(
            "pythonhook",
            "pythonhook-%s: %s finished in %0.2f seconds\n",
            htype,
            funcname,
            duration,
        )
    if r:
        if throw:
            raise error.HookAbort(_("%s hook failed") % hname)
        ui.warn(_("warning: %s hook failed\n") % hname)
    return r, False


def _exthook(ui, repo, htype, name, cmd, args, throw, background=False):
    ui.note(_("running hook %s: %s\n") % (name, cmd))

    starttime = util.timer()
    env = {}

    # make in-memory changes visible to external process
    if repo is not None:
        tr = repo.currenttransaction()
        repo.dirstate.write(tr)
        if tr and tr.writepending(env=env):
            env["HG_PENDING"] = repo.root
            env["HG_SHAREDPENDING"] = repo.sharedroot
    env["HG_HOOKTYPE"] = htype
    env["HG_HOOKNAME"] = name

    cri = bindings.clientinfo.get_client_request_info()
    env["SAPLING_CLIENT_ENTRY_POINT"] = cri["entry_point"]
    env["SAPLING_CLIENT_CORRELATOR"] = cri["correlator"]

    for k, v in args.items():
        if callable(v):
            v = v()
        if isinstance(v, dict):
            # make the dictionary element order stable across Python
            # implementations
            v = "{" + ", ".join("%r: %r" % i for i in sorted(v.items())) + "}"
        env["HG_" + k.upper()] = v

    if repo:
        cwd = repo.root
    else:
        cwd = os.getcwd()

    if background:
        full_env = util.shellenviron(env)
        try:
            util.spawndetached(cmd, cwd=cwd, env=full_env, shell=True)
        except Exception as e:
            ui.warn(_("warning: %s hook failed to run: %r\n") % (name, e))
            return 1
        return 0

    r = ui.system(cmd, environ=env, cwd=cwd, blockedtag="exthook")

    duration = util.timer() - starttime
    ui.log("exthook", "exthook-%s: %s finished in %0.2f seconds\n", name, cmd, duration)
    if r:
        desc, r = util.explainexit(r)
        if throw:
            raise error.HookAbort(_("%s hook %s") % (name, desc))
        ui.warn(_("warning: %s hook %s\n") % (name, desc))
    return r


def _allhooks(ui):
    """return a list of (hook-id, cmd) pairs sorted by priority"""
    hooks = _hookitems(ui)
    return [(k, v) for p, o, k, v in sorted(hooks.values())]


def _hookitems(ui):
    """return all hooks items ready to be sorted"""
    hooks = {}
    for name, cmd in ui.configitems("hooks"):
        if not name.startswith("priority"):
            priority = ui.configint("hooks", "priority.%s" % name, 0)
            hooks[name] = (-priority, len(hooks), name, cmd)
    return hooks


def hashook(ui, htype) -> bool:
    """return True if a hook is configured for 'htype'"""
    if not ui.callhooks:
        return False
    for hname, cmd in _allhooks(ui):
        if hname.split(".")[0] == htype and cmd:
            return True
    return False


def hook(ui, repo, htype, throw: bool = False, skipshell: bool = False, **args) -> bool:
    if not ui.callhooks:
        return False

    hooks = []
    for hname, cmd in _allhooks(ui):
        if hname.split(".")[0] == htype and cmd:
            # This is for Rust commands that already ran "pre" hooks and then fell back to
            # Python. Rust doesn't support python hooks, so let's run those.
            if skipshell and not callable(cmd) and not cmd.startswith("python:"):
                continue
            hooks.append((hname, cmd))

    if not hooks:
        return False

    res = runhooks(ui, repo, htype, hooks, throw=throw, **args)
    r = False
    for hname, cmd in hooks:
        r = res[hname][0] or r
    return r


def runhooks(ui, repo, htype, hooks, throw: bool = False, **args):
    args = args
    res = {}

    for hname, cmd in hooks:
        if callable(cmd):
            r, raised = _pythonhook(ui, repo, htype, hname, cmd, args, throw)
        elif cmd.startswith("python:"):
            if (
                ui.configbool("experimental", "run-python-hooks-via-pyhook")
                and repo is not None
            ):
                funcname = cmd.split(":", 1)[1]
                r, raised = _pythonhook_via_pyhook(
                    ui, repo, htype, hname, funcname, args, throw
                )
            else:
                hookfn = _getpyhook(ui, repo, hname, cmd)
                r, raised = _pythonhook(ui, repo, htype, hname, hookfn, args, throw)
        elif cmd.startswith("background:"):
            # Run a shell command in background. Do not throw.
            cmd = cmd.split(":", 1)[1]
            r = _exthook(
                ui, repo, htype, hname, cmd, args, throw=False, background=True
            )
            raised = False
        else:
            r = _exthook(ui, repo, htype, hname, cmd, args, throw)
            raised = False

        res[hname] = r, raised

        # The stderr is fully buffered on Windows when connected to a pipe.
        # A forcible flush is required to make small stderr data in the
        # remote side available to the client immediately.
        util.stderr.flush()

    return res


def _getpyhook(ui, repo, hname, cmd):
    if not cmd.startswith("python:"):
        raise error.ProgrammingError("getpyhook called without python: prefix")

    if cmd.count(":") >= 2:
        path, cmd = cmd[7:].rsplit(":", 1)
        path = util.expandpath(path)
        if repo:
            path = os.path.join(repo.root, path)
        try:
            mod = extensions.loadpath(path, "hghook.%s" % hname)
        except Exception as e:
            ui.write(_("loading %s hook failed: %s\n") % (hname, e))
            raise
        hookfn = getattr(mod, cmd)
    else:
        hookfn = cmd[7:].strip()
        # Compatibility: Change "ext" to "sapling.ext"
        # automatically.
        if hookfn.startswith("ext."):
            hookfn = "sapling." + hookfn

    return hookfn
