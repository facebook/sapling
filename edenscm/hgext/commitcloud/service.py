# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm.mercurial import error

from . import httpsservice, localservice


def get(ui, token=None):
    servicetype = ui.config("commitcloud", "servicetype")
    if servicetype == "local":
        return localservice.LocalService(ui)
    elif servicetype == "remote":
        return httpsservice.HttpsCommitCloudService(ui, token)
    else:
        msg = "Unrecognized commitcloud.servicetype: %s" % servicetype
        raise error.Abort(msg)
