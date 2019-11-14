# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import string

from edenscm.mercurial import registrar
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
    hg_commit_message = repo["tip"].description()
    try:
        hg_commit_message.decode("utf8")
    except UnicodeDecodeError:
        ui.warn(_("commit message is not utf-8\n"))
        return True

    if ui.configbool("checkmessage", "allownonprintable"):
        return False

    printable = set(string.printable)
    badlines = []
    for lnum, line in enumerate(hg_commit_message.splitlines()):
        for c in line:
            if ord(c) < 128 and c not in printable:
                badlines.append((lnum + 1, line))
                break

    if badlines:
        ui.warn(_("non-printable characters in commit message\n"))
        for num, l in badlines:
            ui.warn(_("Line {}: {!r}\n".format(num, l)))

    # False means success
    return bool(badlines)
