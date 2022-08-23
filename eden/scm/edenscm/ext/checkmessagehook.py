# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import string

from edenscm.mercurial import encoding, registrar
from edenscm.mercurial.i18n import _


configtable = {}
configitem = registrar.configitem(configtable)

configitem("checkmessage", "allownonprintable", default=False)


def reposetup(ui, repo):
    ui.setconfig("hooks", "pretxncommit.checkmessage", checkcommitmessage)


def checkcommitmessage(ui, repo, **kwargs):
    """
    Checks a single commit message for adherence to commit message rules.
    """
    message = encoding.fromlocal(repo["tip"].description())

    if ui.configbool("checkmessage", "allownonprintable"):
        return False

    printable = set(string.printable)
    badlines = []
    for lnum, line in enumerate(message.splitlines()):
        for c in line:
            if ord(c) < 128 and c not in printable:
                badlines.append((lnum + 1, line))
                break

    if badlines:
        ui.warn(_("+-------------------------------------------------------------\n"))
        ui.warn(_("| Non-printable characters in commit message are not allowed.\n"))
        ui.warn(_("| Edit your commit message to fix this issue.\n"))
        ui.warn(_("| The problematic commit message can be found at:\n"))
        for num, l in badlines:
            ui.warn(_("|  Line {}: {!r}\n".format(num, l)))
        ui.warn(_("+-------------------------------------------------------------\n"))

    # False means success
    return bool(badlines)
