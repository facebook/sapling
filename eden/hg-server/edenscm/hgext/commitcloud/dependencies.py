# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm.mercurial import error, extensions
from edenscm.mercurial.i18n import _


infinitepush = None


def extsetup(ui):
    global infinitepush
    try:
        infinitepush = extensions.find("infinitepush")
    except KeyError:
        msg = _("The commitcloud extension requires the infinitepush extension")
        raise error.Abort(msg)
