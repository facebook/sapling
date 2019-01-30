# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from edenscm.mercurial import bundlerepo, extensions
from edenscm.mercurial.i18n import _


def extsetup(ui):
    extensions.wrapfunction(bundlerepo.bundlerepository, "__init__", _init)


def _init(orig, self, ui, *args, **kwargs):
    ui.warn(_("creating bundlerepo"))
    return orig(self, ui, *args, **kwargs)
