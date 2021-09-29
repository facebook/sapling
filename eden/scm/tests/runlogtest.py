# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os.path
import time

from edenscm.mercurial import registrar


cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "basiccommandtest",
    [
        (
            "",
            "waitfile",
            "",
            "if set, wait for file before exitting",
        ),
    ],
    "hg basiccommandtest exit_code",
    norepo=True,
)
def basiccommandtest(ui, exit_code, **opts):
    waitforfile(opts.get("waitfile"))
    exit(int(exit_code))


def waitforfile(path):
    if not path:
        return

    while not os.path.exists(path):
        time.sleep(0.001)

    os.unlink(path)
