# hook.py - hook support for mercurial
#
# Copyright 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import sys
import time

from .i18n import _
from . import (
    demandimport,
    error,
    extensions,
    util,
)

def _pythonhook(ui, repo, name, hname, funcname, args, throw):
    '''call python hook. hook is callable object, looked up as
    name in python module. if callable returns "true", hook
    fails, else passes. if hook raises exception, treated as
    hook failure. exception propagates if throw is "true".

    reason for "true" meaning "hook failed" is so that
    unmodified commands (e.g. mercurial.commands.update) can
    be run as hooks without wrappers to convert return values.'''

    if callable(funcname):
        obj = funcname
        funcname = obj.__module__ + "." + obj.__name__
    else:
        d = funcname.rfind('.')
        if d == -1:
            raise error.HookLoadError(
                _('%s hook is invalid: "%s" not in a module')
                % (hname, funcname))
        modname = funcname[:d]
        oldpaths = sys.path
        if util.mainfrozen():
            # binary installs require sys.path manipulation
            modpath, modfile = os.path.split(modname)
            if modpath and modfile:
                sys.path = sys.path[:] + [modpath]
                modname = modfile
        with demandimport.deactivated():
            try:
                obj = __import__(modname)
            except (ImportError, SyntaxError):
                e1 = sys.exc_info()
                try:
                    # extensions are loaded with hgext_ prefix
                    obj = __import__("hgext_%s" % modname)
                except (ImportError, SyntaxError):
                    e2 = sys.exc_info()
                    if ui.tracebackflag:
                        ui.warn(_('exception from first failed import '
                                  'attempt:\n'))
                    ui.traceback(e1)
                    if ui.tracebackflag:
                        ui.warn(_('exception from second failed import '
                                  'attempt:\n'))
                    ui.traceback(e2)

                    if not ui.tracebackflag:
                        tracebackhint = _(
                            'run with --traceback for stack trace')
                    else:
                        tracebackhint = None
                    raise error.HookLoadError(
                        _('%s hook is invalid: import of "%s" failed') %
                        (hname, modname), hint=tracebackhint)
        sys.path = oldpaths
        try:
            for p in funcname.split('.')[1:]:
                obj = getattr(obj, p)
        except AttributeError:
            raise error.HookLoadError(
                _('%s hook is invalid: "%s" is not defined')
                % (hname, funcname))
        if not callable(obj):
            raise error.HookLoadError(
                _('%s hook is invalid: "%s" is not callable')
                % (hname, funcname))

    ui.note(_("calling hook %s: %s\n") % (hname, funcname))
    starttime = time.time()

    try:
        # redirect IO descriptors to the ui descriptors so hooks
        # that write directly to these don't mess up the command
        # protocol when running through the command server
        old = sys.stdout, sys.stderr, sys.stdin
        sys.stdout, sys.stderr, sys.stdin = ui.fout, ui.ferr, ui.fin

        r = obj(ui=ui, repo=repo, hooktype=name, **args)
    except Exception as exc:
        if isinstance(exc, error.Abort):
            ui.warn(_('error: %s hook failed: %s\n') %
                         (hname, exc.args[0]))
        else:
            ui.warn(_('error: %s hook raised an exception: '
                           '%s\n') % (hname, exc))
        if throw:
            raise
        if not ui.tracebackflag:
            ui.warn(_('(run with --traceback for stack trace)\n'))
        ui.traceback()
        return True, True
    finally:
        sys.stdout, sys.stderr, sys.stdin = old
        duration = time.time() - starttime
        ui.log('pythonhook', 'pythonhook-%s: %s finished in %0.2f seconds\n',
               name, funcname, duration)
    if r:
        if throw:
            raise error.HookAbort(_('%s hook failed') % hname)
        ui.warn(_('warning: %s hook failed\n') % hname)
    return r, False

def _exthook(ui, repo, name, cmd, args, throw):
    ui.note(_("running hook %s: %s\n") % (name, cmd))

    starttime = time.time()
    env = {}

    # make in-memory changes visible to external process
    if repo is not None:
        tr = repo.currenttransaction()
        repo.dirstate.write(tr)
        if tr and tr.writepending():
            env['HG_PENDING'] = repo.root

    for k, v in args.iteritems():
        if callable(v):
            v = v()
        if isinstance(v, dict):
            # make the dictionary element order stable across Python
            # implementations
            v = ('{' +
                 ', '.join('%r: %r' % i for i in sorted(v.iteritems())) +
                 '}')
        env['HG_' + k.upper()] = v

    if repo:
        cwd = repo.root
    else:
        cwd = os.getcwd()
    r = ui.system(cmd, environ=env, cwd=cwd)

    duration = time.time() - starttime
    ui.log('exthook', 'exthook-%s: %s finished in %0.2f seconds\n',
           name, cmd, duration)
    if r:
        desc, r = util.explainexit(r)
        if throw:
            raise error.HookAbort(_('%s hook %s') % (name, desc))
        ui.warn(_('warning: %s hook %s\n') % (name, desc))
    return r

