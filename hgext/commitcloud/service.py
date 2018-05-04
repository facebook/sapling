# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Mercurial
from mercurial import error

from . import (
    httpsservice,
    localservice,
)

def get(ui, token=None):
    servicetype = ui.config('commitcloud', 'servicetype')
    if servicetype == 'local':
        return localservice.LocalService(ui)
    elif servicetype == 'interngraph':
        return httpsservice.HttpsCommitCloudService(ui, token)
    else:
        msg = 'Unrecognized commitcloud.servicetype: %s' % servicetype
        raise error.Abort(msg)
