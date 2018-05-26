# Copyright 2016-present Facebook. All Rights Reserved.
#
# print server fqdn during a remote session

import socket

from mercurial import wireproto
from mercurial.extensions import wrapfunction
from mercurial.i18n import _


def extsetup(ui):

    def printhostname(orig, *args, **kwargs):
        ui.warn(_("hostname") + ": " + socket.getfqdn())
        return orig(*args, **kwargs)

    wrapfunction(wireproto, "_capabilities", printhostname)