# represent an untrusted hook command
_fromuntrusted = object()

def _allhooks(ui):
    """return a list of (hook-id, cmd) pairs sorted by priority"""
    hooks = _hookitems(ui)
    # Be careful in this section, propagating the real commands from untrusted
    # sources would create a security vulnerability, make sure anything altered
    # in that section uses "_fromuntrusted" as its command.
    untrustedhooks = _hookitems(ui, _untrusted=True)
    for name, value in untrustedhooks.items():
        trustedvalue = hooks.get(name, (None, None, name, _fromuntrusted))
        if value != trustedvalue:
            (lp, lo, lk, lv) = trustedvalue
            hooks[name] = (lp, lo, lk, _fromuntrusted)
    # (end of the security sensitive section)
    return [(k, v) for p, o, k, v in sorted(hooks.values())]

def _hookitems(ui, _untrusted=False):
    """return all hooks items ready to be sorted"""
    hooks = {}
    for name, cmd in ui.configitems('hooks', untrusted=_untrusted):
        if not name.startswith('priority'):
            priority = ui.configint('hooks', 'priority.%s' % name, 0)
            hooks[name] = (-priority, len(hooks), name, cmd)
    return hooks

_redirect = False
def redirect(state):
    global _redirect
    _redirect = state

def hook(ui, repo, name, throw=False, **args):
    if not ui.callhooks:
        return False

    hooks = []
    for hname, cmd in _allhooks(ui):
        if hname.split('.')[0] == name and cmd:
            hooks.append((hname, cmd))

    res = runhooks(ui, repo, name, hooks, throw=throw, **args)
    r = False
    for hname, cmd in hooks:
        r = res[hname][0] or r
    return r

def runhooks(ui, repo, name, hooks, throw=False, **args):
    res = {}
    oldstdout = -1

    try:
        for hname, cmd in hooks:
            if oldstdout == -1 and _redirect:
                try:
                    stdoutno = sys.__stdout__.fileno()
                    stderrno = sys.__stderr__.fileno()
                    # temporarily redirect stdout to stderr, if possible
                    if stdoutno >= 0 and stderrno >= 0:
                        sys.__stdout__.flush()
                        oldstdout = os.dup(stdoutno)
                        os.dup2(stderrno, stdoutno)
                except (OSError, AttributeError):
                    # files seem to be bogus, give up on redirecting (WSGI, etc)
                    pass

            if cmd is _fromuntrusted:
                if throw:
                    raise error.HookAbort(
                        _('untrusted hook %s not executed') % name,
                        hint = _("see 'hg help config.trusted'"))
                ui.warn(_('warning: untrusted hook %s not executed\n') % name)
                r = 1
                raised = False
            elif callable(cmd):
                r, raised = _pythonhook(ui, repo, name, hname, cmd, args, throw)
            elif cmd.startswith('python:'):
                if cmd.count(':') >= 2:
                    path, cmd = cmd[7:].rsplit(':', 1)
                    path = util.expandpath(path)
                    if repo:
                        path = os.path.join(repo.root, path)
                    try:
                        mod = extensions.loadpath(path, 'hghook.%s' % hname)
                    except Exception:
                        ui.write(_("loading %s hook failed:\n") % hname)
                        raise
                    hookfn = getattr(mod, cmd)
                else:
                    hookfn = cmd[7:].strip()
                r, raised = _pythonhook(ui, repo, name, hname, hookfn, args,
                                        throw)
            else:
                r = _exthook(ui, repo, hname, cmd, args, throw)
                raised = False

            res[hname] = r, raised

            # The stderr is fully buffered on Windows when connected to a pipe.
            # A forcible flush is required to make small stderr data in the
            # remote side available to the client immediately.
            sys.stderr.flush()
    finally:
        if _redirect and oldstdout >= 0:
            os.dup2(oldstdout, stdoutno)
            os.close(oldstdout)

    return res
