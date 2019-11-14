# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import time
from functools import partial

from edenscm.mercurial import error
from edenscm.mercurial.i18n import _


def generatedate(ui, commithash, commitdate):
    if ui.configbool("pushrebase", "rewritedates"):
        return (time.time(), commitdate[1])
    else:
        return commitdate


def getdatefromfile(definedcommitdates, ui, commithash, commitdate):
    try:
        return (definedcommitdates[commithash], commitdate[1])
    except KeyError:
        raise error.Abort(_("%s not found in commitdatesfile") % commithash)


def commitdategenerator(bundleoperation):
    if bundleoperation.replaydata is not None:
        return bundleoperation.replaydata.getcommitdate
    commitdatesfile = bundleoperation.ui.config("pushrebase", "commitdatesfile")
    if commitdatesfile:
        try:
            with open(commitdatesfile) as f:
                commitdates = json.loads(f.read())
                return partial(getdatefromfile, commitdates)
        except (IOError, ValueError, OSError):
            raise error.Abort(_("commitdatesfile is either nonexistent or corrupted"))
    else:
        return generatedate
