# Copyright 2017-present Facebook. All Rights Reserved.
#
# faster copytrace implementation
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from functools import partial, update_wrapper
from mercurial import commands, dispatch, extensions, filemerge, util
from mercurial.i18n import _
from mercurial.node import hex, wdirid

import os

_copytracinghint = ("hint: if this message is due to a moved file, you can " +
                    "ask mercurial to attempt to automatically resolve this " +
                    "change by re-running with the --tracecopies flag, but " +
                    "this will significantly slow down the operation, so you " +
                    "will need to be patient.\n" +
                    "Source control team is working on fixing this problem.\n")

def uisetup(ui):
    extensions.wrapfunction(dispatch, "runcommand", _runcommand)

def extsetup(ui):
    commands.globalopts.append(
        ("", "tracecopies", None,
         _("enable copytracing. Warning: can be very slow!")))

    # With experimental.disablecopytrace=True there can be cryptic merge errors.
    # Let"s change error message to suggest re-running the command with
    # enabled copytracing
    filemerge._localchangedotherdeletedmsg = _(
        "local%(l)s changed %(fd)s which other%(o)s deleted\n" +
        _copytracinghint +
        "use (c)hanged version, (d)elete, or leave (u)nresolved?"
        "$$ &Changed $$ &Delete $$ &Unresolved")

    filemerge._otherchangedlocaldeletedmsg = _(
        "other%(o)s changed %(fd)s which local%(l)s deleted\n" +
        _copytracinghint +
        "use (c)hanged version, leave (d)eleted, or leave (u)nresolved?"
        "$$ &Changed $$ &Deleted $$ &Unresolved")

    origpromptmerge = filemerge.internals[':prompt']
    wrapperpromptmerge = partial(_promptmerge, origpromptmerge)
    update_wrapper(wrapperpromptmerge, origpromptmerge)
    # wrap function everywhere as in filemerge.internaltool
    filemerge.internals[':prompt'] = wrapperpromptmerge
    filemerge.internalsdoc[':prompt'] = wrapperpromptmerge
    filemerge.internals['internal:prompt'] = wrapperpromptmerge

def _runcommand(orig, lui, repo, cmd, fullargs, ui, *args, **kwargs):
    if "--tracecopies" in fullargs:
        ui.setconfig("experimental", "disablecopytrace",
                     False, "--tracecopies")
    return orig(lui, repo, cmd, fullargs, ui, *args, **kwargs)

def _promptmerge(origfunc, repo, mynode, orig, fcd, fco, *args, **kwargs):
    ui = repo.ui
    try:
        ctx1 = _getctxfromfctx(fco)
        ctx2 = _getctxfromfctx(fcd)
        msg = [(ctx1.phase(), _gethex(ctx1)), (ctx2.phase(), _gethex(ctx2))]

        reporoot = repo.origroot if util.safehasattr(repo, 'origroot') else ''
        reponame = ui.config('paths', 'default', reporoot)
        if reponame:
            reponame = os.path.basename(reponame)
        if fco.isabsent() or fcd.isabsent():
            ui.log("promptmerge", "", mergechangeddeleted=('%s' % msg),
                   reponame=reponame)
    except Exception as e:
        # since it's just a logging we don't want a error in this code to break
        # clients
        ui.log("promptmerge", "", failed='%s' % e)
    return origfunc(repo, mynode, orig, fcd, fco, *args, **kwargs)

def _getctxfromfctx(fctx):
    if fctx.isabsent():
        return fctx._ctx
    else:
        return fctx._changectx

def _gethex(ctx):
    # for workingctx return p1 hex
    return ctx.hex() if ctx.hex() != hex(wdirid) else ctx.p1().hex()
