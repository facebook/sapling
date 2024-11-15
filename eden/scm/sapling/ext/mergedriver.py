# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""custom merge drivers for autoresolved files"""

from __future__ import absolute_import

import errno
import sys
import weakref

from sapling import commands, error, extensions, hook, merge, perftrace
from sapling.i18n import _


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
                "(@prog@ resolve --all to retry, or "
                "@prog@ resolve --all --skip to skip merge driver)\n"
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
                "(@prog@ resolve --all to retry, or "
                "@prog@ resolve --all --skip to skip merge driver)\n"
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


# Cache the loaded preprocess func to avoid completely reloading the mergedriver for every
# commit during in-memory rebase.
#
# Looks like (weakref.ref(wctx), set(sys.modules.keys()), preprocess func)
_cached_driver = None


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

    # Only try caching for in-memory "preprocess" step. If we aren't in memory, we need to
    # re-load the merge driver code from disk each time. If we are in memory, we can cache
    # based on wctx. Note that in-memory rebase still loads merge driver code from disk.
    # We only cache for "preprocess" to keep things a tad simpler (in-memory rebase never
    # gets to the conclude() step).
    try_caching = wctx.isinmemory() and op == "preprocess"

    global _cached_driver

    hookfn = None
    origmodules = set(sys.modules.keys())

    # Check wctx for "equality" using `is`. We basically just want to check we are dealing
    # with the same wctx object as last time, even though it's parent node has changed.
    if try_caching and _cached_driver and _cached_driver[0]() is wctx:
        origmodules = _cached_driver[1]
        hookfn = _cached_driver[2]
    elif _cached_driver:
        # If we aren't using the cached value, unload all modules loaded by the currently
        # cached merge driver (so we do a fresh load below).
        for mod in set(sys.modules.keys()) - _cached_driver[1]:
            del sys.modules[mod]
        _cached_driver = None

    try:
        if not hookfn:
            hookfn = hook._getpyhook(ui, repo, op, f"{mergedriver}:{op}")
            if try_caching:
                _cached_driver = (weakref.ref(wctx), origmodules, hookfn)

        r, raised = hook._pythonhook(
            ui,
            repo,
            "mergedriver-%s" % op,
            op,
            hookfn,
            {
                "mergestate": ms,
                "wctx": wctx,
                "labels": labels,
            },
            False,
        )
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
        if not _cached_driver:
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
