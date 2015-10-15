# mergedriver.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""custom merge drivers for autoresolved files"""

from __future__ import absolute_import

from mercurial.i18n import _

from mercurial import (
    error,
    extensions,
    hook,
    merge,
    util,
)

def wrappreprocess(orig, repo, ms, wctx, labels=None):
    ui = repo.ui
    r, raised = _rundriver(repo, ms, 'preprocess', wctx, labels)

    ms.commit()

    if raised:
        ms._mdstate = 'u'
        ms._dirty = True
        ms.commit()
        ui.warn(_('warning: merge driver failed to preprocess files\n'))
        return False
    elif r or list(ms.driverresolved()):
        ms._mdstate = 'm'
    else:
        ms._mdstate = 's'

    ms._dirty = True
    ms.commit()
    return True

def wrapconclude(orig, repo, ms, wctx, labels=None):
    ui = repo.ui
    r, raised = _rundriver(repo, ms, 'conclude', wctx, labels)
    ms.commit()

    if raised:
        ms._mdstate = 'u'
        ms._dirty = True
        ms.commit()
        ui.warn(_('warning: merge driver failed to resolve files\n'))
        return False
    # assume that driver-resolved files have all been resolved
    driverresolved = list(ms.driverresolved())
    for f in driverresolved:
        ms.mark(f, 'r')
    ms._mdstate = 's'
    ms._dirty = True
    ms.commit()
    return True

def _rundriver(repo, ms, op, wctx, labels):
    ui = repo.ui
    mergedriver = ms.mergedriver
    if not mergedriver.startswith('python:'):
        raise error.ConfigError(_("merge driver must be a python hook"))
    ms.commit()
    raised = False
    try:
        res = hook.runhooks(ui, repo, 'mergedriver-%s' % op,
                            [(op, '%s:%s' % (mergedriver, op))],
                            throw=False, mergestate=ms, wctx=wctx,
                            labels=labels)
        r, raised = res[op]
    except ImportError:
        # underlying function prints out warning
        r = True
        raised = True
    except (IOError, error.HookLoadError) as inst:
        ui.warn(_("%s\n") % inst)
        r = True
        raised = True

    return r, raised

def extsetup(ui):
    extensions.wrapfunction(merge, 'driverpreprocess', wrappreprocess)
    extensions.wrapfunction(merge, 'driverconclude', wrapconclude)
