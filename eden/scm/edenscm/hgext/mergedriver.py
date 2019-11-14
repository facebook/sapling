# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""custom merge drivers for autoresolved files"""

from __future__ import absolute_import

import errno
import sys

from edenscm.mercurial import commands, error, extensions, hook, merge, perftrace
from edenscm.mercurial.i18n import _


@perftrace.tracefunc("Merge Driver Preprocess")
def wrappreprocess(orig, repo, ms, wctx, labels=None):
    ui = repo.ui
    r, raised = _rundriver(repo, ms, "preprocess", wctx, labels)

    ms.commit()

    if raised:
        ms._mdstate = "u"
        ms._dirty = True
        ms.commit()
        ui.warn(_("warning: merge driver failed to preprocess files\n"))
        ui.warn(
            _(
                "(hg resolve --all to retry, or "
                "hg resolve --all --skip to skip merge driver)\n"
            )
        )
        return False
    elif r or list(ms.driverresolved()):
        ms._mdstate = "m"
    else:
        ms._mdstate = "s"

    ms._dirty = True
    ms.commit()
    return True


@perftrace.tracefunc("Merge Driver Conclude")
def wrapconclude(orig, repo, ms, wctx, labels=None):
    ui = repo.ui
    r, raised = _rundriver(repo, ms, "conclude", wctx, labels)
    ms.commit()

    if raised:
        ms._mdstate = "u"
        ms._dirty = True
        ms.commit()
        ui.warn(_("warning: merge driver failed to resolve files\n"))
        ui.warn(
            _(
                "(hg resolve --all to retry, or "
                "hg resolve --all --skip to skip merge driver)\n"
            )
        )
        return False
    # assume that driver-resolved files have all been resolved
    driverresolved = list(ms.driverresolved())
    for f in driverresolved:
        ms.mark(f, "r")
    ms._mdstate = "s"
    ms._dirty = True
    ms.commit()
    return True


def wrapmdprop(orig, self):
    try:
        return orig(self)
    except error.ConfigError:
        # skip this error and go with the new one
        self._dirty = True
        return self._repo.ui.config("experimental", "mergedriver")


def wrapresolve(orig, ui, repo, *pats, **opts):
    backup = None
    overrides = {}
    if opts.get("skip"):
        backup = ui.config("experimental", "mergedriver")
        overrides[("experimental", "mergedriver")] = ""
        ui.warn(
            _(
                "warning: skipping merge driver "
                "(you MUST regenerate artifacts afterwards)\n"
            )
        )

    with ui.configoverride(overrides, "mergedriver"):
        ret = orig(ui, repo, *pats, **opts)
        # load up and commit the merge state again to make sure the driver gets
        # written out
        if backup is not None:
            with repo.wlock():
                ms = merge.mergestate.read(repo)
                if opts.get("skip"):
                    # force people to resolve by hand
                    for f in ms.driverresolved():
                        ms.mark(f, "u")
                ms.commit()
        return ret


def _rundriver(repo, ms, op, wctx, labels):
    ui = repo.ui
    mergedriver = ms.mergedriver
    if not mergedriver.startswith("python:"):
        raise error.ConfigError(_("merge driver must be a python hook"))
    ms.commit()
    raised = False
    # Don't write .pyc files for the loaded hooks (restore this setting
    # after running). Like the `loadedmodules` fix below, this is to prevent
    # drivers changed during a rebase from being loaded inconsistently.
    origbytecodesetting = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    origmodules = set(sys.modules.keys())
    try:
        res = hook.runhooks(
            ui,
            repo,
            "mergedriver-%s" % op,
            [(op, "%s:%s" % (mergedriver, op))],
            throw=False,
            mergestate=ms,
            wctx=wctx,
            labels=labels,
        )
        r, raised = res[op]
    except ImportError:
        # underlying function prints out warning
        r = True
        raised = True
    except (IOError, error.HookLoadError) as inst:
        if isinstance(inst, IOError) and inst.errno == errno.ENOENT:
            # this will usually happen when transitioning from not having a
            # merge driver to having one -- don't fail for this important use
            # case
            r, raised = False, False
        else:
            ui.warn(_("%s\n") % inst)
            r = True
            raised = True
    finally:
        # Evict the loaded module and all of its imports from memory. This is
        # necessary to ensure we always use the latest driver code from ., and
        # prevent cases with a half-loaded driver (where some of the cached
        # modules were loaded from an older commit.)
        loadedmodules = set(sys.modules.keys()) - origmodules
        for mod in loadedmodules:
            del sys.modules[mod]
        sys.dont_write_bytecode = origbytecodesetting
    return r, raised


def extsetup(ui):
    extensions.wrapfunction(merge, "driverpreprocess", wrappreprocess)
    extensions.wrapfunction(merge, "driverconclude", wrapconclude)
    wrappropertycache(merge.mergestate, "mergedriver", wrapmdprop)
    entry = extensions.wrapcommand(commands.table, "resolve", wrapresolve)
    entry[1].append(("", "skip", None, _("skip merge driver")))


def wrappropertycache(cls, propname, wrapper):
    """Wraps a filecache property. These can't be wrapped using the normal
    wrapfunction. This should eventually go into upstream Mercurial.
    """
    assert callable(wrapper)
    for currcls in cls.__mro__:
        if propname in currcls.__dict__:
            origfn = currcls.__dict__[propname].func
            assert callable(origfn)

            def wrap(*args, **kwargs):
                return wrapper(origfn, *args, **kwargs)

            currcls.__dict__[propname].func = wrap
            break

    if currcls is object:
        raise AttributeError(_("%s has no property '%s'") % (type(currcls), propname))
