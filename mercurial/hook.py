# hook.py - hook support for mercurial
#
# Copyright 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from i18n import _
import os, sys
import extensions, util

def _pythonhook(ui, repo, name, hname, funcname, args, throw):
    '''call python hook. hook is callable object, looked up as
    name in python module. if callable returns "true", hook
    fails, else passes. if hook raises exception, treated as
    hook failure. exception propagates if throw is "true".

    reason for "true" meaning "hook failed" is so that
    unmodified commands (e.g. mercurial.commands.update) can
    be run as hooks without wrappers to convert return values.'''

    ui.note(_("calling hook %s: %s\n") % (hname, funcname))
    obj = funcname
    if not hasattr(obj, '__call__'):
        d = funcname.rfind('.')
        if d == -1:
            raise util.Abort(_('%s hook is invalid ("%s" not in '
                               'a module)') % (hname, funcname))
        modname = funcname[:d]
        oldpaths = sys.path[:]
        if hasattr(sys, "frozen"):
            # binary installs require sys.path manipulation
            path, name = os.path.split(modname)
            if path and name:
                sys.path.append(path)
                modname = name
        try:
            obj = __import__(modname)
        except ImportError:
            try:
                # extensions are loaded with hgext_ prefix
                obj = __import__("hgext_%s" % modname)
            except ImportError:
                raise util.Abort(_('%s hook is invalid '
                                   '(import of "%s" failed)') %
                                 (hname, modname))
        sys.path = oldpaths
        try:
            for p in funcname.split('.')[1:]:
                obj = getattr(obj, p)
        except AttributeError:
            raise util.Abort(_('%s hook is invalid '
                               '("%s" is not defined)') %
                             (hname, funcname))
        if not hasattr(obj, '__call__'):
            raise util.Abort(_('%s hook is invalid '
                               '("%s" is not callable)') %
                             (hname, funcname))
    try:
        r = obj(ui=ui, repo=repo, hooktype=name, **args)
    except KeyboardInterrupt:
        raise
    except Exception, exc:
        if isinstance(exc, util.Abort):
            ui.warn(_('error: %s hook failed: %s\n') %
                         (hname, exc.args[0]))
        else:
            ui.warn(_('error: %s hook raised an exception: '
                           '%s\n') % (hname, exc))
        if throw:
            raise
        ui.traceback()
        return True
    if r:
        if throw:
            raise util.Abort(_('%s hook failed') % hname)
        ui.warn(_('warning: %s hook failed\n') % hname)
    return r

def _exthook(ui, repo, name, cmd, args, throw):
    ui.note(_("running hook %s: %s\n") % (name, cmd))

    env = {}
    for k, v in args.iteritems():
        if hasattr(v, '__call__'):
            v = v()
        env['HG_' + k.upper()] = v

    if repo:
        cwd = repo.root
    else:
        cwd = os.getcwd()
    r = util.system(cmd, environ=env, cwd=cwd)
    if r:
        desc, r = util.explain_exit(r)
        if throw:
            raise util.Abort(_('%s hook %s') % (name, desc))
        ui.warn(_('warning: %s hook %s\n') % (name, desc))
    return r

_redirect = False
def redirect(state):
    global _redirect
    _redirect = state

def hook(ui, repo, name, throw=False, **args):
    r = False

    if _redirect:
        # temporarily redirect stdout to stderr
        oldstdout = os.dup(sys.__stdout__.fileno())
        os.dup2(sys.__stderr__.fileno(), sys.__stdout__.fileno())

    try:
        for hname, cmd in ui.configitems('hooks'):
            if hname.split('.')[0] != name or not cmd:
                continue
            if hasattr(cmd, '__call__'):
                r = _pythonhook(ui, repo, name, hname, cmd, args, throw) or r
            elif cmd.startswith('python:'):
                if cmd.count(':') >= 2:
                    path, cmd = cmd[7:].rsplit(':', 1)
                    mod = extensions.loadpath(path, 'hghook.%s' % hname)
                    hookfn = getattr(mod, cmd)
                else:
                    hookfn = cmd[7:].strip()
                r = _pythonhook(ui, repo, name, hname, hookfn, args, throw) or r
            else:
                r = _exthook(ui, repo, hname, cmd, args, throw) or r
    finally:
        if _redirect:
            os.dup2(oldstdout, sys.__stdout__.fileno())
            os.close(oldstdout)

    return r
